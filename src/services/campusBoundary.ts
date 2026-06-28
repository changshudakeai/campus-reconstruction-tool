import type { CampusTarget } from "../domain/campusTarget";
import type { MapCandidate } from "../domain/mapCandidate";
import { polygonCandidate } from "./mapCandidateFactory";

const ENDPOINTS = [
  "/overpass-api/api/interpreter",
  "/overpass-kumi/api/interpreter",
  "/overpass-nchc/api/interpreter"
];

export interface BoundaryElement {
  type: "way" | "relation";
  id: number;
  tags?: Record<string, string>;
  geometry?: Array<{ lat: number; lon: number }>;
  members?: Array<{ role?: string; geometry?: Array<{ lat: number; lon: number }> }>;
}

type Point = { lng: number; lat: number };

export async function queryCampusBoundaryCandidates(campus: CampusTarget): Promise<MapCandidate[]> {
  const errors: string[] = [];
  for (const endpoint of ENDPOINTS) {
    try {
      const response = await fetch(endpoint, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8" },
        body: new URLSearchParams({ data: boundaryQuery(campus) }),
        signal: AbortSignal.timeout(35_000)
      });
      if (!response.ok) {
        errors.push(`${new URL(endpoint).host}: HTTP ${response.status}`);
        continue;
      }
      const payload = await response.json() as { elements?: BoundaryElement[] };
      return rankBoundaryCandidates((payload.elements ?? []).flatMap((element) => elementToCandidates(element, campus)), campus);
    } catch (reason) {
      errors.push(`${new URL(endpoint).host}: ${reason instanceof Error ? reason.message : String(reason)}`);
    }
  }
  throw new Error(`Campus boundary lookup failed (${errors.join(" | ")})`);
}

function boundaryQuery(campus: CampusTarget) {
  const radius = Math.max(600, Math.round(campus.radiusM * 1.4));
  return `[out:json][timeout:30];(way(around:${radius},${campus.openCenter.lat},${campus.openCenter.lng})["amenity"~"university|college|school"];relation(around:${radius},${campus.openCenter.lat},${campus.openCenter.lng})["amenity"~"university|college|school"];);out body geom;`;
}

function elementToCandidates(element: BoundaryElement, campus: CampusTarget): MapCandidate[] {
  const segments = element.type === "relation"
    ? (element.members ?? [])
      .filter((member) => !member.role || member.role === "outer")
      .map((member) => toPoints(member.geometry ?? []))
      .filter((segment) => segment.length >= 2)
    : [toPoints(element.geometry ?? [])];
  const rings = element.type === "relation" ? assembleBoundaryRings(segments) : segments.filter(isClosedRing);
  return rings.filter((ring) => boundaryRingIsValid(ring)).map((ring, index) => {
    const confidence = boundaryConfidence(ring, element.tags?.name ?? "", campus);
    return polygonCandidate({
      id: `campus-boundary-osm-${element.type}-${element.id}-${index}`,
      name: element.tags?.name ?? `${campus.canonicalName} boundary candidate`,
      kind: "campus",
      source: "osm_overpass",
      confidence,
      query: campus.canonicalName,
      rawId: `osm:${element.type}:${element.id}${index ? `:outer:${index}` : ""}`,
      notes: [
        `OSM assembled campus boundary; ${element.tags?.name ?? "unnamed"}`,
        `confidence signals: name=${nameMatches(element.tags?.name ?? "", campus)}, anchor=${pointInRing(campus.openCenter, ring)}, area=${Math.round(ringAreaMeters(ring))}m2`
      ],
      points: ring.map((point) => [point.lng, point.lat])
    });
  });
}

function toPoints(points: Array<{ lat: number; lon: number }>): Point[] {
  return points.map((point) => ({ lng: point.lon, lat: point.lat }));
}

export function assembleBoundaryRings(inputSegments: Point[][]): Point[][] {
  const remaining = inputSegments.map((segment) => dedupeConsecutive(segment)).filter((segment) => segment.length >= 2);
  const rings: Point[][] = [];
  while (remaining.length) {
    let chain = remaining.shift()!;
    let changed = true;
    while (changed && !isClosedRing(chain)) {
      changed = false;
      for (let index = 0; index < remaining.length; index += 1) {
        const segment = remaining[index];
        const first = chain[0];
        const last = chain[chain.length - 1];
        if (samePoint(last, segment[0])) chain = [...chain, ...segment.slice(1)];
        else if (samePoint(last, segment[segment.length - 1])) chain = [...chain, ...segment.slice(0, -1).reverse()];
        else if (samePoint(first, segment[segment.length - 1])) chain = [...segment.slice(0, -1), ...chain];
        else if (samePoint(first, segment[0])) chain = [...segment.slice(1).reverse(), ...chain];
        else continue;
        remaining.splice(index, 1);
        changed = true;
        break;
      }
    }
    if (isClosedRing(chain)) rings.push(chain);
  }
  return rings;
}

export function boundaryRingIsValid(ring: Point[]) {
  return ring.length >= 4 && isClosedRing(ring) && ringAreaMeters(ring) >= 2_000 && !hasSelfIntersection(ring);
}

