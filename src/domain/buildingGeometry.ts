export type BuildingGeometrySource =
  | "overture"
  | "osm_overpass"
  | "existing_project"
  | "arnis_derived"
  | "manual_correction";

export type GeometryConfidence = "high" | "medium" | "low" | "manual" | "missing";

export interface LngLatPoint {
  lng: number;
  lat: number;
}

export interface GeographicBounds {
  minLng: number;
  minLat: number;
  maxLng: number;
  maxLat: number;
}

export interface FootprintComponent {
  exterior: LngLatPoint[];
  interiorRings: LngLatPoint[][];
}

export interface BuildingPartGeometry {
  id: string;
  component: FootprintComponent;
  tags: Record<string, string>;
  heightM: number | null;
  minHeightM: number | null;
  floors: number | null;
  minLevel: number | null;
  roofShape: string | null;
}

export interface BuildingSourceRecord {
  source: BuildingGeometrySource;
  featureId: string;
  releaseId: string | null;
  queryBounds: GeographicBounds;
  queryLimit: number;
  components: FootprintComponent[];
}

export interface BuildingObservationMetrics {
  areaSquareMeters: number;
  widthMeters: number;
  lengthMeters: number;
  orientationDegrees: number;
  pointCount: number;
  center: LngLatPoint;
}

export interface BuildingGeometryObservation {
  id: string;
  source: BuildingGeometrySource;
  sourceFeatureId: string;
  name: string | null;
  tags: Record<string, string>;
  components: FootprintComponent[];
  metrics: BuildingObservationMetrics;
  normalizationNotes: string[];
}

export interface BuildingTarget {
  name: string;
  campus: string;
  aliases: string[];
  approximateCenter: LngLatPoint;
  locationAnchor?: {
    gaodePoiId: string;
    gaodeName: string;
    acquisition: "poi_search" | "map_click";
    gcj02: LngLatPoint;
    wgs84: LngLatPoint;
    transformation: "gcj02-to-wgs84-iterative-v1";
  };
  reviewedSlot?: {
    id: string;
    footprint: LngLatPoint[];
    approximateWidthMeters: number;
    approximateLengthMeters: number;
  };
}

export type ObservationReviewStatus = "pending" | "accepted" | "rejected" | "supporting";
export type IdentityMatchConfidence = "high" | "medium" | "low" | "rejected";

export interface IdentityMatchReason {
  criterion: "name" | "overlap" | "distance" | "dimensions" | "plausibility";
  score: number;
  message: string;
}

export interface BuildingIdentityMatch {
  observationId: string;
  score: number;
  confidence: IdentityMatchConfidence;
  reasons: IdentityMatchReason[];
  reviewRequired: boolean;
}

export interface BuildingIdentityResolution {
  targetSlotId: string | null;
  selectedObservationId: string | null;
  ambiguous: boolean;
  matches: BuildingIdentityMatch[];
}

export type BuildingGeometryField =
  | "footprint"
  | "heightM"
  | "floors"
  | "roof.shape"
  | "roof.material"
  | "roof.orientation"
  | "facade.material"
  | "facade.color";

export interface BuildingFieldDecision {
  field: BuildingGeometryField;
  value: unknown;
  source: BuildingGeometrySource;
  observationId: string | null;
  qualityScore: number;
  confidence: GeometryConfidence;
  ruleId: string | null;
  explanation: string;
}

export interface BuildingFieldContradiction {
  field: BuildingGeometryField;
  candidates: Array<{
    source: BuildingGeometrySource;
    observationId: string | null;
    value: unknown;
    confidence: GeometryConfidence;
    qualityScore: number;
  }>;
  message: string;
}

export interface BuildingGeometryScale {
  areaSquareMeters: number;
  widthMeters: number;
  lengthMeters: number;
}

export interface BuildingGenerationAssumption {
  field: BuildingGeometryField | "floorSpacingMeters";
  value: unknown;
  reason: string;
  ruleId: string | null;
}

export interface BuildingGeometryCorrectionRecord {
  id: string;
  reason: string;
  correctedFields: string[];
  before: Record<string, unknown>;
  after: Record<string, unknown>;
}

export interface BuildingGeometryValidation {
  valid: boolean;
  errors: string[];
  warnings: string[];
  componentCount: number;
  orientationDegrees: number;
  scale: BuildingGeometryScale;
  floorSpacingMeters: number | null;
}

export interface ArnisRuleDecision {
  ruleId: string;
  field: BuildingGeometryField | "facade.rhythm";
  upstreamReference: string;
  inputObservationIds: string[];
  output: unknown;
  explanation: string;
}

export interface RoofHints {
  shape: string | null;
  material: string | null;
  orientation: string | null;
}

export interface FacadeHints {
  material: string | null;
  color: string | null;
}

export interface FieldConfidence {
  footprint: GeometryConfidence;
  height: GeometryConfidence;
  floors: GeometryConfidence;
  roof: GeometryConfidence;
  facade: GeometryConfidence;
}

export interface BuildingGeometryHandoff {
  foundationSlotId: string;
  sourceFeatureId: string;
  selectedBlock: string;
  rawId: string;
  approximateWidthMeters: number;
  approximateLengthMeters: number;
}

export interface GeometryProvenance {
  sourcePriority: BuildingGeometrySource[];
  usedSources: BuildingGeometrySource[];
  missingFields: string[];
  notes: string[];
  handoff: BuildingGeometryHandoff | null;
  sourceRecords: BuildingSourceRecord[];
  observations: BuildingGeometryObservation[];
  identityResolution: BuildingIdentityResolution;
  observationReviews: Record<string, ObservationReviewStatus>;
  fieldDecisions: BuildingFieldDecision[];
  contradictions: BuildingFieldContradiction[];
  arnisRuleDecisions: ArnisRuleDecision[];
  generationAssumptions: BuildingGenerationAssumption[];
  corrections: BuildingGeometryCorrectionRecord[];
}

export interface BuildingGeometry {
  schemaVersion: "0.1.0";
  buildingName: string;
  target: BuildingTarget;
  footprint: LngLatPoint[];
  footprintComponents: FootprintComponent[];
  buildingParts?: BuildingPartGeometry[];
  orientationDegrees: number;
  scale: BuildingGeometryScale;
  heightM: number | null;
  floors: number | null;
  floorSpacingMeters: number | null;
  roof: RoofHints;
  facade: FacadeHints;
  confidence: FieldConfidence;
  validation: BuildingGeometryValidation;
  provenance: GeometryProvenance;
}

export const BUILDING_GEOMETRY_SOURCE_PRIORITY: BuildingGeometrySource[] = [
  "overture",
  "osm_overpass",
  "existing_project",
  "arnis_derived",
  "manual_correction"
];

export const PUTUO_LIBRARY_TARGET: BuildingTarget = {
  name: "Putuo Campus Library",
  campus: "ECNU Putuo Campus",
  aliases: ["图书馆", "华东师范大学普陀校区图书馆", "ECNU Putuo Library"],
  approximateCenter: {
    lng: 121.409,
    lat: 31.228
  }
};
