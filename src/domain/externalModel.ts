export type ExternalModelSource = "3dmr" | "wikidata" | "manual_reference";
export type ExternalModelLicenseEligibility = "eligible" | "review_only" | "blocked";
export type ExternalModelReviewDecision = "pending" | "eligible_primary" | "supporting_evidence" | "rejected";

export interface ExternalModelLicense {
  name: string;
  url?: string;
  allowsAdaptation?: boolean;
  requiresAttribution?: boolean;
  requiresShareAlike?: boolean;
  allowsCommercialUse?: boolean;
  noDerivatives?: boolean;
}

export interface ExternalModelCandidate {
  id: string;
  source: ExternalModelSource;
  title: string;
  sourceUrl: string;
  author: string;
  license: ExternalModelLicense | null;
  linkedFeatureId?: string;
  wikidataId?: string;
  dimensionsMeters?: {
    width?: number;
    height?: number;
    length?: number;
  };
  notes?: string[];
}

export interface ExternalModelLicenseReview {
  eligibility: ExternalModelLicenseEligibility;
  reasons: string[];
  obligations: string[];
}

export interface ExternalModelProvenance {
  candidate: ExternalModelCandidate;
  licenseReview: ExternalModelLicenseReview;
  decision: ExternalModelReviewDecision;
  decisionReason: string;
  reviewedAt: string;
  adaptationNotice: string;
  attribution: string;
}
