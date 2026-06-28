import type {
  BuildingGeometryObservation,
  BuildingGeometrySource,
  FootprintComponent,
  LngLatPoint
} from "../domain/buildingGeometry";

interface LocalPoint {
  x: number;
  y: number;
}

export interface ObservationOverlap {
  leftObservationId: string;
  rightObservationId: string;
  score: number;
}

export function createBuildingGeometryObservation(input: {
  id: string;
  source: BuildingGeometrySource;
  sourceFeatureId: string;
  name?: string | null;
  tags?: Record<string, string>;
  components: FootprintComponent[];
  normalizationNotes?: string[];
}): BuildingGeometryObservation {
  const components = input.components
    .filter((component) => component.exterior.length >= 3)
    .map((component) => structuredClone(component));
  return {
    id: input.id,
    source: input.source,
    sourceFeatureId: input.sourceFeatureId,
    name: input.name?.trim() || null,
    tags: { ...(input.tags ?? {}) },
    components,
    metrics: measureFootprintComponents(components),
    normalizationNotes: [...(input.normalizationNotes ?? [])]
  };
}

export function pairwiseObservationOverlaps(
  observations: BuildingGeometryObservation[]
): ObservationOverlap[] {
  const overlaps: ObservationOverlap[] = [];
  for (let leftIndex = 0; leftIndex < observations.length; leftIndex += 1) {
    for (let rightIndex = leftIndex + 1; rightIndex < observations.length; rightIndex += 1) {
      overlaps.push({
        leftObservationId: observations[leftIndex].id,
        rightObservationId: observations[rightIndex].id,
        score: approximateFootprintIoU(
          observations[leftIndex].components,
          observations[rightIndex].components
        )
      });
    }
  }
  return overlaps;
}

export function approximateFootprintIoU(
  left: FootprintComponent[],
  right: FootprintComponent[],
  resolution = 80
): number {
  const points = [...componentPoints(left), ...componentPoints(right)];
  if (!points.length) return 0;
  const bounds = coordinateBounds(points);
  if (bounds.maxLng === bounds.minLng || bounds.maxLat === bounds.minLat) return 0;
  let intersection = 0;
  let union = 0;
  for (let y = 0; y < resolution; y += 1) {
    for (let x = 0; x < resolution; x += 1) {
      const point = {
        lng: bounds.minLng + ((x + 0.5) / resolution) * (bounds.maxLng - bounds.minLng),
        lat: bounds.minLat + ((y + 0.5) / resolution) * (bounds.maxLat - bounds.minLat)
      };
      const inLeft = pointInComponents(point, left);
      const inRight = pointInComponents(point, right);
      if (inLeft || inRight) union += 1;
      if (inLeft && inRight) intersection += 1;
    }
  }
  return union ? intersection / union : 0;
}

export function measureFootprintComponents(components: FootprintComponent[]) {
  const geographicPoints = componentPoints(components);
  if (!geographicPoints.length) {
    return {
      areaSquareMeters: 0,
      widthMeters: 0,
      lengthMeters: 0,
      orientationDegrees: 0,
      pointCount: 0,
      center: { lng: 0, lat: 0 }
    };
  }
  const center = geographicPoints.reduce(
    (sum, point) => ({ lng: sum.lng + point.lng, lat: sum.lat + point.lat }),
    { lng: 0, lat: 0 }
  );
  center.lng /= geographicPoints.length;
  center.lat /= geographicPoints.length;
  const project = localProjector(center);
  const localComponents = components.map((component) => ({
    exterior: component.exterior.map(project),
    interiorRings: component.interiorRings.map((ring) => ring.map(project))
  }));
  const longest = longestExteriorEdge(localComponents);
  const orientationRadians = Math.atan2(longest.y, longest.x);
  const rotated = localComponents.flatMap((component) =>
    component.exterior.map((point) => rotate(point, -orientationRadians))
  );
  const bounds = localBounds(rotated);
  const firstSpan = bounds.maxX - bounds.minX;
  const secondSpan = bounds.maxY - bounds.minY;
  const area = localComponents.reduce((sum, component) =>
    sum + polygonArea(component.exterior) - component.interiorRings.reduce(
      (holes, ring) => holes + polygonArea(ring),
      0
    ), 0);

  return {
    areaSquareMeters: Math.max(0, area),
    widthMeters: Math.min(firstSpan, secondSpan),
    lengthMeters: Math.max(firstSpan, secondSpan),
    orientationDegrees: normalizeOrientation(orientationRadians * 180 / Math.PI),
    pointCount: components.reduce((sum, component) =>
      sum + component.exterior.length + component.interiorRings.reduce((ringSum, ring) => ringSum + ring.length, 0),
      0
    ),
    center
  };
}

