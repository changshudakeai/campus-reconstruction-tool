import type { CampusTarget } from "../domain/campusTarget";
import type { LngLatPoint } from "../domain/buildingGeometry";

export type BuildingNameSource = "shared_annotation" | "gaode_reverse_geocode" | "manual";
export type CampusBuildingRecordStatus = "included" | "excluded";
export type CampusBuildingClassificationSource = "automatic_name_filter" | "manual";

export interface CampusBuildingNameRecord {
  sourceId: string;
  name: string;
  updatedAt: string;
  nameSource?: BuildingNameSource;
  gcj02?: LngLatPoint;
  wgs84?: LngLatPoint;
  status?: CampusBuildingRecordStatus;
  classificationSource?: CampusBuildingClassificationSource;
  classificationReason?: string;
}

export interface CampusBuildingSuppression {
  sourceId: string;
  wgs84?: LngLatPoint;
  deletedAt: string;
  reason: string;
}

function storageKey(campus: CampusTarget) {
  return `campus-building-directory:${encodeURIComponent(campus.canonicalName)}`;
}

function suppressionStorageKey(campus: CampusTarget) {
  return `campus-building-suppressions:${encodeURIComponent(campus.canonicalName)}`;
}

export function loadCampusBuildingDirectory(campus: CampusTarget): CampusBuildingNameRecord[] {
  try {
    const raw = localStorage.getItem(storageKey(campus));
    const records = raw ? JSON.parse(raw) as CampusBuildingNameRecord[] : [];
    const legacyExcluded = records.filter((record) => record.status === "excluded");
    if (legacyExcluded.length) {
      let suppressions = loadCampusBuildingSuppressions(campus);
      for (const record of legacyExcluded) {
        suppressions = upsertSuppression(suppressions, {
          sourceId: canonicalBuildingSourceId(record.sourceId),
          wgs84: record.wgs84,
          deletedAt: record.updatedAt,
          reason: record.classificationReason ?? "Migrated legacy exclusion."
        });
      }
      saveCampusBuildingSuppressions(campus, suppressions);
      const included = records.filter((record) => record.status !== "excluded");
      localStorage.setItem(storageKey(campus), JSON.stringify(included));
      return included;
    }
    return records;
  } catch {
    return [];
  }
}

export function saveCampusBuildingName(
  campus: CampusTarget,
  sourceId: string,
  name: string,
  details: Partial<Pick<CampusBuildingNameRecord, "nameSource" | "gcj02" | "wgs84">> = {}
): CampusBuildingNameRecord[] {
  const trimmed = name.trim();
  if (!trimmed) return loadCampusBuildingDirectory(campus);
  const canonicalSourceId = canonicalBuildingSourceId(sourceId);
  const records = removeCampusBuildingRecord(campus, canonicalSourceId);
  const next = [...records, {
    sourceId: canonicalSourceId,
    name: trimmed,
    updatedAt: new Date().toISOString(),
    status: "included" as const,
    ...details
  }];
  localStorage.setItem(storageKey(campus), JSON.stringify(next));
  return next;
}

export function replaceCampusBuildingDirectory(campus: CampusTarget, records: readonly CampusBuildingNameRecord[]) {
  const normalized = records.filter((record) => record.status !== "excluded").map((record) => ({
    ...record,
    sourceId: canonicalBuildingSourceId(record.sourceId)
  }));
  localStorage.setItem(storageKey(campus), JSON.stringify(normalized));
  return normalized;
}

export function removeCampusBuildingRecord(campus: CampusTarget, sourceId: string) {
  const canonicalSourceId = canonicalBuildingSourceId(sourceId);
  const next = loadCampusBuildingDirectory(campus).filter(
    (record) => canonicalBuildingSourceId(record.sourceId) !== canonicalSourceId
  );
  localStorage.setItem(storageKey(campus), JSON.stringify(next));
  return next;
}

export function loadCampusBuildingSuppressions(campus: CampusTarget): CampusBuildingSuppression[] {
  try {
    const raw = localStorage.getItem(suppressionStorageKey(campus));
    return raw ? JSON.parse(raw) as CampusBuildingSuppression[] : [];
  } catch {
    return [];
  }
}

export function saveCampusBuildingSuppressions(
  campus: CampusTarget,
  suppressions: readonly CampusBuildingSuppression[]
) {
  const normalized = suppressions.map((suppression) => ({
    ...suppression,
    sourceId: canonicalBuildingSourceId(suppression.sourceId)
  }));
  localStorage.setItem(suppressionStorageKey(campus), JSON.stringify(normalized));
  return normalized;
}

