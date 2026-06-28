import type {
  BuildingGeometry,
  BuildingGeometryObservation,
  BuildingIdentityMatch,
  BuildingIdentityResolution,
  BuildingTarget,
  FootprintComponent,
  ObservationReviewStatus
} from "../domain/buildingGeometry";
import {
  approximateFootprintIoU,
  createBuildingGeometryObservation
} from "./buildingObservation";

const AMBIGUITY_MARGIN = 0.08;
const MAX_PLAUSIBLE_DISTANCE_M = 300;

export function resolveBuildingIdentity(
  target: BuildingTarget,
  observations: BuildingGeometryObservation[]
): BuildingIdentityResolution {
  const matches = observations
    .map((observation) => scoreObservationIdentity(target, observation))
    .sort((left, right) => right.score - left.score);
  const automatic = matches.filter((match) => {
    const observation = observations.find((item) => item.id === match.observationId);
    return observation?.source !== "existing_project" && match.confidence !== "rejected";
  });
  const best = automatic[0] ?? null;
  const runnerUp = automatic[1] ?? null;
  const ambiguous = Boolean(
    best && runnerUp && best.score >= 0.45 && best.score - runnerUp.score < AMBIGUITY_MARGIN
  );
  const ambiguousIds = new Set(ambiguous ? [best?.observationId, runnerUp?.observationId] : []);

  return {
    targetSlotId: target.reviewedSlot?.id ?? null,
    selectedObservationId: best && !ambiguous ? best.observationId : null,
    ambiguous,
    matches: matches.map((match) => ({
      ...match,
      reviewRequired:
        match.reviewRequired || ambiguousIds.has(match.observationId)
    }))
  };
}

export function scoreObservationIdentity(
  target: BuildingTarget,
  observation: BuildingGeometryObservation
): BuildingIdentityMatch {
  const anchor = anchorComponents(target);
  const overlap = anchor.length ? approximateFootprintIoU(anchor, observation.components) : 0;
  const distanceM = metersBetween(target.approximateCenter, observation.metrics.center);
  const nameScore = scoreName(target, observation);
  const dimensionScore = scoreDimensions(target, observation);
  const distanceScore = Math.max(0, 1 - distanceM / MAX_PLAUSIBLE_DISTANCE_M);
  const implausibleDistance = distanceM > MAX_PLAUSIBLE_DISTANCE_M;
  const implausibleDimensions = dimensionScore < 0.2 && observation.metrics.areaSquareMeters > 0;
  const rejected = implausibleDistance || implausibleDimensions || observation.components.length === 0;
  const score = rejected
    ? 0
    : clampScore(overlap * 0.5 + nameScore * 0.2 + distanceScore * 0.15 + dimensionScore * 0.15);
  const confidence = rejected
    ? "rejected"
    : score >= 0.75 && overlap >= 0.4
      ? "high"
      : score >= 0.5
        ? "medium"
        : "low";

  return {
    observationId: observation.id,
    score,
    confidence,
    reviewRequired: confidence !== "high" && observation.source !== "existing_project",
    reasons: [
      {
        criterion: "overlap",
        score: overlap,
        message: anchor.length
          ? `Footprint overlap with reviewed Building Slot: ${(overlap * 100).toFixed(1)}%.`
          : "No reviewed Building Slot footprint was available."
      },
      {
        criterion: "distance",
        score: distanceScore,
        message: `Center distance from reviewed Building Slot: ${distanceM.toFixed(1)} m.`
      },
      {
        criterion: "name",
        score: nameScore,
        message: nameScore > 0
          ? "Provider name matches a Chinese or English library alias."
          : "Provider name does not match a known library alias."
      },
      {
        criterion: "dimensions",
        score: dimensionScore,
        message: `Dimension agreement with reviewed Building Slot: ${(dimensionScore * 100).toFixed(1)}%.`
      },
      {
        criterion: "plausibility",
        score: rejected ? 0 : 1,
        message: rejected
          ? `Rejected as implausible (${implausibleDistance ? "excessive distance" : "dimension mismatch"}).`
          : "Candidate passes distance and dimension plausibility thresholds."
      }
    ]
  };
}

