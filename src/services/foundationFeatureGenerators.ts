import type { MapFeatureKind } from "../domain/foundationManifest";

export type FoundationGeneratorId = "arnis:road/v1" | "arnis:vegetation/v1" | "arnis:water/v1" | "arnis:sports/v1" | "core:solid-fill/v1";

export interface FoundationGeneratorStyle {
  generator: FoundationGeneratorId;
  blocks: string[];
  width?: number;
  density?: number;
  seed?: number;
}

export interface FoundationStylePack {
  schemaVersion: "1.0";
  id: string;
  name: string;
  features: Partial<Record<MapFeatureKind, FoundationGeneratorStyle>>;
}

export const ARNIS_CLASSIC_FOUNDATION_STYLE_PACK: FoundationStylePack = {
  schemaVersion: "1.0",
  id: "arnis:classic/v1",
  name: "Arnis Classic",
  features: {
    campus: { generator: "core:solid-fill/v1", blocks: ["minecraft:grass_block"] },
    building: { generator: "core:solid-fill/v1", blocks: ["minecraft:quartz_block"] },
    road: { generator: "arnis:road/v1", blocks: ["minecraft:gray_concrete", "minecraft:stone_bricks"] },
    vegetation: { generator: "arnis:vegetation/v1", blocks: ["minecraft:moss_block", "minecraft:oak_log", "minecraft:oak_leaves"], density: 0.035, seed: 104729 },
    water: { generator: "arnis:water/v1", blocks: ["minecraft:water", "minecraft:sand"] },
    sports: { generator: "arnis:sports/v1", blocks: ["minecraft:green_concrete", "minecraft:white_concrete"] }
  }
};

export const MODERN_CAMPUS_FOUNDATION_STYLE_PACK: FoundationStylePack = {
  schemaVersion: "1.0", id: "arnis:modern-campus/v1", name: "Modern Campus",
  features: {
    campus: { generator: "core:solid-fill/v1", blocks: ["minecraft:grass_block"] },
    building: { generator: "core:solid-fill/v1", blocks: ["minecraft:smooth_quartz"] },
    road: { generator: "arnis:road/v1", blocks: ["minecraft:light_gray_concrete", "minecraft:smooth_stone"], width: 4 },
    vegetation: { generator: "arnis:vegetation/v1", blocks: ["minecraft:moss_block", "minecraft:birch_log", "minecraft:birch_leaves"], density: 0.025, seed: 104729 },
    water: { generator: "arnis:water/v1", blocks: ["minecraft:water", "minecraft:smooth_sandstone"] },
    sports: { generator: "arnis:sports/v1", blocks: ["minecraft:green_concrete", "minecraft:white_concrete"] }
  }
};

export const HISTORIC_RED_BRICK_FOUNDATION_STYLE_PACK: FoundationStylePack = {
  schemaVersion: "1.0", id: "arnis:historic-red-brick/v1", name: "Historic Red-Brick Campus",
  features: {
    campus: { generator: "core:solid-fill/v1", blocks: ["minecraft:grass_block"] },
    building: { generator: "core:solid-fill/v1", blocks: ["minecraft:bricks"] },
    road: { generator: "arnis:road/v1", blocks: ["minecraft:stone_bricks", "minecraft:cobblestone"], width: 3 },
    vegetation: { generator: "arnis:vegetation/v1", blocks: ["minecraft:dark_oak_leaves", "minecraft:dark_oak_log", "minecraft:dark_oak_leaves"], density: 0.045, seed: 104729 },
    water: { generator: "arnis:water/v1", blocks: ["minecraft:water", "minecraft:mud_bricks"] },
    sports: { generator: "arnis:sports/v1", blocks: ["minecraft:terracotta", "minecraft:white_concrete"] }
  }
};

export const LIGHTWEIGHT_DRAFT_FOUNDATION_STYLE_PACK: FoundationStylePack = {
  schemaVersion: "1.0", id: "arnis:lightweight-draft/v1", name: "Lightweight Draft",
  features: {
    campus: { generator: "core:solid-fill/v1", blocks: ["minecraft:grass_block"] },
    building: { generator: "core:solid-fill/v1", blocks: ["minecraft:stone"] },
    road: { generator: "core:solid-fill/v1", blocks: ["minecraft:gray_concrete"], width: 2 },
    vegetation: { generator: "core:solid-fill/v1", blocks: ["minecraft:moss_block"], density: 0.01, seed: 104729 },
    water: { generator: "core:solid-fill/v1", blocks: ["minecraft:water"] },
    sports: { generator: "core:solid-fill/v1", blocks: ["minecraft:green_concrete"] }
  }
};

export const FOUNDATION_STYLE_PRESETS = [
  ARNIS_CLASSIC_FOUNDATION_STYLE_PACK,
  MODERN_CAMPUS_FOUNDATION_STYLE_PACK,
  HISTORIC_RED_BRICK_FOUNDATION_STYLE_PACK,
  LIGHTWEIGHT_DRAFT_FOUNDATION_STYLE_PACK
] as const;

export const DEFAULT_ARNIS_FOUNDATION_STYLE_PACK = ARNIS_CLASSIC_FOUNDATION_STYLE_PACK;

const GENERATORS = new Set<FoundationGeneratorId>(["arnis:road/v1", "arnis:vegetation/v1", "arnis:water/v1", "arnis:sports/v1", "core:solid-fill/v1"]);

export function parseFoundationStylePack(json: string): FoundationStylePack {
  const value = JSON.parse(json) as Partial<FoundationStylePack>;
  if (value.schemaVersion !== "1.0" || !value.id || !value.name || !value.features) throw new Error("Invalid Foundation Style Pack header.");
  for (const style of Object.values(value.features)) {
    if (!style || !GENERATORS.has(style.generator) || !Array.isArray(style.blocks) || !style.blocks.length) throw new Error("Invalid or unregistered Foundation Feature Generator.");
  }
  return value as FoundationStylePack;
}

export function stylePackBlocks(pack: FoundationStylePack) {
  return Object.values(pack.features).flatMap((style) => style?.blocks ?? []).map((block) => block.includes(":") ? block : `minecraft:${block}`);
}
