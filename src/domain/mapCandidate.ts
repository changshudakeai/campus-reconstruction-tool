import type { MapFeatureKind, SourceConfidence } from "./foundationManifest";

export type CandidateSource =
  | "arnis_open_geodata"
  | "overture"
  | "osm_overpass"
  | "gaode_poi"
  | "gaode_aoi"
  | "screenshot_analysis"
  | "manual_drawing";

export type GeometryType = "polygon" | "polyline" | "point";

export interface CandidatePoint {
  lng: number;
  lat: number;
}

export interface CandidateGeometry {
  type: GeometryType;
  points: CandidatePoint[];
}

export interface CandidateProvenance {
  source: CandidateSource;
  sourceLabel: string;
  query: string;
  rawId: string;
  notes: string[];
  coordinateSystem?: "GCJ-02" | "WGS-84";
}

export interface MapCandidate {
  id: string;
  name: string;
  kind: MapFeatureKind;
  source: CandidateSource;
  confidence: SourceConfidence;
  confidenceReasons?: string[];
  featureSubtype?: string;
  geometry: CandidateGeometry;
  provenance: CandidateProvenance;
  editable: true;
  accepted: false;
}

export interface OnlineMapQueryTarget {
  query: string;
  campus: string;
  center: CandidatePoint;
  gaodeCenter?: CandidatePoint;
  radiusM: number;
}

export const MAP_CANDIDATE_SOURCE_PRIORITY: CandidateSource[] = [
  "overture",
  "osm_overpass",
  "gaode_poi",
  "gaode_aoi",
  "manual_drawing",
  "arnis_open_geodata",
  "screenshot_analysis"
];

export const PUTUO_ONLINE_QUERY_TARGET: OnlineMapQueryTarget = {
  query: "华东师范大学普陀校区",
  campus: "ECNU Putuo Campus",
  center: {
    lng: 121.409,
    lat: 31.228
  },
  radiusM: 650
};

export const PUTUO_LIBRARY_SEARCH_TARGET: OnlineMapQueryTarget = {
  query: "华东师范大学普陀校区图书馆",
  campus: "ECNU Putuo Campus",
  center: PUTUO_ONLINE_QUERY_TARGET.center,
  radiusM: 250
};

export function isAoiCandidate(candidate: MapCandidate) {
  return candidate.source === "gaode_aoi";
}
