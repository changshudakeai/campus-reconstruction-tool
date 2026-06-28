export type SourceConflictKind = "dimension_mismatch" | "license_blocked" | "visual_mismatch" | "source_disagreement";
export type SourceConflictSeverity = "info" | "warning" | "blocking";
export type SourceConflictDecision = "unresolved" | "primary_selected" | "supporting_only" | "rejected";

export interface SourceConflictRecord {
  id: string;
  kind: SourceConflictKind;
  severity: SourceConflictSeverity;
  externalModelId: string | null;
  summary: string;
  evidence: Array<{
    label: string;
    value: string;
  }>;
  decision: SourceConflictDecision;
  decisionReason: string | null;
  decidedAt: string | null;
}
