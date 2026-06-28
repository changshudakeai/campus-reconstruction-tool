import type {
  ExternalModelCandidate,
  ExternalModelLicenseReview,
  ExternalModelProvenance,
  ExternalModelReviewDecision
} from "../domain/externalModel";
import type { SchematicModel } from "../domain/schematicModel";
import { cloneSchematicProvenance } from "../domain/schematicModel";

export function classifyExternalModelLicense(candidate: ExternalModelCandidate): ExternalModelLicenseReview {
  const license = candidate.license;
  const reasons: string[] = [];
  const obligations: string[] = [];

  if (!license) {
    return {
      eligibility: "blocked",
      reasons: ["External model license is missing."],
      obligations: []
    };
  }

  const normalizedName = license.name.trim();
  if (!normalizedName) reasons.push("External model license name is empty.");
  if (!license.url) reasons.push("External model license URL is missing or unclear.");
  if (license.noDerivatives || /(^|[-\s])ND($|[-\s])/i.test(normalizedName)) {
    reasons.push("No-derivatives terms block adaptation into a schematic.");
  }
  if (license.allowsAdaptation === false) {
    reasons.push("License does not permit adaptation.");
  }
  if (license.allowsAdaptation !== true) {
    reasons.push("Adaptation permission is not explicit.");
  }

  if (license.requiresAttribution) obligations.push("Retain author attribution in provenance and export notes.");
  if (license.requiresShareAlike) obligations.push("Retain share-alike obligation with the adapted schematic.");
  if (license.allowsCommercialUse === false) obligations.push("Restrict use to non-commercial project contexts.");

  const blocking = reasons.some((reason) =>
    reason.includes("No-derivatives") ||
    reason.includes("does not permit") ||
    reason.includes("missing") ||
    reason.includes("empty") ||
    reason.includes("not explicit")
  );

  return {
    eligibility: blocking ? "blocked" : "eligible",
    reasons: reasons.length ? reasons : ["License terms permit adaptation for this project."],
    obligations
  };
}

export function externalModelAttribution(candidate: ExternalModelCandidate) {
  const license = candidate.license;
  return [
    candidate.title,
    candidate.author ? `by ${candidate.author}` : "author unknown",
    license?.name ? `licensed ${license.name}` : "license missing",
    candidate.sourceUrl
  ].filter(Boolean).join(" · ");
}

export function recordExternalModelReview(
  model: SchematicModel,
  candidate: ExternalModelCandidate,
  decision: ExternalModelReviewDecision,
  decisionReason: string,
  reviewedAt = new Date().toISOString()
): SchematicModel {
  const reason = decisionReason.trim();
  if (!reason) throw new Error("External model review requires a decision reason.");

  const licenseReview = classifyExternalModelLicense(candidate);
  if (decision === "eligible_primary" && licenseReview.eligibility !== "eligible") {
    throw new Error("Only clearly eligible external models can become primary geometry.");
  }
  if (decision === "eligible_primary" && candidate.license?.requiresAttribution && !candidate.author.trim()) {
    throw new Error("Attribution-required external models need an author before use.");
  }

  const provenance = cloneSchematicProvenance(model.metadata.provenance);
  if (!provenance) throw new Error("External model review requires schematic provenance.");

  const externalModel: ExternalModelProvenance = {
    candidate: structuredClone(candidate),
    licenseReview,
    decision,
    decisionReason: reason,
    reviewedAt,
    adaptationNotice: decision === "eligible_primary"
      ? "External model may seed geometry only under the retained license obligations; material source conflicts still require human review."
      : "External model retained as review evidence only.",
    attribution: externalModelAttribution(candidate)
  };

  provenance.externalModels = [
    ...(provenance.externalModels ?? []).filter((item) => item.candidate.id !== candidate.id),
    externalModel
  ];

  return { ...model, metadata: { ...model.metadata, provenance } };
}