export function suppressCampusBuilding(
  campus: CampusTarget,
  sourceId: string,
  details: { wgs84?: LngLatPoint; reason: string }
) {
  const suppression = {
    sourceId: canonicalBuildingSourceId(sourceId),
    wgs84: details.wgs84,
    deletedAt: new Date().toISOString(),
    reason: details.reason
  };
  const next = upsertSuppression(loadCampusBuildingSuppressions(campus), suppression);
  saveCampusBuildingSuppressions(campus, next);
  removeCampusBuildingRecord(campus, sourceId);
  return next;
}

export function isIncludedCampusBuildingRecord(record: CampusBuildingNameRecord) {
  return record.status !== "excluded" && Boolean(record.name.trim());
}

export function findCampusBuildingRecord(
  records: readonly CampusBuildingNameRecord[],
  sourceId: string,
  wgs84?: LngLatPoint,
  maxDistanceM = 20
) {
  return findRecordForGeometry(records, sourceId, [], wgs84, maxDistanceM);
}

export function findCampusBuildingRecordForGeometry(
  records: readonly CampusBuildingNameRecord[],
  sourceId: string,
  exteriors: readonly (readonly LngLatPoint[])[],
  center?: LngLatPoint,
  maxDistanceM = 20
) {
  return findRecordForGeometry(records, sourceId, exteriors, center, maxDistanceM);
}

export function findCampusBuildingSuppression(
  suppressions: readonly CampusBuildingSuppression[],
  sourceId: string,
  exteriors: readonly (readonly LngLatPoint[])[] = [],
  center?: LngLatPoint,
  maxDistanceM = 20
) {
  return findRecordForGeometry(suppressions, sourceId, exteriors, center, maxDistanceM);
}

function findRecordForGeometry<T extends { sourceId: string; wgs84?: LngLatPoint }>(
  records: readonly T[],
  sourceId: string,
  exteriors: readonly (readonly LngLatPoint[])[],
  center?: LngLatPoint,
  maxDistanceM = 20
) {
  const canonicalSourceId = canonicalBuildingSourceId(sourceId);
  const exact = records.find(
    (record) => canonicalBuildingSourceId(record.sourceId) === canonicalSourceId
  );
  if (exact) return exact;
  const contained = records.find(
    (record) => record.wgs84 && exteriors.some((ring) => pointInRing(record.wgs84!, ring))
  );
  if (contained || !center) return contained;
  return records
    .filter((record) => record.wgs84)
    .map((record) => ({ record, distance: distanceMeters(record.wgs84!, center) }))
    .filter((match) => match.distance <= maxDistanceM)
    .sort((left, right) => left.distance - right.distance)[0]?.record;
}

export function canonicalBuildingSourceId(sourceId: string) {
  return sourceId.replace(/^osm:(?:way|relation):/, "osm:");
}

export function mergeCampusBuildingDirectories(
  shared: CampusBuildingNameRecord[],
  local: CampusBuildingNameRecord[]
) {
  const merged = new Map(
    shared
      .filter((record) => record.status !== "excluded")
      .map((record) => [canonicalBuildingSourceId(record.sourceId), record])
  );
  for (const record of local) {
    if (record.status !== "excluded") {
      merged.set(canonicalBuildingSourceId(record.sourceId), record);
    }
  }
  return Array.from(merged.values());
}

function upsertSuppression(
  suppressions: readonly CampusBuildingSuppression[],
  next: CampusBuildingSuppression
) {
  const sourceId = canonicalBuildingSourceId(next.sourceId);
  return [
    ...suppressions.filter(
      (suppression) => canonicalBuildingSourceId(suppression.sourceId) !== sourceId
    ),
    { ...next, sourceId }
  ];
}

function pointInRing(point: LngLatPoint, ring: readonly LngLatPoint[]) {
  if (ring.length < 3) return false;
  let inside = false;
  for (let current = 0, previous = ring.length - 1; current < ring.length; previous = current++) {
    const a = ring[current];
    const b = ring[previous];
    if (
      (a.lat > point.lat) !== (b.lat > point.lat) &&
      point.lng < ((b.lng - a.lng) * (point.lat - a.lat)) / (b.lat - a.lat) + a.lng
    ) {
      inside = !inside;
    }
  }
  return inside;
}

function distanceMeters(left: LngLatPoint, right: LngLatPoint) {
  const latScale = 111_320;
  const lngScale = latScale * Math.cos(((left.lat + right.lat) / 2) * Math.PI / 180);
  return Math.hypot((left.lng - right.lng) * lngScale, (left.lat - right.lat) * latScale);
}
