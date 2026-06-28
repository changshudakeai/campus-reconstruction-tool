import type {
  BuildingGeometry,
  BuildingGeometryField,
  FacadeHints,
  FootprintComponent,
  LngLatPoint,
  RoofHints
} from "../domain/buildingGeometry";
import { synchronizeBuildingGeometryDerivedState } from "./buildingGeometryValidation";

export interface ManualBuildingGeometryCorrection {
  reason?: string;
  footprint?: LngLatPoint[];
  footprintComponents?: FootprintComponent[];
  heightM?: number;
  floors?: number;
  roof?: Partial<RoofHints>;
  facade?: Partial<FacadeHints>;
}

export interface ManualCorrectionResult {
  geometry: BuildingGeometry;
  correctedFields: string[];
}

export function applyManualBuildingGeometryCorrection(
  geometry: BuildingGeometry,
  correction: ManualBuildingGeometryCorrection
): ManualCorrectionResult {
  validateCorrection(correction);
  const correctedFields: string[] = [];
  const next = structuredClone(geometry);
  const reason = correction.reason?.trim() ?? "";

  const correctedComponents = correction.footprintComponents ?? (
    correction.footprint ? [{ exterior: correction.footprint, interiorRings: [] }] : undefined
  );
  if (correctedComponents) {
    next.footprintComponents = structuredClone(correctedComponents);
    next.footprint = structuredClone(correctedComponents[0]?.exterior ?? []);
    next.confidence.footprint = "manual";
    correctedFields.push("footprint");
  }
  if (correction.heightM !== undefined) {
    next.heightM = correction.heightM;
    next.confidence.height = "manual";
    correctedFields.push("heightM");
  }
  if (correction.floors !== undefined) {
    next.floors = correction.floors;
    next.confidence.floors = "manual";
    correctedFields.push("floors");
  }

  applyHintCorrection(next.roof, correction.roof, "roof", correctedFields);
  if (hasDefinedHint(correction.roof)) next.confidence.roof = "manual";
  applyHintCorrection(next.facade, correction.facade, "facade", correctedFields);
  if (hasDefinedHint(correction.facade)) next.confidence.facade = "manual";

  if (correctedFields.length > 0) {
    const before = snapshotFields(geometry, correctedFields);
    synchronizeBuildingGeometryDerivedState(next);
    if (!next.validation.valid) {
      throw new Error(`Manual correction is invalid: ${next.validation.errors.join(" ")}`);
    }
    if (!next.provenance.usedSources.includes("manual_correction")) {
      next.provenance.usedSources.push("manual_correction");
    }
    next.provenance.corrections.push({
      id: `manual-correction-${next.provenance.corrections.length + 1}`,
      reason,
      correctedFields: [...correctedFields],
      before,
      after: snapshotFields(next, correctedFields)
    });
    recordManualFieldDecisions(next, correctedFields, reason);
    next.provenance.notes.push(
      `Manual correction applied: ${correctedFields.join(", ")}. Reason: ${reason}`
    );
  } else {
    synchronizeBuildingGeometryDerivedState(next);
  }
  next.provenance.missingFields = findMissingFields(next);

  return { geometry: next, correctedFields };
}

function applyHintCorrection<T extends RoofHints | FacadeHints>(
  target: T,
  correction: Partial<T> | undefined,
  prefix: string,
  correctedFields: string[]
) {
  if (!correction) return;
  for (const key of Object.keys(correction) as Array<keyof T & string>) {
    const value = correction[key];
    if (value !== undefined) {
      target[key] = value as T[typeof key];
      correctedFields.push(`${prefix}.${key}`);
    }
  }
}

function hasDefinedHint(value: object | undefined) {
  return value ? Object.values(value).some((entry) => entry !== undefined) : false;
}

function validateCorrection(correction: ManualBuildingGeometryCorrection) {
  const components = correction.footprintComponents ?? (
    correction.footprint ? [{ exterior: correction.footprint, interiorRings: [] }] : undefined
  );
  const hasCorrection = Boolean(
    components || correction.heightM !== undefined || correction.floors !== undefined ||
    hasDefinedHint(correction.roof) || hasDefinedHint(correction.facade)
  );
  if (hasCorrection && !correction.reason?.trim()) {
    throw new Error("Manual geometry corrections require a reason.");
  }
  if (components?.some((component) => component.exterior.length < 3)) {
    throw new Error("Manual footprint correction must contain at least three points.");
  }
  if (correction.heightM !== undefined && (!Number.isFinite(correction.heightM) || correction.heightM <= 0)) {
    throw new Error("Manual height correction must be a positive number.");
  }
  if (correction.floors !== undefined && (!Number.isInteger(correction.floors) || correction.floors <= 0)) {
    throw new Error("Manual floor correction must be a positive integer.");
  }
}

function recordManualFieldDecisions(
  geometry: BuildingGeometry,
  correctedFields: string[],
  reason: string
) {
  for (const field of correctedFields as BuildingGeometryField[]) {
    const decision = {
      field,
      value: fieldValue(geometry, field),
      source: "manual_correction" as const,
      observationId: null,
      qualityScore: 1,
      confidence: "manual" as const,
      ruleId: null,
      explanation: `Reviewed manual correction: ${reason}`
    };
    const index = geometry.provenance.fieldDecisions.findIndex((item) => item.field === field);
    if (index >= 0) geometry.provenance.fieldDecisions[index] = decision;
    else geometry.provenance.fieldDecisions.push(decision);
  }
}

function snapshotFields(geometry: BuildingGeometry, fields: string[]) {
  return Object.fromEntries(fields.map((field) => [
    field,
    structuredClone(fieldValue(geometry, field as BuildingGeometryField))
  ]));
}

function fieldValue(geometry: BuildingGeometry, field: BuildingGeometryField) {
  if (field === "footprint") return geometry.footprintComponents;
  if (field === "heightM") return geometry.heightM;
  if (field === "floors") return geometry.floors;
  if (field === "roof.shape") return geometry.roof.shape;
  if (field === "roof.material") return geometry.roof.material;
  if (field === "roof.orientation") return geometry.roof.orientation;
  if (field === "facade.material") return geometry.facade.material;
  return geometry.facade.color;
}

function findMissingFields(geometry: BuildingGeometry) {
  const missing: string[] = [];
  if (geometry.footprint.length < 3) missing.push("footprint");
  if (!geometry.heightM) missing.push("heightM");
  if (!geometry.floors) missing.push("floors");
  if (!geometry.roof.shape) missing.push("roof.shape");
  if (!geometry.facade.material && !geometry.facade.color) missing.push("facade");
  return missing;
}
