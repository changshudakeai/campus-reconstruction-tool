import type { CampusTarget } from "../domain/campusTarget";
import type { MapCandidate } from "../domain/mapCandidate";
import { wgs84ToGcj02 } from "./buildingLocationAnchor";
import {
  canonicalBuildingSourceId,
  type CampusBuildingNameRecord
} from "./campusBuildingDirectory";
import { reverseGeocodeGaode } from "./liveMapProviders";

interface ReverseGeocodeAttempt {
  record: CampusBuildingNameRecord | null;
  attemptedAt: string;
  error?: string;
}

export function candidateCenter(candidate: MapCandidate) {
  const points = candidate.geometry.points;
  return points.reduce(
    (sum, point) => ({
      lng: sum.lng + point.lng / points.length,
      lat: sum.lat + point.lat / points.length
    }),
    { lng: 0, lat: 0 }
  );
}

export function candidateInsideCampus(candidate: MapCandidate, campus: CampusTarget) {
  const center = candidateCenter(candidate);
  return distanceMeters(center, campus.openCenter) <= campus.radiusM;
}

export function isCampusAffiliatedName(name: string, campus: CampusTarget) {
  const normalizedName = normalizeIdentity(name);
  if (!normalizedName) return false;
  return [campus.canonicalName, ...campus.aliases, campus.schoolName]
    .map(normalizeIdentity)
    .filter((term) => term.length >= 4)
    .some((term) => normalizedName.startsWith(term));
}

export async function mapWithConcurrency<T, R>(
  values: readonly T[],
  concurrency: number,
  mapper: (value: T, index: number) => Promise<R>
): Promise<R[]> {
  const results = new Array<R>(values.length);
  let cursor = 0;
  const workerCount = Math.min(values.length, Math.max(1, Math.floor(concurrency)));
  await Promise.all(Array.from({ length: workerCount }, async () => {
    while (cursor < values.length) {
      const index = cursor;
      cursor += 1;
      results[index] = await mapper(values[index], index);
    }
  }));
  return results;
}

export async function reverseGeocodeBuildingCandidate(
  candidate: MapCandidate,
  campus: CampusTarget,
  reverseGeocode: typeof reverseGeocodeGaode = reverseGeocodeGaode
): Promise<{ record: CampusBuildingNameRecord | null; cached: boolean }> {
  const sourceId = canonicalBuildingSourceId(candidate.provenance.rawId);
  const key = `gaode-reverse-geocode:v2:${encodeURIComponent(campus.canonicalName)}:${sourceId}`;
  const cached = localStorage.getItem(key);
  if (cached) {
    return {
      record: (JSON.parse(cached) as ReverseGeocodeAttempt).record,
      cached: true
    };
  }
  const wgs84 = candidateCenter(candidate);
  const gcj02 = wgs84ToGcj02(wgs84);
  let result;
  try {
    result = await reverseGeocode(gcj02);
  } catch (error) {
    localStorage.setItem(key, JSON.stringify({
      record: null,
      attemptedAt: new Date().toISOString(),
      error: error instanceof Error ? error.message : String(error)
    } satisfies ReverseGeocodeAttempt));
    throw error;
  }
  const campusMatch = result.candidates?.find((candidate) =>
    isCampusAffiliatedName(candidate.name, campus)
  );
  const selectedName = campusMatch?.name ?? (result.name && isCampusAffiliatedName(result.name, campus) && isUsableBuildingLikeName(result.name) ? result.name : "");
  const record = selectedName ? {
    sourceId,
    name: selectedName,
    updatedAt: new Date().toISOString(),
    nameSource: "gaode_reverse_geocode" as const,
    gcj02,
    wgs84
  } : null;
  localStorage.setItem(key, JSON.stringify({
    record,
    attemptedAt: new Date().toISOString()
  } satisfies ReverseGeocodeAttempt));
  return { record, cached: false };
}

export function clearReverseGeocodeBuildingCandidateCache(candidate: MapCandidate, campus: CampusTarget) {
  const sourceId = canonicalBuildingSourceId(candidate.provenance.rawId);
  localStorage.removeItem(`gaode-reverse-geocode:v2:${encodeURIComponent(campus.canonicalName)}:${sourceId}`);
}

function isUsableBuildingLikeName(name: string) {
  const normalized = normalizeIdentity(name);
  return Boolean(normalized) && !/停车|车库|出入口|入口|出口|道路|公交|地铁|厕所|卫生间|ATM|快递|充电|门卫|校门/.test(name);
}

function normalizeIdentity(value: string) {
  return value.replace(/[\s()（）·_-]/g, "").toLowerCase();
}

function distanceMeters(left: { lng: number; lat: number }, right: { lng: number; lat: number }) {
  const latScale = 111_320;
  const lngScale = 111_320 * Math.cos(((left.lat + right.lat) / 2) * Math.PI / 180);
  return Math.hypot((left.lng - right.lng) * lngScale, (left.lat - right.lat) * latScale);
}
