import type { BuildingSlot } from "../domain/foundationManifest";
import type { BuildingGeometryProvider } from "./minimalArnisAdapter";
import { createBuildingGeometryObservation } from "../services/buildingObservation";

export function createBuildingSlotHandoffProvider(slot: BuildingSlot): BuildingGeometryProvider {
  return {
    source: "existing_project",
    async fetchBuildingGeometry() {
      const components = [{ exterior: slot.geometry.points, interiorRings: [] }];
      const queryBounds = slot.dimensions.bounds;
      return {
        footprint: slot.geometry.points,
        confidence: {
          footprint: slot.confidence
        },
        handoff: {
          foundationSlotId: slot.id,
          sourceFeatureId: slot.sourceFeatureId,
          selectedBlock: slot.selectedBlock,
          rawId: slot.provenance.rawId,
          approximateWidthMeters: slot.dimensions.approximateWidthMeters,
          approximateLengthMeters: slot.dimensions.approximateLengthMeters
        },
        sourceRecords: [{
          source: "existing_project" as const,
          featureId: slot.provenance.rawId,
          releaseId: null,
          queryBounds,
          queryLimit: 1,
          components
        }],
        observations: [createBuildingGeometryObservation({
          id: `manifest:${slot.id}`,
          source: "existing_project",
          sourceFeatureId: slot.provenance.rawId,
          name: slot.name,
          tags: {
            selectedBlock: slot.selectedBlock,
            confidence: slot.confidence,
            sourceFeatureId: slot.sourceFeatureId
          },
          components,
          normalizationNotes: ["Reviewed Foundation Manifest Building Slot retained as user intent."]
        })],
        notes: [
          `Foundation Manifest handoff slot ${slot.id} selected block ${slot.selectedBlock}.`,
          `Foundation source feature ${slot.sourceFeatureId}; rawId ${slot.provenance.rawId}.`,
          ...slot.provenance.notes
        ]
      };
    }
  };
}