function localProjector(origin: LngLatPoint) {
  const metersPerDegreeLat = 111_320;
  const metersPerDegreeLng = metersPerDegreeLat * Math.cos(origin.lat * Math.PI / 180);
  return (point: LngLatPoint): LocalPoint => ({
    x: (point.lng - origin.lng) * metersPerDegreeLng,
    y: (point.lat - origin.lat) * metersPerDegreeLat
  });
}

function longestExteriorEdge(
  components: Array<{ exterior: LocalPoint[]; interiorRings: LocalPoint[][] }>
): LocalPoint {
  let longest = { x: 1, y: 0 };
  let lengthSquared = 0;
  for (const component of components) {
    for (let index = 0; index < component.exterior.length; index += 1) {
      const point = component.exterior[index];
      const next = component.exterior[(index + 1) % component.exterior.length];
      const edge = { x: next.x - point.x, y: next.y - point.y };
      const candidateLength = edge.x ** 2 + edge.y ** 2;
      if (candidateLength > lengthSquared) {
        longest = edge;
        lengthSquared = candidateLength;
      }
    }
  }
  return longest;
}

function localBounds(points: LocalPoint[]) {
  return points.reduce((bounds, point) => ({
    minX: Math.min(bounds.minX, point.x),
    maxX: Math.max(bounds.maxX, point.x),
    minY: Math.min(bounds.minY, point.y),
    maxY: Math.max(bounds.maxY, point.y)
  }), {
    minX: Number.POSITIVE_INFINITY,
    maxX: Number.NEGATIVE_INFINITY,
    minY: Number.POSITIVE_INFINITY,
    maxY: Number.NEGATIVE_INFINITY
  });
}

function rotate(point: LocalPoint, angle: number): LocalPoint {
  return {
    x: point.x * Math.cos(angle) - point.y * Math.sin(angle),
    y: point.x * Math.sin(angle) + point.y * Math.cos(angle)
  };
}

function polygonArea(points: LocalPoint[]) {
  return Math.abs(points.reduce((sum, point, index) => {
    const next = points[(index + 1) % points.length];
    return sum + point.x * next.y - next.x * point.y;
  }, 0) / 2);
}

function normalizeOrientation(value: number) {
  const normalized = ((value % 180) + 180) % 180;
  return normalized > 90 ? normalized - 180 : normalized;
}

function componentPoints(components: FootprintComponent[]) {
  return components.flatMap((component) => [component.exterior, ...component.interiorRings].flat());
}

function coordinateBounds(points: LngLatPoint[]) {
  return points.reduce((bounds, point) => ({
    minLng: Math.min(bounds.minLng, point.lng),
    minLat: Math.min(bounds.minLat, point.lat),
    maxLng: Math.max(bounds.maxLng, point.lng),
    maxLat: Math.max(bounds.maxLat, point.lat)
  }), {
    minLng: Number.POSITIVE_INFINITY,
    minLat: Number.POSITIVE_INFINITY,
    maxLng: Number.NEGATIVE_INFINITY,
    maxLat: Number.NEGATIVE_INFINITY
  });
}

function pointInComponents(point: LngLatPoint, components: FootprintComponent[]) {
  return components.some((component) =>
    pointInRing(point, component.exterior) &&
    !component.interiorRings.some((ring) => pointInRing(point, ring))
  );
}

function pointInRing(point: LngLatPoint, ring: LngLatPoint[]) {
  let inside = false;
  for (let index = 0, previous = ring.length - 1; index < ring.length; previous = index, index += 1) {
    const currentPoint = ring[index];
    const previousPoint = ring[previous];
    const crosses = currentPoint.lat > point.lat !== previousPoint.lat > point.lat &&
      point.lng < (previousPoint.lng - currentPoint.lng) *
        (point.lat - currentPoint.lat) /
        (previousPoint.lat - currentPoint.lat) + currentPoint.lng;
    if (crosses) inside = !inside;
  }
  return inside;
}
