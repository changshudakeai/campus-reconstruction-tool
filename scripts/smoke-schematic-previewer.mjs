import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const previewerSource = readFileSync("src/components/SchematicPreviewer.tsx", "utf8");
const previewerMarkers = [
  "OrbitControls",
  "InstancedMesh",
  "Raycaster",
  "onInspectBlock",
  "listInspectableBlocks"
];

for (const marker of previewerMarkers) {
  if (!previewerSource.includes(marker)) {
    throw new Error(`Missing previewer marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/schematic-previewer-entry.ts`;
const bundle = `${smokeDir}/schematic-previewer-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { countBlocks } from "../../src/domain/schematicModel";
import {
  generateSchematicFromBuildingGeometry,
  schematicPaletteIndexes
} from "../../src/services/buildingGeometryToSchematic";
import {
  inspectBlock,
  listInspectableBlocks,
  replaceAllMatchingBlocks
} from "../../src/services/schematicEditing";
import { writeSpongeV2Schematic } from "../../src/services/spongeSchematic";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const adapter = new MinimalArnisAdapter([putuoLibraryFixtureProvider]);
const geometry = await adapter.getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const model = generateSchematicFromBuildingGeometry(geometry);
const visibleBlocks = listInspectableBlocks(model);
const firstWall = visibleBlocks.find((block) => block.block === "minecraft:stone_bricks");

assert(visibleBlocks.length > 0, "expected preview loading data to include visible blocks");
assert(firstWall, "expected a stone brick wall block to inspect");

const inspected = inspectBlock(model, firstWall.x, firstWall.y, firstWall.z);
assert(inspected?.block === "minecraft:stone_bricks", "expected block inspection lookup to find wall block");

const oldWallCount = countBlocks(model, schematicPaletteIndexes.WALL);
const result = replaceAllMatchingBlocks(
  model,
  "minecraft:stone_bricks",
  "minecraft:mossy_stone_bricks"
);
const replacementIndex = result.model.palette.indexOf("minecraft:mossy_stone_bricks");

assert(result.replacedCount === oldWallCount, "expected batch replacement to update every wall block");
assert(replacementIndex >= 0, "expected replacement block to be added to palette");
assert(countBlocks(result.model, schematicPaletteIndexes.WALL) === 0, "expected original wall palette entries to be replaced");
assert(countBlocks(result.model, replacementIndex) === oldWallCount, "expected replacement palette entries to match old wall count");

const bytes = writeSpongeV2Schematic(result.model);
const decoded = new TextDecoder().decode(bytes);
assert(bytes.length > 0, "expected export bytes after replacement");
assert(decoded.includes("minecraft:mossy_stone_bricks"), "expected exported Sponge palette to include replacement block");
  `.trim()
);

await build({
  entryPoints: [entryPath],
  outfile: bundlePath,
  bundle: true,
  platform: "node",
  format: "esm",
  sourcemap: false,
  logLevel: "silent"
});

await import(`${pathToFileURL(bundlePath).href}?t=${Date.now()}`);
console.log("Schematic previewer smoke test passed.");
