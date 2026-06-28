import type { CandidatePoint, MapCandidate } from "../domain/mapCandidate";

export const CANDIDATE_CONFIDENCE_RULESET_VERSION = "campus-confidence/v1";

export function classifyCampusCandidates(candidates: MapCandidate[]): MapCandidate[] {
  return candidates.map((candidate) => classifyCandidate(candidate, candidates));
}

export function classifyCandidate(candidate: MapCandidate, context: MapCandidate[] = []): MapCandidate {
  if (candidate.confidence === "manual") return { ...candidate, confidenceReasons: ["Human-reviewed geometry."] };
  let score = candidate.confidence === "high" ? 4 : candidate.confidence === "medium" ? 2 : 0;
  const reasons = [`Ruleset ${CANDIDATE_CONFIDENCE_RULESET_VERSION}.`];
  const metrics = geometryMetrics(candidate.geometry.points);
  const notes = candidate.provenance.notes.join(" ").toLowerCase();
  const subtype = candidate.featureSubtype ?? inferSubtype(candidate, notes);

  if (candidate.kind === "building") {
    if (candidate.geometry.type !== "polygon" || candidate.geometry.points.length < 4) {
      score = -4; reasons.push("Building geometry is not a closed usable footprint.");
    } else {
      if (metrics.areaM2 < 12 || metrics.minDimensionM < 1.5) { score -= 4; reasons.push("Footprint is too small or narrow for normal building review."); }
      else if (metrics.areaM2 < 28 || metrics.minDimensionM < 2.5) { score -= 2; reasons.push("Footprint is unusually small or narrow."); }
      else { score += 1; reasons.push("Footprint has plausible campus-building dimensions."); }
      if (/building=(?:roof|shed|garage|garages|service|hut|carport)/.test(notes)) { score -= 2; reasons.push("Source classification suggests an accessory or roof-only structure."); }
      if (/height=|num_floors=|building:levels=/.test(notes)) { score += 1; reasons.push("Height or floor evidence is available."); }
      if (candidate.source === "overture") { score += 1; reasons.push("Overture supplies a structured building footprint."); }
      const center = centroid(candidate.geometry.points);
      const conflict = context.find((other) => other.id !== candidate.id && ["water", "sports"].includes(other.kind) && other.geometry.type === "polygon" && pointInPolygon(center, other.geometry.points));
      if (conflict) { score -= 3; reasons.push(`Footprint conflicts with ${conflict.kind} evidence.`); }
    }
  } else if (candidate.kind === "road") {
    if (metrics.lengthM < 4) { score -= 3; reasons.push("Circulation geometry is too short."); }
    else { score += 1; reasons.push("Circulation geometry has usable continuity."); }
  } else if (candidate.kind === "water") {
    if (candidate.geometry.type === "point" || (candidate.geometry.type === "polygon" && metrics.areaM2 < 20)) { score -= 3; reasons.push("Water geometry is too small or incomplete."); }
    else { score += 1; reasons.push("Pond, lake, or river geometry is usable."); }
  } else if (candidate.kind === "vegetation") {
    if (candidate.geometry.type === "point") { score = Math.min(score, 1); reasons.push("Individual tree retained as optional detail."); }
    else { score += 1; reasons.push("Vegetation area or tree-row geometry is usable."); }
  } else if (candidate.kind === "sports") {
    if (candidate.geometry.type !== "polygon" || metrics.areaM2 < 40) { score -= 3; reasons.push("Sports geometry is incomplete or implausibly small."); }
    else { score += 1; reasons.push("Sports field or court geometry is usable."); }
  }

  if (candidate.source === "screenshot_analysis" && candidate.kind !== "building" && score > 0) {
    score += 1;
    reasons.push("Visual recovery is plausible for gap-filling but still requires review.");
  }

  const confidence = score >= 4 ? "high" : score >= 2 ? "medium" : "low";
  return { ...candidate, confidence, confidenceReasons: reasons, featureSubtype: subtype };
}

function inferSubtype(candidate: MapCandidate, notes: string) {
  if (candidate.kind === "road") {
    if (/highway=steps/.test(notes)) return "steps";
    if (/highway=(?:footway|path|pedestrian)/.test(notes)) return "pedestrian";
    if (/area:highway=|amenity=parking/.test(notes)) return "paved_area";
    return "vehicle";
  }
  if (candidate.kind === "vegetation") {
    if (/natural=tree_row/.test(notes)) return "tree_row";
    if (/natural=tree/.test(notes)) return "tree";
    return "vegetation_area";
  }
  if (candidate.kind === "sports") return notes.match(/sport=([^,; ]+)/)?.[1] ?? (notes.includes("leisure=track") ? "track" : "field");
  if (candidate.kind === "water") return notes.match(/(?:water|waterway)=([^,; ]+)/)?.[1] ?? "waterbody";
  return undefined;
}

function geometryMetrics(points: CandidatePoint[]) {
  if (!points.length) return { areaM2: 0, lengthM: 0, minDimensionM: 0 };
  const centerLat = points.reduce((sum, point) => sum + point.lat / points.length, 0) * Math.PI / 180;
  const sx = 111_320 * Math.cos(centerLat), sy = 111_320;
  const origin = points[0];
  const local = points.map((point) => ({ x: (point.lng - origin.lng) * sx, y: (point.lat - origin.lat) * sy }));
  const areaM2 = Math.abs(local.reduce((sum, point, index) => { const next = local[(index + 1) % local.length]; return sum + point.x * next.y - next.x * point.y; }, 0) / 2);
  const lengthM = local.slice(1).reduce((sum, point, index) => sum + Math.hypot(point.x - local[index].x, point.y - local[index].y), 0);
  const width = Math.max(...local.map((point) => point.x)) - Math.min(...local.map((point) => point.x));
  const height = Math.max(...local.map((point) => point.y)) - Math.min(...local.map((point) => point.y));
  return { areaM2, lengthM, minDimensionM: Math.min(width, height) };
}

function centroid(points: CandidatePoint[]) { return points.reduce((sum, point) => ({ lng: sum.lng + point.lng / points.length, lat: sum.lat + point.lat / points.length }), { lng: 0, lat: 0 }); }
function pointInPolygon(point: CandidatePoint, polygon: CandidatePoint[]) { let inside = false; for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) { const a = polygon[i], b = polygon[j]; if ((a.lat > point.lat) !== (b.lat > point.lat) && point.lng < (b.lng - a.lng) * (point.lat - a.lat) / ((b.lat - a.lat) || Number.EPSILON) + a.lng) inside = !inside; } return inside; }
