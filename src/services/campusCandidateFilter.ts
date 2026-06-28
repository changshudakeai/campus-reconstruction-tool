import type { MapCandidate } from "../domain/mapCandidate";
import type { CampusTarget } from "../domain/campusTarget";

const NON_CAMPUS_SUFFIXES = [
  "停车场", "停车库", "体育馆", "食堂", "宿舍", "公寓", "学院", "学部", "系",
  "中心", "办公室", "财务处", "后勤", "图书馆", "校门", "东门", "西门", "南门", "北门"
];

export function filterCampusCandidates(candidates: readonly MapCandidate[], query: string): MapCandidate[] {
  const normalizedQuery = normalize(query);
  return candidates
    .filter((candidate) => isCampusName(candidate.name, normalizedQuery))
    .sort((left, right) => campusRank(right.name, normalizedQuery) - campusRank(left.name, normalizedQuery));
}

export function filterBuildingCandidatesToCampus(
  candidates: readonly MapCandidate[],
  campus: CampusTarget
) {
  const terms = [campus.canonicalName, ...campus.aliases, campus.schoolName]
    .map(normalize)
    .filter((term) => term.length >= 4);
  return candidates.filter((candidate) => {
    const point = candidate.geometry.points[0];
    if (!point || distanceMeters(point, campus.center) > campus.radiusM) return false;
    const evidence = normalize(`${candidate.name} ${candidate.provenance.notes.join(" ")}`);
    return terms.some((term) => evidence.includes(term));
  });
}

function isCampusName(name: string, query: string) {
  const normalizedName = normalize(name);
  if (!normalizedName || NON_CAMPUS_SUFFIXES.some((suffix) => normalizedName.endsWith(suffix))) return false;
  if (query.endsWith("校区") || query.endsWith("校园")) return normalizedName === query;
  if (!normalizedName.startsWith(query)) return false;
  const suffix = normalizedName.slice(query.length);
  if (/附属|中学|小学|学校|学院|医院/.test(suffix)) return false;
  return suffix.endsWith("校区") || suffix.endsWith("校园");
}

function campusRank(name: string, query: string) {
  const normalizedName = normalize(name);
  return normalizedName === query ? 3 : normalizedName.endsWith("校区") ? 2 : 1;
}

function normalize(value: string) {
  return value.replace(/[\s()（）·]/g, "").toLowerCase();
}

function distanceMeters(left: { lng: number; lat: number }, right: { lng: number; lat: number }) {
  const latScale = 111_320;
  const lngScale = 111_320 * Math.cos(((left.lat + right.lat) / 2) * Math.PI / 180);
  return Math.hypot((left.lng - right.lng) * lngScale, (left.lat - right.lat) * latScale);
}
