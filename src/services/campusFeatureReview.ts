import polygonClipping from "polygon-clipping";
import type { MapFeature } from "../domain/foundationManifest";
import type { CandidatePoint, MapCandidate, OnlineMapQueryTarget } from "../domain/mapCandidate";
import { classifyCampusCandidates } from "./candidateConfidence";

export interface ScopedCandidate {
  candidate: MapCandidate;
  defaultAccepted: boolean;
  reason: string;
}

export function targetFromCampusBoundary(target: OnlineMapQueryTarget, boundary: CandidatePoint[]): OnlineMapQueryTarget {
  const bounds = geometryBounds(boundary);
  const center = { lng: (bounds.minLng + bounds.maxLng) / 2, lat: (bounds.minLat + bounds.maxLat) / 2 };
  const corners = [
    { lng: bounds.minLng, lat: bounds.minLat },
    { lng: bounds.maxLng, lat: bounds.maxLat }
  ];
  const radiusM = Math.max(250, ...corners.map((point) => distanceMeters(center, point)));
  return { ...target, center, radiusM, query: target.campus };
}

export function scopeCandidatesToBoundary(candidates: MapCandidate[], boundary: CandidatePoint[]): ScopedCandidate[] {
  const scoped = candidates.flatMap((candidate) => {
    if (candidate.kind === "campus") return [];
    if (candidate.geometry.type === "point") {
      return pointInPolygon(candidate.geometry.points[0], boundary)
        ? [{ candidate, defaultAccepted: false, reason: "Point evidence requires geometry review." }]
        : [];
    }
    if (candidate.kind === "building") return scopeBuilding(candidate, boundary);
    if (candidate.geometry.type === "polyline") {
      const points = clipPolyline(candidate.geometry.points, boundary);
      if (points.length < 2) return [];
      return [{ candidate: withGeometry(candidate, "polyline", points, "Clipped to confirmed Campus Boundary."), defaultAccepted: true, reason: "Valid clipped line geometry." }];
    }
    const points = intersectPolygons(candidate.geometry.points, boundary);
    if (points.length < 3) return [];
    return [{ candidate: withGeometry(candidate, "polygon", points, "Clipped to confirmed Campus Boundary."), defaultAccepted: true, reason: "Valid clipped area geometry." }];
  });
  const classified = classifyCampusCandidates(scoped.map((item) => item.candidate));
  return scoped.map((item, index) => ({
    ...item,
    candidate: classified[index],
    defaultAccepted: item.defaultAccepted && classified[index].confidence !== "low",
    reason: classified[index].confidence === "low" ? classified[index].confidenceReasons?.join(" ") ?? item.reason : item.reason
  }));
}

export function featureCounts(candidates: MapCandidate[], manualFeatures: MapFeature[]) {
  const counts: Record<"building" | "road" | "water" | "vegetation" | "sports", number> = {
    building: 0, road: 0, water: 0, vegetation: 0, sports: 0
  };
  for (const item of [...candidates, ...manualFeatures]) {
    if (item.kind in counts) counts[item.kind as keyof typeof counts] += 1;
  }
  return counts;
}

function scopeBuilding(candidate: MapCandidate, boundary: CandidatePoint[]): ScopedCandidate[] {
  if (candidate.geometry.type !== "polygon") return [];
  const center = polygonCentroid(candidate.geometry.points);
  const intersection = intersectPolygons(candidate.geometry.points, boundary);
  if (!intersection.length) return [];
  const overlap = polygonArea(intersection) / Math.max(polygonArea(candidate.geometry.points), Number.EPSILON);
  const clearlyInside = pointInPolygon(center, boundary) && overlap >= 0.7;
  return [{
    candidate: clearlyInside ? candidate : { ...candidate, confidence: "low", provenance: { ...candidate.provenance, notes: [...candidate.provenance.notes, `Boundary-straddling building; overlap=${Math.round(overlap * 100)}%.`] } },
    defaultAccepted: clearlyInside,
    reason: clearlyInside ? "Building main body lies inside boundary." : "Boundary-straddling building requires review."
  }];
}

function withGeometry(candidate: MapCandidate, type: "polygon" | "polyline", points: CandidatePoint[], note: string): MapCandidate {
  return { ...candidate, geometry: { type, points }, provenance: { ...candidate.provenance, notes: [...candidate.provenance.notes, note] } };
}

function intersectPolygons(subject: CandidatePoint[], clip: CandidatePoint[]): CandidatePoint[] {
  const subjectRing = closeRing(subject).map((point) => [point.lng, point.lat] as [number, number]);
  const clipRing = closeRing(clip).map((point) => [point.lng, point.lat] as [number, number]);
  const result = polygonClipping.intersection([subjectRing], [clipRing]);
  const rings = result.flatMap((polygon) => polygon.slice(0, 1));
  const largest = rings.sort((left, right) => polygonArea(right.map(([lng, lat]) => ({ lng, lat }))) - polygonArea(left.map(([lng, lat]) => ({ lng, lat }))))[0];
  return largest?.map(([lng, lat]) => ({ lng, lat })) ?? [];
}

