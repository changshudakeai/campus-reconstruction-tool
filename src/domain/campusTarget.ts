import type { CandidatePoint, MapCandidate, OnlineMapQueryTarget } from "./mapCandidate";
import { gcj02ToWgs84 } from "../services/buildingLocationAnchor";

export interface CampusTarget {
  id: string;
  schoolName: string;
  canonicalName: string;
  aliases: string[];
  center: CandidatePoint;
  openCenter: CandidatePoint;
  radiusM: number;
  gaodePoiId: string;
  blocksPerMeter: number;
  orientationDegrees: number;
  orientationLine: [CandidatePoint, CandidatePoint] | null;
}

const KNOWN_CAMPUSES = [
  {
    schoolName: "华东师范大学",
    canonicalName: "华东师范大学普陀校区",
    aliases: ["华东师范大学中北校区", "华师大普陀校区", "华师大中北校区"]
  }
] as const;

export function campusTargetFromGaodeCandidate(candidate: MapCandidate, query: string): CampusTarget {
  const known = KNOWN_CAMPUSES.find((campus) =>
    [campus.canonicalName, ...campus.aliases].some((name) =>
      query.includes(name) || candidate.name.includes(name)
    )
  );
  const canonicalName = known?.canonicalName ?? candidate.name;
  const schoolName = known?.schoolName ?? inferSchoolName(canonicalName);
  const point = candidate.geometry.points[0];
  return {
    id: `campus-${slug(candidate.provenance.rawId || canonicalName)}`,
    schoolName,
    canonicalName,
    aliases: Array.from(new Set([candidate.name, query, ...(known?.aliases ?? [])])).filter(
      (name) => name && name !== canonicalName
    ),
    center: point,
    openCenter: gcj02ToWgs84(point),
    radiusM: 900,
    gaodePoiId: candidate.provenance.rawId,
    blocksPerMeter: 1,
    orientationDegrees: 0,
    orientationLine: null
  };
}

export function campusOnlineQueryTarget(campus: CampusTarget, query = campus.canonicalName): OnlineMapQueryTarget {
  return { query, campus: campus.canonicalName, center: campus.openCenter, gaodeCenter: campus.center, radiusM: campus.radiusM };
}

function inferSchoolName(name: string) {
  const match = name.match(/^(.+?(?:大学|学院|学校))/);
  return match?.[1] ?? name;
}

function slug(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, "-").replace(/^-|-$/g, "");
}