function boundaryConfidence(ring: Point[], name: string, campus: CampusTarget): MapCandidate["confidence"] {
  const area = ringAreaMeters(ring);
  const anchorNear = pointInRing(campus.openCenter, ring) || distanceToRingMeters(campus.openCenter, ring) <= 120;
  const score = (anchorNear ? 3 : 0) + (nameMatches(name, campus) ? 2 : 0) + (area >= 10_000 && area <= 5_000_000 ? 1 : 0);
  return score >= 4 ? "high" : score >= 2 ? "medium" : "low";
}

function rankBoundaryCandidates(candidates: MapCandidate[], campus: CampusTarget) {
  return candidates.sort((left, right) => score(right, campus) - score(left, campus));
}

function score(candidate: MapCandidate, campus: CampusTarget) {
  const ring = candidate.geometry.points;
  const anchorNear = pointInRing(campus.openCenter, ring) || distanceToRingMeters(campus.openCenter, ring) <= 120;
  const center = ring.reduce((sum, point) => ({ lng: sum.lng + point.lng / ring.length, lat: sum.lat + point.lat / ring.length }), { lng: 0, lat: 0 });
  const distance = distanceMeters(center, campus.openCenter);
  return (anchorNear ? 10_000 : 0) + (nameMatches(candidate.name, campus) ? 5_000 : 0) + Math.min(2_000, ring.length * 10) - distance;
}

function nameMatches(name: string, campus: CampusTarget) {
  const value = normalize(name);
  if (!value) return false;
  return [campus.canonicalName, ...campus.aliases, campus.schoolName]
    .map(normalize)
    .filter((term) => term.length >= 4)
    .some((term) => value.includes(term) || term.includes(value));
}

function normalize(value: string) { return value.replace(/[\s()（）·_-]/g, "").toLowerCase(); }
function samePoint(left: Point, right: Point) { return Math.abs(left.lng - right.lng) < 1e-7 && Math.abs(left.lat - right.lat) < 1e-7; }
function isClosedRing(points: Point[]) { return points.length >= 4 && samePoint(points[0], points[points.length - 1]); }
function dedupeConsecutive(points: Point[]) { return points.filter((point, index) => index === 0 || !samePoint(point, points[index - 1])); }

function ringAreaMeters(points: Point[]) {
  const centerLat = points.reduce((sum, point) => sum + point.lat / points.length, 0) * Math.PI / 180;
  const sx = 111_320 * Math.cos(centerLat);
  const sy = 111_320;
  const origin = points[0];
  return Math.abs(points.reduce((sum, point, index) => {
    const next = points[(index + 1) % points.length];
    const px = (point.lng - origin.lng) * sx, py = (point.lat - origin.lat) * sy;
    const nx = (next.lng - origin.lng) * sx, ny = (next.lat - origin.lat) * sy;
    return sum + px * ny - nx * py;
  }, 0) / 2);
}

function pointInRing(point: Point, ring: Point[]) {
  let inside = false;
  for (let index = 0, previous = ring.length - 1; index < ring.length; previous = index++) {
    const left = ring[index];
    const right = ring[previous];
    if (((left.lat > point.lat) !== (right.lat > point.lat)) && point.lng < (right.lng - left.lng) * (point.lat - left.lat) / ((right.lat - left.lat) || Number.EPSILON) + left.lng) inside = !inside;
  }
  return inside;
}

function distanceToRingMeters(point: Point, ring: Point[]) {
  return Math.min(...ring.slice(0, -1).map((start, index) => distanceToSegmentMeters(point, start, ring[index + 1])));
}

function distanceToSegmentMeters(point: Point, start: Point, end: Point) {
  const lat = point.lat * Math.PI / 180;
  const scale = { lng: 111_320 * Math.cos(lat), lat: 111_320 };
  const ax = (start.lng - point.lng) * scale.lng, ay = (start.lat - point.lat) * scale.lat;
  const bx = (end.lng - point.lng) * scale.lng, by = (end.lat - point.lat) * scale.lat;
  const dx = bx - ax, dy = by - ay;
  const t = Math.max(0, Math.min(1, -(ax * dx + ay * dy) / ((dx * dx + dy * dy) || 1)));
  return Math.hypot(ax + dx * t, ay + dy * t);
}

function distanceMeters(left: Point, right: Point) {
  const lat = ((left.lat + right.lat) / 2) * Math.PI / 180;
  return Math.hypot((left.lng - right.lng) * 111_320 * Math.cos(lat), (left.lat - right.lat) * 111_320);
}

function hasSelfIntersection(ring: Point[]) {
  for (let left = 0; left < ring.length - 1; left += 1) {
    for (let right = left + 2; right < ring.length - 1; right += 1) {
      if (left === 0 && right === ring.length - 2) continue;
      if (segmentsIntersect(ring[left], ring[left + 1], ring[right], ring[right + 1])) return true;
    }
  }
  return false;
}

function segmentsIntersect(a: Point, b: Point, c: Point, d: Point) {
  const cross = (p: Point, q: Point, r: Point) => (q.lng - p.lng) * (r.lat - p.lat) - (q.lat - p.lat) * (r.lng - p.lng);
  const abC = cross(a, b, c), abD = cross(a, b, d), cdA = cross(c, d, a), cdB = cross(c, d, b);
  return abC * abD < 0 && cdA * cdB < 0;
}
