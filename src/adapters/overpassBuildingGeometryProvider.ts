import type {
  BuildingTarget,
  FootprintComponent,
  GeographicBounds,
  LngLatPoint
} from "../domain/buildingGeometry";
import { createBuildingGeometryObservation } from "../services/buildingObservation";
import { scoreObservationIdentity } from "../services/buildingIdentity";
import type { BuildingGeometryProvider, PartialGeometry } from "./minimalArnisAdapter";

interface OverpassGeometryPoint {
  lat: number;
  lon: number;
}

interface OverpassRelationMember {
  type: "way" | "node" | "relation";
  ref: number;
  role?: string;
  geometry?: OverpassGeometryPoint[];
}

interface OverpassBuildingElement {
  type: "way" | "relation";
  id: number;
  tags?: Record<string, string>;
  geometry?: OverpassGeometryPoint[];
  members?: OverpassRelationMember[];
}

interface OverpassBuildingResponse {
  elements?: OverpassBuildingElement[];
}

const DEFAULT_RADIUS_M = 120;
const MAX_RADIUS_M = 250;
const QUERY_LIMIT = 100;

export class OverpassBuildingGeometryProvider implements BuildingGeometryProvider {
  readonly source = "osm_overpass" as const;

  constructor(
    private readonly options: {
      endpoint?: string;
      fetchJson?: typeof fetch;
      radiusM?: number;
      timeoutMs?: number;
    } = {}
  ) {}

  async fetchBuildingGeometry(target: BuildingTarget): Promise<PartialGeometry | null> {
    const endpoint = this.options.endpoint ?? "https://overpass-api.de/api/interpreter";
    const radiusM = clampRadius(this.options.radiusM ?? DEFAULT_RADIUS_M);
    const controller = new AbortController();
    const timeout = globalThis.setTimeout(() => controller.abort(), this.options.timeoutMs ?? 20_000);
    try {
      const response = await (this.options.fetchJson ?? fetch)(endpoint, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8" },
        body: new URLSearchParams({ data: buildBuildingQuery(target, radiusM) }),
        signal: controller.signal
      });
      if (!response.ok) throw new Error(`Overpass building request failed: ${response.status}`);
      const payload = (await response.json()) as OverpassBuildingResponse;
      const candidates = usableBuildingElements((payload.elements ?? []).slice(0, QUERY_LIMIT));
      const observations = candidates.map(({ element, components }) =>
        createOverpassObservation(element, components, radiusM)
      );
      const selected = selectBuildingElement(candidates, observations, target);
      if (!selected) return null;
      const geometry = overpassElementToPartialGeometry(selected.element, selected.components, target, radiusM);
      geometry.observations = observations;
      return geometry;
    } finally {
      globalThis.clearTimeout(timeout);
    }
  }
}

function buildBuildingQuery(target: BuildingTarget, radiusM: number) {
  const { lat, lng } = target.approximateCenter;
  return `
[out:json][timeout:20];
(
  way(around:${radiusM},${lat},${lng})["building"];
  relation(around:${radiusM},${lat},${lng})["building"];
);
out body ${QUERY_LIMIT} geom;
`.trim();
}

function selectBuildingElement(
  usable: Array<{ element: OverpassBuildingElement; components: FootprintComponent[] }>,
  observations: ReturnType<typeof createOverpassObservation>[],
  target: BuildingTarget
): { element: OverpassBuildingElement; components: FootprintComponent[] } | null {
  usable.sort((left, right) => {
    const leftObservation = observations.find((item) => item.id === rawElementId(left.element));
    const rightObservation = observations.find((item) => item.id === rawElementId(right.element));
    const scoreDifference = (rightObservation ? scoreObservationIdentity(target, rightObservation).score : 0) -
      (leftObservation ? scoreObservationIdentity(target, leftObservation).score : 0);
    return scoreDifference ||
      distanceToTarget(left.components, target) - distanceToTarget(right.components, target);
  });
  return usable[0] ?? null;
}

function usableBuildingElements(elements: OverpassBuildingElement[]) {
  return elements
    .map((element) => ({ element, components: elementFootprintComponents(element) }))
    .filter((entry) => entry.components.some((component) => component.exterior.length >= 3));
}

function overpassElementToPartialGeometry(
  element: OverpassBuildingElement,
  components: FootprintComponent[],
  target: BuildingTarget,
  radiusM: number
): PartialGeometry {
  const tags = element.tags ?? {};
  const heightM = positiveNumber(tags.height);
  const floors = positiveNumber(tags["building:levels"] ?? tags.levels);
  const roofShape = text(tags["roof:shape"]);
  const roofMaterial = text(tags["roof:material"]);
  const roofOrientation = text(tags["roof:orientation"] ?? tags["roof:direction"]);
  const facadeMaterial = text(tags["building:material"] ?? tags.material);
  const facadeColor = text(tags["building:colour"] ?? tags["building:color"] ?? tags.colour);
  const rawId = rawElementId(element);
  const queryBounds = boundsAround(target.approximateCenter, radiusM);
  const observation = createOverpassObservation(element, components, radiusM);

  return {
    footprint: representativeExterior(components),
    heightM,
    floors,
    roof: { shape: roofShape, material: roofMaterial, orientation: roofOrientation },
    facade: { material: facadeMaterial, color: facadeColor },
    confidence: {
      footprint: "medium",
      height: heightM ? "medium" : "missing",
      floors: floors ? "medium" : "missing",
      roof: roofShape || roofMaterial || roofOrientation ? "medium" : "missing",
      facade: facadeMaterial || facadeColor ? "low" : "missing"
    },
    sourceRecords: [{
      source: "osm_overpass",
      featureId: rawId,
      releaseId: null,
      queryBounds,
      queryLimit: QUERY_LIMIT,
      components
    }],
    observations: [observation],
    notes: [
      `Live OSM/Overpass building ${rawId}.`,
      `OSM tags: ${Object.entries(tags).map(([key, value]) => `${key}=${value}`).join(", ") || "none"}.`,
      `Preserved ${components.length} polygon component(s).`
    ]
  };
}