export function applyObservationReviewDecision(
  geometry: BuildingGeometry,
  observationId: string,
  status: ObservationReviewStatus
): BuildingGeometry {
  if (!geometry.provenance.observations.some((observation) => observation.id === observationId)) {
    throw new Error(`Unknown Building Geometry Observation: ${observationId}`);
  }
  const next = structuredClone(geometry);
  next.provenance.observationReviews[observationId] = status;
  const accepted = next.provenance.identityResolution.matches
    .filter((match) => next.provenance.observationReviews[match.observationId] === "accepted")
    .sort((left, right) => right.score - left.score);
  next.provenance.identityResolution.selectedObservationId = accepted[0]?.observationId ??
    next.provenance.identityResolution.selectedObservationId;
  if (status === "rejected" && next.provenance.identityResolution.selectedObservationId === observationId) {
    next.provenance.identityResolution.selectedObservationId = accepted[0]?.observationId ?? null;
  }
  next.provenance.notes.push(`Observation ${observationId} marked ${status} during identity review.`);
  return next;
}

function anchorComponents(target: BuildingTarget): FootprintComponent[] {
  const footprint = target.reviewedSlot?.footprint ?? [];
  return footprint.length >= 3 ? [{ exterior: footprint, interiorRings: [] }] : [];
}

function scoreName(target: BuildingTarget, observation: BuildingGeometryObservation) {
  const aliases = [target.name, ...target.aliases]
    .map(normalizeName)
    .filter(Boolean);
  const names = [
    observation.name,
    observation.tags.name,
    observation.tags["name:zh"],
    observation.tags["name:en"]
  ].map((value) => normalizeName(value ?? "")).filter(Boolean);
  if (names.some((name) => aliases.includes(name))) return 1;
  if (names.some((name) => aliases.some((alias) => name.includes(alias) || alias.includes(name)))) return 0.75;
  return 0;
}

function scoreDimensions(target: BuildingTarget, observation: BuildingGeometryObservation) {
  const slot = target.reviewedSlot;
  if (!slot) return 0.5;
  const expected = [slot.approximateWidthMeters, slot.approximateLengthMeters].sort((a, b) => a - b);
  const actual = [observation.metrics.widthMeters, observation.metrics.lengthMeters].sort((a, b) => a - b);
  if (actual[0] <= 0 || actual[1] <= 0) return 0;
  return (
    Math.min(expected[0], actual[0]) / Math.max(expected[0], actual[0]) +
    Math.min(expected[1], actual[1]) / Math.max(expected[1], actual[1])
  ) / 2;
}

function metersBetween(left: { lng: number; lat: number }, right: { lng: number; lat: number }) {
  const latRadians = ((left.lat + right.lat) / 2) * Math.PI / 180;
  const x = (left.lng - right.lng) * 111_320 * Math.cos(latRadians);
  const y = (left.lat - right.lat) * 111_320;
  return Math.sqrt(x ** 2 + y ** 2);
}

function normalizeName(value: string) {
  return value
    .normalize("NFKC")
    .toLocaleLowerCase()
    .replace(/[\s\p{P}\p{S}]+/gu, "");
}

function clampScore(value: number) {
  return Math.max(0, Math.min(1, value));
}

export function reviewedSlotObservation(target: BuildingTarget) {
  const slot = target.reviewedSlot;
  if (!slot) return null;
  return createBuildingGeometryObservation({
    id: `manifest:${slot.id}`,
    source: "existing_project",
    sourceFeatureId: slot.id,
    name: target.name,
    components: [{ exterior: slot.footprint, interiorRings: [] }],
    normalizationNotes: ["Reviewed Foundation Manifest Building Slot used as identity anchor."]
  });
}
