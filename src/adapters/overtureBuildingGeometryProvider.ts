import { invokeDesktop } from "../services/tauriInvoke";
import type {
  BuildingSourceRecord,
  BuildingTarget,
  FootprintComponent,
  GeographicBounds,
  LngLatPoint
} from "../domain/buildingGeometry";
import type { BuildingSlot } from "../domain/foundationManifest";
import { createBuildingSlotHandoffProvider } from "./buildingSlotHandoffProvider";
import { createBuildingGeometryObservation } from "../services/buildingObservation";
import { scoreObservationIdentity } from "../services/buildingIdentity";
import type { BuildingGeometryProvider, PartialGeometry } from "./minimalArnisAdapter";
import { OverpassBuildingGeometryProvider } from "./overpassBuildingGeometryProvider";
import { putuoLibraryFixtureProvider } from "./putuoLibraryFixtureProvider";

type OvertureProperties = Record<string, unknown>;

interface OvertureFeature {
  id?: string;
  geometry?: {
    type?: "Polygon" | "MultiPolygon";
    coordinates?: unknown;
  };
  properties?: OvertureProperties;
}

interface OvertureFeatureCollection {
  type?: "FeatureCollection";
  features?: OvertureFeature[];
  metadata?: {
    releaseId?: string;
    queryBounds?: GeographicBounds;
    queryLimit?: number;
  };
}

export interface OvertureQueryRequest {
  lng: number;
  lat: number;
  radiusM: number;
  name: string;
  releaseId: string | null;
  limit: number;
  bounds: GeographicBounds;
}

const viteEnv = (import.meta as ImportMeta & {
  env?: Record<string, string | undefined>;
}).env ?? {};

const DEFAULT_RADIUS_M = 120;
const MIN_RADIUS_M = 20;
const MAX_RADIUS_M = 250;
const DEFAULT_LIMIT = 20;
const MAX_LIMIT = 50;
const DEFAULT_TIMEOUT_MS = 15_000;

export class OvertureBuildingGeometryProvider implements BuildingGeometryProvider {
  readonly source = "overture" as const;

  constructor(
    private readonly options: {
      endpoint?: string;
      fetchJson?: typeof fetch;
      queryBackend?: (request: OvertureQueryRequest) => Promise<OvertureFeatureCollection>;
      radiusM?: number;
      releaseId?: string;
      limit?: number;
      timeoutMs?: number;
    } = {}
  ) {}

  async fetchBuildingGeometry(target: BuildingTarget): Promise<PartialGeometry | null> {
    const request = createBoundedOvertureRequest(target, this.options);
    const payload = await this.query(request);
    const candidates = usableFeatureEntries(payload.features ?? []);
    const releaseId = payload.metadata?.releaseId ?? request.releaseId;
    const candidateObservations = candidates.map(({ feature, components }) =>
      createOvertureObservation(feature, components, releaseId)
    );
    const selected = selectNearestUsableFeature(candidates, candidateObservations, target);
    if (!selected) return null;

    const geometry = overtureFeatureToPartialGeometry(selected.feature, selected.components, {
      releaseId,
      queryBounds: payload.metadata?.queryBounds ?? request.bounds,
      queryLimit: payload.metadata?.queryLimit ?? request.limit
    });
    geometry.observations = candidateObservations;
    return geometry;
  }

  private async query(request: OvertureQueryRequest): Promise<OvertureFeatureCollection> {
    if (this.options.queryBackend) return this.options.queryBackend(request);
    if (!this.options.endpoint) {
      return invokeDesktop<OvertureFeatureCollection>("query_overture_buildings", { request });
    }

    const url = new URL(this.options.endpoint);
    url.searchParams.set("lng", String(request.lng));
    url.searchParams.set("lat", String(request.lat));
    url.searchParams.set("radius_m", String(request.radiusM));
    url.searchParams.set("name", request.name);
    url.searchParams.set("theme", "buildings");
    url.searchParams.set("type", "building");
    url.searchParams.set("bbox", boundsToString(request.bounds));
    url.searchParams.set("limit", String(request.limit));
    if (request.releaseId) url.searchParams.set("release", request.releaseId);

    const controller = new AbortController();
    const timeout = globalThis.setTimeout(
      () => controller.abort(),
      this.options.timeoutMs ?? DEFAULT_TIMEOUT_MS
    );
    try {
      const response = await (this.options.fetchJson ?? fetch)(url, {
        signal: controller.signal,
        headers: { accept: "application/geo+json, application/json" }
      });
      if (!response.ok) {
        throw new Error(`Overture building request failed: ${response.status}`);
      }
      return (await response.json()) as OvertureFeatureCollection;
    } finally {
      globalThis.clearTimeout(timeout);
    }
  }
}

