import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/schematic-generation-entry.ts`;
const bundle = `${smokeDir}/schematic-generation-bundle.mjs`;
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
  encodeVarintBlockData,
  writeSpongeV2Schematic
} from "../../src/services/spongeSchematic";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const adapter = new MinimalArnisAdapter([putuoLibraryFixtureProvider]);
const geometry = await adapter.getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const schematic = generateSchematicFromBuildingGeometry(geometry);
const bytes = writeSpongeV2Schematic(schematic);

assert(schematic.width >= 70, "expected library footprint width to be derived from geometry");
assert(schematic.length >= 60, "expected library footprint length to be derived from geometry");
assert(schematic.height >= 25, "expected height to include wall height and roof");
assert(schematic.blockData.length === schematic.width * schematic.height * schematic.length, "blockData shape mismatch");
assert(schematic.palette.includes("minecraft:stone_bricks"), "missing wall block palette entry");
assert(schematic.palette.includes("minecraft:dark_oak_slab"), "missing roof block palette entry");
assert(countBlocks(schematic, schematicPaletteIndexes.WALL) > 0, "expected wall blocks");
assert(countBlocks(schematic, schematicPaletteIndexes.ROOF) > 0, "expected roof blocks");
assert(schematic.metadata.nonRectangularFootprint, "expected non-rectangular footprint marker");
assert(schematic.metadata.roofShape === "hipped", "expected roof hint to affect generated model");
assert(encodeVarintBlockData(schematic.blockData).length > schematic.blockData.length * 0.8, "expected Sponge block data payload");
assert(bytes[0] === 10, "expected root TAG_Compound");
assert(new TextDecoder().decode(bytes).includes("Schematic"), "expected Sponge root name");
assert(new TextDecoder().decode(bytes).includes("PaletteMax"), "expected Sponge v2 PaletteMax tag");
assert(new TextDecoder().decode(bytes).includes("BlockData"), "expected Sponge v2 BlockData tag");
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
console.log("Schematic generation smoke test passed.");