function clipPolyline(line: CandidatePoint[], polygon: CandidatePoint[]) {
  const pieces: CandidatePoint[][] = [];
  for (let index = 0; index < line.length - 1; index += 1) {
    const start = line[index], end = line[index + 1];
    const cuts = [0, 1, ...segmentPolygonIntersections(start, end, polygon)].sort((a, b) => a - b);
    for (let cut = 0; cut < cuts.length - 1; cut += 1) {
      const fromT = cuts[cut], toT = cuts[cut + 1];
      const midpoint = interpolate(start, end, (fromT + toT) / 2);
      if (!pointInPolygon(midpoint, polygon)) continue;
      const from = interpolate(start, end, fromT), to = interpolate(start, end, toT);
      const lastPiece = pieces[pieces.length - 1];
      if (lastPiece && samePoint(lastPiece[lastPiece.length - 1], from)) lastPiece.push(to);
      else pieces.push([from, to]);
    }
  }
  return pieces.sort((left, right) => polylineLength(right) - polylineLength(left))[0] ?? [];
}

function segmentPolygonIntersections(start: CandidatePoint, end: CandidatePoint, polygon: CandidatePoint[]) {
  const values: number[] = [];
  const ring = closeRing(polygon);
  for (let index = 0; index < ring.length - 1; index += 1) {
    const value = segmentIntersectionT(start, end, ring[index], ring[index + 1]);
    if (value !== null && value > 1e-9 && value < 1 - 1e-9 && !values.some((existing) => Math.abs(existing - value) < 1e-8)) values.push(value);
  }
  return values;
}

function segmentIntersectionT(a: CandidatePoint, b: CandidatePoint, c: CandidatePoint, d: CandidatePoint) {
  const r = { x: b.lng - a.lng, y: b.lat - a.lat }, s = { x: d.lng - c.lng, y: d.lat - c.lat };
  const denominator = r.x * s.y - r.y * s.x;
  if (Math.abs(denominator) < 1e-12) return null;
  const q = { x: c.lng - a.lng, y: c.lat - a.lat };
  const t = (q.x * s.y - q.y * s.x) / denominator;
  const u = (q.x * r.y - q.y * r.x) / denominator;
  return t >= 0 && t <= 1 && u >= 0 && u <= 1 ? t : null;
}

function pointInPolygon(point: CandidatePoint, polygon: CandidatePoint[]) {
  let inside = false;
  for (let index = 0, previous = polygon.length - 1; index < polygon.length; previous = index++) {
    const left = polygon[index], right = polygon[previous];
    if (((left.lat > point.lat) !== (right.lat > point.lat)) && point.lng < (right.lng - left.lng) * (point.lat - left.lat) / ((right.lat - left.lat) || Number.EPSILON) + left.lng) inside = !inside;
  }
  return inside;
}

function polygonCentroid(points: CandidatePoint[]) {
  return points.reduce((sum, point) => ({ lng: sum.lng + point.lng / points.length, lat: sum.lat + point.lat / points.length }), { lng: 0, lat: 0 });
}
function polygonArea(points: CandidatePoint[]) { return Math.abs(closeRing(points).slice(0, -1).reduce((sum, point, index) => { const next = closeRing(points)[index + 1]; return sum + point.lng * next.lat - next.lng * point.lat; }, 0) / 2); }
function geometryBounds(points: CandidatePoint[]) { return { minLng: Math.min(...points.map((point) => point.lng)), minLat: Math.min(...points.map((point) => point.lat)), maxLng: Math.max(...points.map((point) => point.lng)), maxLat: Math.max(...points.map((point) => point.lat)) }; }
function closeRing(points: CandidatePoint[]) { return points.length && !samePoint(points[0], points[points.length - 1]) ? [...points, points[0]] : points; }
function samePoint(left: CandidatePoint, right: CandidatePoint) { return Math.abs(left.lng - right.lng) < 1e-8 && Math.abs(left.lat - right.lat) < 1e-8; }
function interpolate(start: CandidatePoint, end: CandidatePoint, t: number) { return { lng: start.lng + (end.lng - start.lng) * t, lat: start.lat + (end.lat - start.lat) * t }; }
function polylineLength(points: CandidatePoint[]) { return points.slice(1).reduce((sum, point, index) => sum + distanceMeters(points[index], point), 0); }
function distanceMeters(left: CandidatePoint, right: CandidatePoint) { const lat = ((left.lat + right.lat) / 2) * Math.PI / 180; return Math.hypot((left.lng - right.lng) * 111_320 * Math.cos(lat), (left.lat - right.lat) * 111_320); }
