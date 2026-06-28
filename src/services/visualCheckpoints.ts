import type {
  PreviewCameraView,
  SchematicModel,
  VisualCheckpointDecision,
  VisualCheckpointKind,
  VisualComparisonEvidenceSource,
  VisualComparisonOutcome,
  VisualResultComparison,
  VisualReviewRecord
} from "../domain/schematicModel";
import { cloneSchematicProvenance } from "../domain/schematicModel";

export const PREVIEW_CAMERA_VIEWS: PreviewCameraView[] = ["top", "front", "side", "perspective"];
export const VISUAL_CHECKPOINTS: VisualCheckpointKind[] = [
  "footprint", "massing", "roof", "facade_rhythm", "materials", "recognizability"
];

const VISUAL_COMPARISON_EVIDENCE_SOURCES: VisualComparisonEvidenceSource[] = [
  "accepted_real_result",
  "arnis_reference_reconstruction"
];

export function createVisualReview(): VisualReviewRecord {
  return {
    capturedViews: [],
    checkpoints: Object.fromEntries(
      VISUAL_CHECKPOINTS.map((kind) => [kind, { decision: "pending", note: "" }])
    ) as VisualReviewRecord["checkpoints"],
    resultComparison: null
  };
}

export function recordCapturedView(model: SchematicModel, view: PreviewCameraView): SchematicModel {
  return updateReview(model, (review) => ({
    ...review,
    capturedViews: review.capturedViews.includes(view)
      ? review.capturedViews
      : [...review.capturedViews, view]
  }));
}

export function recordCheckpointDecision(
  model: SchematicModel,
  kind: VisualCheckpointKind,
  decision: VisualCheckpointDecision,
  note: string
): SchematicModel {
  const normalizedNote = note.trim();
  if (decision === "rejected" && !normalizedNote) {
    throw new Error("A rejection note is required.");
  }
  return updateReview(model, (review) => ({
    ...review,
    checkpoints: {
      ...review.checkpoints,
      [kind]: { decision, note: normalizedNote }
    }
  }));
}

export function recordResultComparison(
  model: SchematicModel,
  comparison: {
    evidence: {
      source: VisualComparisonEvidenceSource;
      label: string;
      description: string;
      uri?: string;
      capturedViews?: PreviewCameraView[];
      notes?: string[];
    };
    outcome: VisualComparisonOutcome;
    summary: string;
    correctionNotes?: string[];
    comparedAt?: string;
  }
): SchematicModel {
  const evidenceSource = comparison.evidence.source;
  if (!VISUAL_COMPARISON_EVIDENCE_SOURCES.includes(evidenceSource)) {
    throw new Error("Visual comparison must use accepted real-result evidence or the Arnis Reference Reconstruction.");
  }

  const label = comparison.evidence.label.trim();
  const description = comparison.evidence.description.trim();
  const summary = comparison.summary.trim();
  if (!label) throw new Error("Visual comparison evidence label is required.");
  if (!description) throw new Error("Visual comparison evidence description is required.");
  if (!summary) throw new Error("Visual comparison summary is required.");

  const correctionNotes = normalizeNotes(comparison.correctionNotes ?? []);
  if (comparison.outcome === "differs" && correctionNotes.length === 0) {
    throw new Error("A differing comparison requires at least one correction note.");
  }

  const resultComparison: VisualResultComparison = {
    evidence: {
      source: evidenceSource,
      label,
      description,
      uri: comparison.evidence.uri?.trim() || undefined,
      capturedViews: [...new Set(comparison.evidence.capturedViews ?? [])],
      notes: normalizeNotes(comparison.evidence.notes ?? [])
    },
    comparedAt: comparison.comparedAt ?? new Date().toISOString(),
    summary,
    outcome: comparison.outcome,
    correctionNotes
  };

  return updateReview(model, (review) => ({ ...review, resultComparison }));
}

export function visualReviewFor(model: SchematicModel): VisualReviewRecord {
  const review = structuredClone(model.metadata.provenance?.visualReview ?? createVisualReview());
  return {
    ...createVisualReview(),
    ...review,
    checkpoints: {
      ...createVisualReview().checkpoints,
      ...review.checkpoints
    },
    resultComparison: review.resultComparison ?? null
  };
}

function normalizeNotes(notes: string[]) {
  return notes.map((note) => note.trim()).filter(Boolean);
}

function updateReview(model: SchematicModel, change: (review: VisualReviewRecord) => VisualReviewRecord) {
  const provenance = cloneSchematicProvenance(model.metadata.provenance);
  if (!provenance) throw new Error("Visual review requires schematic provenance.");
  provenance.visualReview = change(visualReviewFor(model));
  return { ...model, metadata: { ...model.metadata, provenance } };
}
