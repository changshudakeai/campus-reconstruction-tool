import type { SourceConflictDecision, SourceConflictRecord } from "../domain/sourceConflict";
import type { SchematicModel } from "../domain/schematicModel";
import { cloneSchematicProvenance } from "../domain/schematicModel";

export function detectSourceConflicts(model: SchematicModel, tolerancePercent = 15): SourceConflictRecord[] {
  const report = model.metadata.generationReport;
  const externalModels = model.metadata.provenance?.externalModels ?? [];
  if (!report || externalModels.length === 0) return [];

  const generatedWidth = report.dimensions.footprintWidthMeters;
  const generatedLength = report.dimensions.footprintLengthMeters;
  const generatedHeight = model.height / report.blocksPerMeter;

  return externalModels.flatMap((externalModel) => {
    const dimensions = externalModel.candidate.dimensionsMeters;
    if (!dimensions) return [];
    const evidence: SourceConflictRecord["evidence"] = [];
    const widthDelta = dimensions.width !== undefined ? percentDelta(dimensions.width, generatedWidth) : null;
    const lengthDelta = dimensions.length !== undefined ? percentDelta(dimensions.length, generatedLength) : null;
    const heightDelta = dimensions.height !== undefined ? percentDelta(dimensions.height, generatedHeight) : null;

    if (widthDelta !== null) evidence.push({ label: "width", value: deltaLabel(dimensions.width!, generatedWidth, widthDelta) });
    if (lengthDelta !== null) evidence.push({ label: "length", value: deltaLabel(dimensions.length!, generatedLength, lengthDelta) });
    if (heightDelta !== null) evidence.push({ label: "height", value: deltaLabel(dimensions.height!, generatedHeight, heightDelta) });

    const maxDelta = Math.max(widthDelta ?? 0, lengthDelta ?? 0, heightDelta ?? 0);
    if (maxDelta <= tolerancePercent) return [];

    return [{
      id: `external-dimension:${externalModel.candidate.id}`,
      kind: "dimension_mismatch",
      severity: maxDelta > tolerancePercent * 2 ? "blocking" : "warning",
      externalModelId: externalModel.candidate.id,
      summary: `External model dimensions differ from the generated result by up to ${maxDelta.toFixed(1)}%.`,
      evidence,
      decision: "unresolved",
      decisionReason: null,
      decidedAt: null
    }];
  });
}

export function recordSourceConflictDecision(
  model: SchematicModel,
  conflict: SourceConflictRecord,
  decision: Exclude<SourceConflictDecision, "unresolved">,
  reason: string,
  decidedAt = new Date().toISOString()
): SchematicModel {
  const normalizedReason = reason.trim();
  if (!normalizedReason) throw new Error("Source conflict decisions require a reason.");

  const provenance = cloneSchematicProvenance(model.metadata.provenance);
  if (!provenance) throw new Error("Source conflict review requires schematic provenance.");

  const decided: SourceConflictRecord = {
    ...structuredClone(conflict),
    decision,
    decisionReason: normalizedReason,
    decidedAt
  };
  provenance.sourceConflicts = [
    ...(provenance.sourceConflicts ?? []).filter((item) => item.id !== conflict.id),
    decided
  ];
  return { ...model, metadata: { ...model.metadata, provenance } };
}

export function sourceConflictsForReview(model: SchematicModel): SourceConflictRecord[] {
  const stored = model.metadata.provenance?.sourceConflicts ?? [];
  const detected = detectSourceConflicts(model);
  const storedById = new Map(stored.map((conflict) => [conflict.id, conflict]));
  return detected.map((conflict) => storedById.get(conflict.id) ?? conflict);
}

function percentDelta(observed: number, generated: number) {
  if (!Number.isFinite(observed) || !Number.isFinite(generated) || generated <= 0) return 0;
  return Math.abs(observed - generated) / generated * 100;
}

function deltaLabel(observed: number, generated: number, delta: number) {
  return `external ${observed.toFixed(1)}m vs generated ${generated.toFixed(1)}m (${delta.toFixed(1)}%)`;
}
