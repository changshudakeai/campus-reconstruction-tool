import type { MinecraftBlockName } from "../domain/schematicModel";

export interface MinecraftBlockCatalogEntry {
  id: MinecraftBlockName;
  label: string;
  icon: string | null;
}

interface MinecraftBlockCatalogPayload {
  version: string;
  count: number;
  blocks: MinecraftBlockCatalogEntry[];
}

export const MINECRAFT_ASSET_VERSION = "26.2";
let catalogPromise: Promise<MinecraftBlockCatalogEntry[]> | null = null;

export function loadMinecraftBlockCatalog() {
  catalogPromise ??= fetch(`/minecraft/${MINECRAFT_ASSET_VERSION}/blocks.json`)
    .then(async (response) => {
      if (!response.ok) throw new Error(`Minecraft block catalog returned HTTP ${response.status}`);
      const payload = await response.json() as MinecraftBlockCatalogPayload;
      return payload.blocks.filter((block) => !/[\[]/.test(block.id));
    });
  return catalogPromise;
}

export function minecraftBlockIcon(block: string) {
  const id = block.replace(/^minecraft:/, "").replace(/\[.*$/, "");
  return `/minecraft/${MINECRAFT_ASSET_VERSION}/block-icons/${encodeURIComponent(id)}.png`;
}

export function minecraftBlockLabel(block: string) {
  return block.replace(/^minecraft:/, "").replace(/_/g, " ");
}

export function minecraftBlockTint(block: string) {
  const id = block.replace(/^minecraft:/, "").replace(/\[.*$/, "");
  if (id === "grass_block" || /(?:^|_)grass$/.test(id)) return "#79c05a";
  if (/leaves$/.test(id)) return "#63a948";
  if (id === "water") return "#3f76e4";
  return null;
}
