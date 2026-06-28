import {
  BuildingSlot,
  chooseRepresentativeBuildingSlot,
  FoundationManifest,
  isPutuoLibraryName,
  MapFeature,
  MapFeatureKind,
  summarizeGeometry
} from "../domain/foundationManifest";
import { CandidateGeometry, CandidateProvenance, MapCandidate } from "../domain/mapCandidate";

export type CandidateReviewStatus = "pending" | "accepted" | "rejected" | "merged";

export interface ReviewedCandidate {
  candidate: MapCandidate;
  status: CandidateReviewStatus;
}

export interface ManualFeatureInput {
  id: string;
  name: string;
  kind: MapFeatureKind;
  block: string;
  geometry: CandidateGeometry;
  provenance: CandidateProvenance;
}

export function acceptCandidate(candidate: MapCandidate): ReviewedCandidate {
  return {
    candidate,
    status: "accepted"
  };
}

export function rejectCandidate(candidate: MapCandidate): ReviewedCandidate {
  return {
    candidate,
    status: "rejected"
  };
}

export function candidateToMapFeature(candidate: MapCandidate): MapFeature {
  return {
    id: `feature-${candidate.id}`,
    kind: candidate.kind,
    name: candidate.name,
    source: candidate.provenance.sourceLabel,
    confidence: candidate.confidence,
    block: defaultBlockForKind(candidate.kind),
    reviewed: true,
    geometry: candidate.geometry,
    dimensions: summarizeGeometry(candidate.geometry),
    provenance: candidate.provenance,
    replacementPolicy: candidate.kind === "building" ? "replace" : "foundation-only"
  };
}

export function manualInputToMapFeature(input: ManualFeatureInput): MapFeature {
  return {
    id: input.id,
    kind: input.kind,
    name: input.name,
    source: input.provenance.sourceLabel,
    confidence: "manual",
    block: input.block,
    reviewed: true,
    geometry: input.geometry,
    dimensions: summarizeGeometry(input.geometry),
    provenance: input.provenance,
    replacementPolicy: input.kind === "building" ? "replace" : "foundation-only"
  };
}

export function mapFeatureToBuildingSlot(feature: MapFeature): BuildingSlot | null {
  if (feature.kind !== "building") return null;

  return {
    id: `slot-${feature.id}`,
    name: feature.name,
    sourceFeatureId: feature.id,
    geometryRole: isPutuoLibraryName(feature.name)
      ? "representative-building"
      : "foundation-only",
    replacementPolicy: "replace",
    confidence: feature.confidence,
    selectedBlock: feature.block,
    geometry: feature.geometry,
    dimensions: feature.dimensions,
    provenance: feature.provenance
  };
}

export function buildFoundationManifestFromReviews(
  baseManifest: FoundationManifest,
  reviews: ReviewedCandidate[],
  manualFeatures: MapFeature[] = []
): FoundationManifest {
  const acceptedFeatures = reviews
    .filter((review) => review.status === "accepted")
    .map((review) => candidateToMapFeature(review.candidate));

  const mapFeatures = [...manualFeatures, ...acceptedFeatures];
  const buildingSlots = mapFeatures
    .map(mapFeatureToBuildingSlot)
    .filter((slot): slot is BuildingSlot => Boolean(slot));
  const representativeBuildingSlot = chooseRepresentativeBuildingSlot(buildingSlots);

  return {
    ...baseManifest,
    mapFeatures,
    buildingSlots,
    representativeBuildingSlotId: representativeBuildingSlot?.id ?? null
  };
}

export function makeManualPutuoBoundaryFeature(): MapFeature {
  return manualInputToMapFeature({
    id: "feature-manual-putuo-boundary",
    name: "Manual Putuo Campus boundary",
    kind: "campus",
    block: "grass_block",
    geometry: {
      type: "polygon",
      points: [
        { lng: 121.4048, lat: 31.2312 },
        { lng: 121.413, lat: 31.231 },
        { lng: 121.4138, lat: 31.2258 },
        { lng: 121.4071, lat: 31.2246 },
        { lng: 121.404, lat: 31.2272 }
      ]
    },
    provenance: {
      source: "manual_drawing",
      sourceLabel: "Manual drawing",
      query: "ECNU Putuo Campus manual boundary",
      rawId: "manual:putuo-boundary-demo",
      notes: ["User-created closed geometry placeholder for the first review path."]
    }
  });
}

function defaultBlockForKind(kind: MapFeatureKind) {
  return {
    campus: "grass_block",
    building: "quartz_block",
    road: "gray_concrete",
    vegetation: "moss_block",
    water: "water",
    sports: "orange_concrete"
  }[kind];
}
