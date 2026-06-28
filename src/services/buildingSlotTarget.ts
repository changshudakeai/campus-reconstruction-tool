import type { BuildingSlot } from "../domain/foundationManifest";
import type { BuildingTarget } from "../domain/buildingGeometry";

export function buildingSlotToBuildingTarget(slot: BuildingSlot): BuildingTarget {
  const bounds = slot.dimensions.bounds;

  return {
    name: slot.name,
    campus: "ECNU Putuo Campus",
    aliases: Array.from(
      new Set([
        slot.name,
        slot.provenance.query,
        slot.provenance.rawId,
        "Putuo Campus Library",
        "ECNU Putuo Library",
        "图书馆",
        "华东师范大学普陀校区图书馆",
        "图书馆"
      ].filter(Boolean))
    ),
    approximateCenter: {
      lng: (bounds.minLng + bounds.maxLng) / 2,
      lat: (bounds.minLat + bounds.maxLat) / 2
    },
    reviewedSlot: {
      id: slot.id,
      footprint: slot.geometry.points,
      approximateWidthMeters: slot.dimensions.approximateWidthMeters,
      approximateLengthMeters: slot.dimensions.approximateLengthMeters
    }
  };
}