export function createDefaultBuildingGeometryProviders(
  slot: BuildingSlot
): BuildingGeometryProvider[] {
  const useFixture = viteEnv.VITE_BUILDING_GEOMETRY_OFFLINE_FIXTURE === "true";
  const overtureProvider = useFixture
    ? putuoLibraryFixtureProvider
    : withBuildingGeometryFailureAsEmpty(new OvertureBuildingGeometryProvider({
        releaseId: viteEnv.VITE_OVERTURE_RELEASE_ID
      }));

  return [
    overtureProvider,
    withBuildingGeometryFailureAsEmpty(new OverpassBuildingGeometryProvider({
      endpoint: viteEnv.VITE_OVERPASS_ENDPOINT
    })),
    createBuildingSlotHandoffProvider(slot)
  ];
}

export function createBoundedOvertureRequest(
  target: BuildingTarget,
  options: { radiusM?: number; releaseId?: string; limit?: number } = {}
): OvertureQueryRequest {
  const radiusM = clampInteger(options.radiusM ?? DEFAULT_RADIUS_M, MIN_RADIUS_M, MAX_RADIUS_M);
  const limit = clampInteger(options.limit ?? DEFAULT_LIMIT, 1, MAX_LIMIT);
  return {
    lng: target.approximateCenter.lng,
    lat: target.approximateCenter.lat,
    radiusM,
    name: target.name,
    releaseId: options.releaseId?.trim() || null,
    limit,
    bounds: boundsAround(target.approximateCenter, radiusM)
  };
}

export function withBuildingGeometryFailureAsEmpty(
  provider: BuildingGeometryProvider
): BuildingGeometryProvider {
  return {
    source: provider.source,
    async fetchBuildingGeometry(target) {
      try {
        return await provider.fetchBuildingGeometry(target);
      } catch (error) {
        console.warn(`Live ${provider.source} building provider failed; continuing to fallback.`, error);
        return null;
      }
    }
  };
}

export function withBuildingGeometryFallback(
  liveProvider: BuildingGeometryProvider,
  fallbackProvider: BuildingGeometryProvider
): BuildingGeometryProvider {
  if (liveProvider.source !== fallbackProvider.source) {
    throw new Error("Building Geometry fallback providers must use the same source.");
  }

  return {
    source: liveProvider.source,
    async fetchBuildingGeometry(target) {
      try {
        const liveGeometry = await liveProvider.fetchBuildingGeometry(target);
        if (liveGeometry) return liveGeometry;
      } catch (error) {
        console.warn(`Live ${liveProvider.source} building provider failed; using fixture fallback.`, error);
      }
      return fallbackProvider.fetchBuildingGeometry(target);
    }
  };
}

function selectNearestUsableFeature(
  usable: Array<{ feature: OvertureFeature; components: FootprintComponent[] }>,
  observations: ReturnType<typeof createOvertureObservation>[],
  target: BuildingTarget
): { feature: OvertureFeature; components: FootprintComponent[] } | null {
  usable.sort((left, right) => {
    const leftObservation = observations.find((item) => item.sourceFeatureId === rawFeatureId(left.feature));
    const rightObservation = observations.find((item) => item.sourceFeatureId === rawFeatureId(right.feature));
    const scoreDifference = (rightObservation ? scoreObservationIdentity(target, rightObservation).score : 0) -
      (leftObservation ? scoreObservationIdentity(target, leftObservation).score : 0);
    return scoreDifference ||
      squaredDistance(centroid(representativeExterior(left.components)), target.approximateCenter) -
      squaredDistance(centroid(representativeExterior(right.components)), target.approximateCenter);
  });
  return usable[0] ?? null;
}

function usableFeatureEntries(features: OvertureFeature[]) {
  return features
    .map((feature) => ({ feature, components: extractFootprintComponents(feature.geometry) }))
    .filter((entry) => entry.components.some((component) => component.exterior.length >= 3));
}

function overtureFeatureToPartialGeometry(
  feature: OvertureFeature,
  components: FootprintComponent[],
  query: { releaseId: string | null; queryBounds: GeographicBounds; queryLimit: number }
): PartialGeometry {
  const properties = feature.properties ?? {};
  const footprint = representativeExterior(components);
  const heightM = positiveNumber(properties.height ?? properties.height_m);
  const floors = positiveNumber(properties.num_floors ?? properties.levels);
  const roofShape = text(properties.roof_shape ?? properties["roof:shape"]);
  const roofMaterial = text(properties.roof_material ?? properties["roof:material"]);
  const roofOrientation = text(
    properties.roof_orientation ?? properties.roof_direction ?? properties["roof:orientation"]
  );
  const facadeMaterial = text(
    properties.facade_material ?? properties.material ?? properties["building:material"]
  );
  const facadeColor = text(
    properties.facade_color ?? properties.color ?? properties["building:colour"]
  );
  const featureId = rawFeatureId(feature);
  const sourceRecord: BuildingSourceRecord = {
    source: "overture",
    featureId,
    releaseId: query.releaseId,
    queryBounds: query.queryBounds,
    queryLimit: query.queryLimit,
    components
  };
  const observation = createOvertureObservation(feature, components, query.releaseId);

  return {
    footprint,
    heightM,
    floors,
    roof: { shape: roofShape, material: roofMaterial, orientation: roofOrientation },
    facade: { material: facadeMaterial, color: facadeColor },
    confidence: {
      footprint: "high",
      height: heightM ? "high" : "missing",
      floors: floors ? "high" : "missing",
      roof: roofShape || roofMaterial || roofOrientation ? "medium" : "missing",
      facade: facadeMaterial || facadeColor ? "medium" : "missing"
    },
    sourceRecords: [sourceRecord],
    observations: [observation],
    notes: [
      `Live Overture building feature ${featureId}.`,
      `Overture release ${query.releaseId ?? "provider-default"}; bounded query ${boundsToString(query.queryBounds)}; limit ${query.queryLimit}.`,
      `Preserved ${components.length} footprint component(s) and ${components.reduce((sum, component) => sum + component.interiorRings.length, 0)} interior ring(s).`
    ]
  };
}

