import type { FoundationManifest, GeometryDimensions } from "../domain/foundationManifest";

export interface FoundationManifestExport {
  fileName: string;
  json: string;
  featureCount: number;
  slotCount: number;
}

export function exportFoundationManifestJson(
  manifest: FoundationManifest,
  fileName = "ecnu_putuo_foundation_manifest.json"
): FoundationManifestExport {
  return {
    fileName,
    json: `${JSON.stringify(manifest, null, 2)}\n`,
    featureCount: manifest.mapFeatures.length,
    slotCount: manifest.buildingSlots.length
  };
}

export function parseFoundationManifestJson(json: string): FoundationManifest {
  const manifest = JSON.parse(json) as FoundationManifest;

  if (manifest.schemaVersion !== "0.1.0") {
    throw new Error("Unsupported Foundation Manifest schema version.");
  }

  if (!Array.isArray(manifest.mapFeatures) || !Array.isArray(manifest.buildingSlots)) {
    throw new Error("Invalid Foundation Manifest handoff shape.");
  }

  if (
    manifest.representativeBuildingSlotId &&
    !manifest.buildingSlots.some((slot) => slot.id === manifest.representativeBuildingSlotId)
  ) {
    throw new Error("Invalid Foundation Manifest representative Building Slot.");
  }

  for (const feature of manifest.mapFeatures) {
    if (!feature.block || !feature.geometry?.points?.length || !feature.provenance?.rawId) {
      throw new Error("Invalid Foundation Manifest Map Feature handoff contract.");
    }

    assertGeometryDimensions(feature.dimensions);
  }

  for (const slot of manifest.buildingSlots) {
    if (
      !slot.selectedBlock ||
      !slot.geometry?.points?.length ||
      !slot.provenance?.rawId ||
      !slot.confidence
    ) {
      throw new Error("Invalid Foundation Manifest Building Slot handoff contract.");
    }

    assertGeometryDimensions(slot.dimensions);
  }

  return manifest;
}

function assertGeometryDimensions(dimensions: GeometryDimensions | undefined) {
  if (
    !dimensions ||
    dimensions.pointCount < 1 ||
    !Number.isFinite(dimensions.approximateWidthMeters) ||
    !Number.isFinite(dimensions.approximateLengthMeters) ||
    !Number.isFinite(dimensions.bounds.minLng) ||
    !Number.isFinite(dimensions.bounds.minLat) ||
    !Number.isFinite(dimensions.bounds.maxLng) ||
    !Number.isFinite(dimensions.bounds.maxLat)
  ) {
    throw new Error("Invalid Foundation Manifest geometry dimensions.");
  }
}
