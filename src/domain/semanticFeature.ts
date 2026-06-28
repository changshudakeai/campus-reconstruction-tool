export type SemanticFeatureKind =
  | "entrance_emphasis"
  | "window_band"
  | "roof_ridge"
  | "cornice"
  | "frame";

export type SemanticFeatureSide = "north" | "south" | "east" | "west" | "center";
export type SemanticFeatureHeightBand = "lower" | "middle" | "upper" | "roof";

export interface SemanticFeatureAnnotation {
  id: string;
  kind: SemanticFeatureKind;
  label: string;
  side: SemanticFeatureSide;
  heightBand: SemanticFeatureHeightBand;
  strength: "subtle" | "visible" | "strong";
  reason: string;
}

export interface SemanticFeaturePreservationRecord {
  annotation: SemanticFeatureAnnotation;
  appliedAt: string;
  affectedBlocks: number;
  block: string;
  envelopeChanged: false;
}