function createOverpassObservation(
  element: OverpassBuildingElement,
  components: FootprintComponent[],
  radiusM: number
) {
  const rawId = rawElementId(element);
  return createBuildingGeometryObservation({
    id: rawId,
    source: "osm_overpass",
    sourceFeatureId: rawId,
    name: elementName(element),
    tags: element.tags ?? {},
    components,
    normalizationNotes: [
      element.type === "relation"
        ? "Assembled relation outer and inner way members into polygon components."
        : "Normalized closed OSM way geometry into a polygon component.",
      `Bounded Overpass query radius ${radiusM} m; result limit ${QUERY_LIMIT}.`
    ]
  });
}

function rawElementId(element: OverpassBuildingElement) {
  return `osm:${element.type}:${element.id}`;
}

export function elementFootprintComponents(element: OverpassBuildingElement): FootprintComponent[] {
  if (element.type === "way") {
    const exterior = geometryRing(element.geometry);
    return exterior.length >= 3 ? [{ exterior, interiorRings: [] }] : [];
  }

  const outerRings = (element.members ?? [])
    .filter((member) => member.type === "way" && (member.role ?? "outer") === "outer")
    .map((member) => geometryRing(member.geometry))
    .filter((ring) => ring.length >= 3);
  const innerRings = (element.members ?? [])
    .filter((member) => member.type === "way" && member.role === "inner")
    .map((member) => geometryRing(member.geometry))
    .filter((ring) => ring.length >= 3);
  if (!outerRings.length) {
    const fallback = geometryRing(element.geometry);
    return fallback.length >= 3 ? [{ exterior: fallback, interiorRings: [] }] : [];
  }
  const components = outerRings.map((exterior) => ({ exterior, interiorRings: [] as LngLatPoint[][] }));
  for (const innerRing of innerRings) {
    const owner = components.find((component) => pointInRing(innerRing[0], component.exterior));
    if (owner) owner.interiorRings.push(innerRing);
  }
  return components;
}

function geometryRing(geometry: OverpassGeometryPoint[] | undefined): LngLatPoint[] {
  const points = (geometry ?? [])
    .filter((point) => Number.isFinite(point.lon) && Number.isFinite(point.lat))
    .map((point) => ({ lng: point.lon, lat: point.lat }));
  if (points.length < 2) return points;
  const first = points[0];
  const last = points[points.length - 1];
  return first.lng === last.lng && first.lat === last.lat ? points.slice(0, -1) : points;
}

function representativeExterior(components: FootprintComponent[]) {
  return [...components].sort((left, right) => ringArea(right.exterior) - ringArea(left.exterior))[0]?.exterior ?? [];
}

function distanceToTarget(components: FootprintComponent[], target: BuildingTarget) {
  const points = components.flatMap((component) => component.exterior);
  const center = points.reduce(
    (sum, point) => ({ lng: sum.lng + point.lng, lat: sum.lat + point.lat }),
    { lng: 0, lat: 0 }
  );
  const lng = center.lng / points.length;
  const lat = center.lat / points.length;
  return (lng - target.approximateCenter.lng) ** 2 + (lat - target.approximateCenter.lat) ** 2;
}

function boundsAround(center: LngLatPoint, radiusM: number): GeographicBounds {
  const latDelta = radiusM / 111_320;
  const lngDelta = radiusM / (111_320 * Math.max(0.1, Math.cos(center.lat * Math.PI / 180)));
  return {
    minLng: center.lng - lngDelta,
    minLat: center.lat - latDelta,
    maxLng: center.lng + lngDelta,
    maxLat: center.lat + latDelta
  };
}

function pointInRing(point: LngLatPoint, ring: LngLatPoint[]) {
  let inside = false;
  for (let index = 0, previous = ring.length - 1; index < ring.length; previous = index, index += 1) {
    const current = ring[index];
    const prior = ring[previous];
    const crosses = current.lat > point.lat !== prior.lat > point.lat &&
      point.lng < (prior.lng - current.lng) * (point.lat - current.lat) / (prior.lat - current.lat) + current.lng;
    if (crosses) inside = !inside;
  }
  return inside;
}

function ringArea(points: LngLatPoint[]) {
  return Math.abs(points.reduce((sum, point, index) => {
    const next = points[(index + 1) % points.length];
    return sum + point.lng * next.lat - next.lng * point.lat;
  }, 0) / 2);
}

function elementName(element: OverpassBuildingElement) {
  return element.tags?.name ?? element.tags?.["name:zh"] ?? element.tags?.["name:en"] ?? "";
}

function clampRadius(value: number) {
  if (!Number.isFinite(value)) return DEFAULT_RADIUS_M;
  return Math.max(20, Math.min(MAX_RADIUS_M, Math.round(value)));
}

function normalizeName(value: string) {
  return value.trim().toLocaleLowerCase().replace(/\s+/g, "");
}

function positiveNumber(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) && value > 0 ? value : null;
  if (typeof value !== "string") return null;
  const number = Number.parseFloat(value);
  return Number.isFinite(number) && number > 0 ? number : null;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}
