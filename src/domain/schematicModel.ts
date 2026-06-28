import type { ExternalModelProvenance } from "./externalModel";
import type { SourceConflictRecord } from "./sourceConflict";
import type { SemanticFeaturePreservationRecord } from "./semanticFeature";

export type MinecraftBlockName = `minecraft:${string}`;

export interface BlockReplacementRecord {
  sourceBlock: MinecraftBlockName;
  replacementBlock: MinecraftBlockName;
  replacedCount: number;
}

export type PreviewCameraView = "top" | "front" | "side" | "perspective";
export type VisualCheckpointKind =
  | "footprint"
  | "massing"
  | "roof"
  | "facade_rhythm"
  | "materials"
  | "recognizability";
export type VisualCheckpointDecision = "pending" | "approved" | "rejected";
export type VisualComparisonEvidenceSource = "accepted_real_result" | "arnis_reference_reconstruction";
export type VisualComparisonOutcome = "pending" | "matches" | "differs" | "inconclusive";

export interface VisualComparisonEvidence {
  source: VisualComparisonEvidenceSource;
  label: string;
  description: string;
  uri?: string;
  capturedViews: PreviewCameraView[];
  notes: string[];
}

export interface VisualResultComparison {
  evidence: VisualComparisonEvidence;
  comparedAt: string;
  summary: string;
  outcome: VisualComparisonOutcome;
  correctionNotes: string[];
}

export type AxiomImportResult = "not_tested" | "succeeded" | "failed";
export type AxiomCheckDecision = "pending" | "passed" | "failed" | "not_applicable";

export interface AxiomPlacementCheck {
  origin: { x: number; y: number; z: number };
  orientationDegrees: number | null;
  blocksPerMeter: number | null;
  schematicDimensions: {
    widthBlocks: number;
    heightBlocks: number;
    lengthBlocks: number;
  };
  footprintDimensions: {
    widthBlocks: number | null;
    lengthBlocks: number | null;
  };
  expectedSlotDimensions: {
    widthBlocks: number | null;
    lengthBlocks: number | null;
  };
  widthDeltaBlocks: number | null;
  lengthDeltaBlocks: number | null;
  toleranceBlocks: number;
  status: "fits" | "exceeds" | "unknown";
  notes: string[];
}

export interface AxiomAcceptanceRecord {
  testedAt: string;
  minecraftVersion: string;
  axiomVersion: string;
  importResult: AxiomImportResult;
  placement: AxiomPlacementCheck;
  checks: {
    orientation: AxiomCheckDecision;
    scale: AxiomCheckDecision;
    palette: AxiomCheckDecision;
    blockPlacement: AxiomCheckDecision;
  };
  screenshots: Array<{
    view: PreviewCameraView | "axiom";
    uri: string;
    note: string;
  }>;
  correctionNotes: string[];
}

export interface VisualReviewRecord {
  capturedViews: PreviewCameraView[];
  checkpoints: Record<VisualCheckpointKind, {
    decision: VisualCheckpointDecision;
    note: string;
  }>;
  resultComparison: VisualResultComparison | null;
}

export interface SchematicProvenance {
  sourcePriority: string[];
  usedSources: string[];
  missingFields: string[];
  notes: string[];
  locationAnchor?: {
    gaodePoiId: string;
    gaodeName: string;
    acquisition: "poi_search" | "map_click";
    gcj02: { lng: number; lat: number };
    wgs84: { lng: number; lat: number };
    transformation: string;
  };
  buildingParts?: unknown[];
  handoff: {
    foundationSlotId: string;
    sourceFeatureId: string;
    selectedBlock: string;
    rawId: string;
    approximateWidthMeters: number;
    approximateLengthMeters: number;
  } | null;
  sourceRecords: Array<{
    source: string;
    featureId: string;
    releaseId: string | null;
    queryBounds: { minLng: number; minLat: number; maxLng: number; maxLat: number };
    queryLimit: number;
    components: Array<{
      exterior: Array<{ lng: number; lat: number }>;
      interiorRings: Array<Array<{ lng: number; lat: number }>>;
    }>;
  }>;
  observations: unknown[];
  identityResolution: unknown;
  observationReviews: Record<string, string>;
  fieldDecisions: unknown[];
  contradictions: unknown[];
  arnisRuleDecisions: unknown[];
  generationAssumptions: unknown[];
  corrections: unknown[];
  geometryValidation: unknown;
  blockReplacements: BlockReplacementRecord[];
  visualReview?: VisualReviewRecord;
  axiomAcceptance?: AxiomAcceptanceRecord;
  externalModels?: ExternalModelProvenance[];
  sourceConflicts?: SourceConflictRecord[];
  semanticFeatures?: SemanticFeaturePreservationRecord[];
}