function createOvertureObservation(
  feature: OvertureFeature,
  components: FootprintComponent[],
  releaseId: string | null
) {
  const properties = feature.properties ?? {};
  const featureId = rawFeatureId(feature);
  return createBuildingGeometryObservation({
    id: `overture:${featureId}`,
    source: "overture",
    sourceFeatureId: featureId,
    name: text(properties.name) ?? text(properties.names),
    tags: stringProperties(properties),
    components,
    normalizationNotes: [
      `Release ${releaseId ?? "provider-default"}.`,
      "Polygon parts and interior rings preserved from GeoJSON."
    ]
  });
}

function rawFeatureId(feature: OvertureFeature) {
  return feature.id ?? text(feature.properties?.id) ?? "without-id";
}

export function extractFootprintComponents(
  geometry: OvertureFeature["geometry"]
): FootprintComponent[] {
  if (!geometry?.coordinates) return [];
  const polygons = geometry.type === "Polygon"
    ? [geometry.coordinates]
    : geometry.type === "MultiPolygon"
      ? geometry.coordinates
      : [];
  if (!Array.isArray(polygons)) return [];

  return polygons.flatMap((polygon): FootprintComponent[] => {
    if (!Array.isArray(polygon)) return [];
    const rings = polygon
      .map((ring) => Array.isArray(ring) ? coordinateRing(ring) : [])
      .filter((ring) => ring.length >= 3);
    if (!rings.length) return [];
    return [{ exterior: rings[0], interiorRings: rings.slice(1) }];
  });
}

function representativeExterior(components: FootprintComponent[]): LngLatPoint[] {
  return [...components]
    .sort((left, right) => polygonArea(right.exterior) - polygonArea(left.exterior))[0]?.exterior ?? [];
}

function coordinateRing(ring: unknown[]): LngLatPoint[] {
  return removeClosingPoint(ring
    .map((coordinate) => {
      if (!Array.isArray(coordinate)) return null;
      const lng = Number(coordinate[0]);
      const lat = Number(coordinate[1]);
      return Number.isFinite(lng) && Number.isFinite(lat) ? { lng, lat } : null;
    })
    .filter((point): point is LngLatPoint => Boolean(point)));
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

function boundsToString(bounds: GeographicBounds) {
  return [bounds.minLng, bounds.minLat, bounds.maxLng, bounds.maxLat].join(",");
}

function clampInteger(value: number, minimum: number, maximum: number) {
  if (!Number.isFinite(value)) return minimum;
  return Math.max(minimum, Math.min(maximum, Math.round(value)));
}

function removeClosingPoint(points: LngLatPoint[]) {
  if (points.length < 2) return points;
  const first = points[0];
  const last = points[points.length - 1];
  return first.lng === last.lng && first.lat === last.lat ? points.slice(0, -1) : points;
}

function polygonArea(points: LngLatPoint[]) {
  return Math.abs(points.reduce((sum, point, index) => {
    const next = points[(index + 1) % points.length];
    return sum + point.lng * next.lat - next.lng * point.lat;
  }, 0) / 2);
}

function centroid(points: LngLatPoint[]): LngLatPoint {
  const sum = points.reduce(
    (total, point) => ({ lng: total.lng + point.lng, lat: total.lat + point.lat }),
    { lng: 0, lat: 0 }
  );
  return { lng: sum.lng / points.length, lat: sum.lat / points.length };
}

function squaredDistance(left: LngLatPoint, right: LngLatPoint) {
  return (left.lng - right.lng) ** 2 + (left.lat - right.lat) ** 2;
}

function positiveNumber(value: unknown): number | null {
  const number = typeof value === "string" ? Number(value) : value;
  return typeof number === "number" && Number.isFinite(number) && number > 0 ? number : null;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function stringProperties(properties: OvertureProperties) {
  return Object.fromEntries(
    Object.entries(properties)
      .filter((entry): entry is [string, string | number | boolean] =>
        ["string", "number", "boolean"].includes(typeof entry[1])
      )
      .map(([key, value]) => [key, String(value)])
  );
}
