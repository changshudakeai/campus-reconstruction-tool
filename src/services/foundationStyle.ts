import type {
  FoundationManifest,
  MapFeatureKind
} from "../domain/foundationManifest";

export type FeatureBlockStyles = Record<MapFeatureKind, string>;

export interface FoundationStyleSettings {
  roadWidthBlocks: number;
  blocks: FeatureBlockStyles;
}

export const FEATURE_KINDS: MapFeatureKind[] = [
  "campus",
  "building",
  "road",
  "vegetation",
  "water",
  "sports"
];

export const DEFAULT_FOUNDATION_STYLE: FoundationStyleSettings = {
  roadWidthBlocks: 4,
  blocks: {
    campus: "grass_block",
    building: "quartz_block",
    road: "gray_concrete",
    vegetation: "moss_block",
    water: "water",
    sports: "orange_concrete"
  }
};

export const FEATURE_BLOCK_OPTIONS: Record<MapFeatureKind, string[]> = {
  campus: ["grass_block", "moss_block", "stone", "smooth_stone"],
  building: ["quartz_block", "stone_bricks", "bricks", "sandstone"],
  road: ["gray_concrete", "stone", "deepslate", "black_concrete"],
  vegetation: ["moss_block", "oak_leaves", "grass_block", "green_wool"],
  water: ["water", "blue_stained_glass", "blue_concrete", "prismarine"],
  sports: ["orange_concrete", "green_concrete", "red_concrete", "blue_concrete"]
};

export function applyFoundationStyle(
  manifest: FoundationManifest,
  style: FoundationStyleSettings
): FoundationManifest {
  const mapFeatures = manifest.mapFeatures.map((feature) => ({
    ...feature,
    block: style.blocks[feature.kind] ?? feature.block
  }));

  return {
    ...manifest,
    mapFeatures,
    buildingSlots: manifest.buildingSlots.map((slot) => {
      const sourceFeature = mapFeatures.find((feature) => feature.id === slot.sourceFeatureId);

      return {
        ...slot,
        selectedBlock: sourceFeature?.block ?? slot.selectedBlock,
        geometry: sourceFeature?.geometry ?? slot.geometry,
        dimensions: sourceFeature?.dimensions ?? slot.dimensions
      };
    })
  };
}

export function updateFeatureBlockStyle(
  style: FoundationStyleSettings,
  kind: MapFeatureKind,
  block: string
): FoundationStyleSettings {
  return {
    ...style,
    blocks: {
      ...style.blocks,
      [kind]: block
    }
  };
}

export function updateRoadWidthStyle(
  style: FoundationStyleSettings,
  roadWidthBlocks: number
): FoundationStyleSettings {
  return {
    ...style,
    roadWidthBlocks: Math.max(1, Math.min(16, Math.round(roadWidthBlocks)))
  };
}