export interface SchematicGenerationReport {
  blocksPerMeter: number;
  paddingBlocks: number;
  dimensions: {
    widthBlocks: number;
    heightBlocks: number;
    lengthBlocks: number;
    footprintWidthMeters: number;
    footprintLengthMeters: number;
  };
  orientationDegrees: number;
  floorCount: number;
  floorSpacingBlocks: number;
  roof: {
    shape: string;
    heightBlocks: number;
    assumption: string | null;
  };
  facadeRhythm: string;
  entrance: {
    side: "south";
    widthBlocks: number;
  };
  blockCounts: Record<string, number>;
  semanticBlockCounts: {
    foundation: number;
    walls: number;
    windows: number;
    roof: number;
    floors: number;
    entrance: number;
    accents: number;
  };
  fidelity: {
    footprintIoU: number;
    areaErrorPercent: number;
    widthErrorMeters: number;
    lengthErrorMeters: number;
    orientationErrorDegrees: number;
  };
  footprintOverlay: Array<{
    exterior: Array<{ x: number; z: number }>;
    interiorRings: Array<Array<{ x: number; z: number }>>;
  }>;
}

export interface SchematicModel {
  schemaVersion: "0.1.0";
  name: string;
  width: number;
  height: number;
  length: number;
  palette: MinecraftBlockName[];
  blockData: Uint16Array;
  metadata: {
    generator: "building-geometry-to-schematic" | "foundation-manifest-to-schematic";
    sourceBuilding: string;
    nonRectangularFootprint: boolean;
    roofShape: string | null;
    generationReport: SchematicGenerationReport | null;
    provenance: SchematicProvenance | null;
  };
}

export function createEmptySchematicModel({
  name,
  width,
  height,
  length,
  palette,
  generator,
  sourceBuilding,
  nonRectangularFootprint,
  roofShape,
  generationReport = null,
  provenance = null
}: {
  name: string;
  width: number;
  height: number;
  length: number;
  palette: MinecraftBlockName[];
  generator?: SchematicModel["metadata"]["generator"];
  sourceBuilding: string;
  nonRectangularFootprint: boolean;
  roofShape: string | null;
  generationReport?: SchematicGenerationReport | null;
  provenance?: SchematicProvenance | null;
}): SchematicModel {
  return {
    schemaVersion: "0.1.0",
    name,
    width,
    height,
    length,
    palette,
    blockData: new Uint16Array(width * height * length),
    metadata: {
      generator: generator ?? "building-geometry-to-schematic",
      sourceBuilding,
      nonRectangularFootprint,
      roofShape,
      generationReport: generationReport ? structuredClone(generationReport) : null,
      provenance: cloneSchematicProvenance(provenance)
    }
  };
}

export function cloneSchematicProvenance(
  provenance: SchematicProvenance | null
): SchematicProvenance | null {
  if (!provenance) return null;
  return {
    sourcePriority: [...provenance.sourcePriority],
    usedSources: [...provenance.usedSources],
    missingFields: [...provenance.missingFields],
    notes: [...provenance.notes],
    locationAnchor: provenance.locationAnchor ? structuredClone(provenance.locationAnchor) : undefined,
    buildingParts: provenance.buildingParts?.map((part) => structuredClone(part)),
    handoff: provenance.handoff ? { ...provenance.handoff } : null,
    sourceRecords: provenance.sourceRecords.map((record) => structuredClone(record)),
    observations: provenance.observations.map((observation) => structuredClone(observation)),
    identityResolution: structuredClone(provenance.identityResolution),
    observationReviews: { ...provenance.observationReviews },
    fieldDecisions: provenance.fieldDecisions.map((decision) => structuredClone(decision)),
    contradictions: provenance.contradictions.map((contradiction) => structuredClone(contradiction)),
    arnisRuleDecisions: provenance.arnisRuleDecisions.map((decision) => structuredClone(decision)),
    generationAssumptions: provenance.generationAssumptions.map((assumption) => structuredClone(assumption)),
    corrections: provenance.corrections.map((correction) => structuredClone(correction)),
    geometryValidation: structuredClone(provenance.geometryValidation),
    blockReplacements: provenance.blockReplacements.map((record) => ({ ...record })),
    visualReview: provenance.visualReview ? structuredClone(provenance.visualReview) : undefined,
    axiomAcceptance: provenance.axiomAcceptance ? structuredClone(provenance.axiomAcceptance) : undefined,
    externalModels: provenance.externalModels?.map((model) => structuredClone(model)),
    sourceConflicts: provenance.sourceConflicts?.map((conflict) => structuredClone(conflict)),
    semanticFeatures: provenance.semanticFeatures?.map((feature) => structuredClone(feature))
  };
}

export function blockIndex(
  x: number,
  y: number,
  z: number,
  width: number,
  length: number
): number {
  return y * width * length + z * width + x;
}

export function setBlock(
  model: SchematicModel,
  x: number,
  y: number,
  z: number,
  paletteIndex: number
) {
  if (
    x < 0 ||
    y < 0 ||
    z < 0 ||
    x >= model.width ||
    y >= model.height ||
    z >= model.length
  ) {
    return;
  }

  model.blockData[blockIndex(x, y, z, model.width, model.length)] = paletteIndex;
}

export function countBlocks(model: SchematicModel, paletteIndex: number): number {
  let total = 0;
  for (const block of model.blockData) {
    if (block === paletteIndex) total += 1;
  }
  return total;
}
