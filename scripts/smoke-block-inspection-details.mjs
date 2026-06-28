import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const previewer = readFileSync("src/components/SchematicPreviewer.tsx", "utf8");
const editing = readFileSync("src/services/schematicEditing.ts", "utf8");

for (const marker of [
  "selectedBlock.block",
  "selectedBlock.x",
  "selectedBlock.y",
  "selectedBlock.z",
  "selectedBlock.paletteIndex",
  'aria-live="polite"',
  "Raycaster",
  "Number.isInteger(x)"
]) {
  if (!app.includes(marker) && !previewer.includes(marker) && !editing.includes(marker)) {
    throw new Error(`Missing block inspection marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/block-inspection-details-entry.ts`;
const bundle = `${smokeDir}/block-inspection-details-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { inspectBlock, listInspectableBlocks } from "../../src/services/schematicEditing";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider])
  .getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const model = generateSchematicFromBuildingGeometry(geometry);
const visibleBlocks = listInspectableBlocks(model);
const wall = visibleBlocks.find((block) => block.block === "minecraft:stone_bricks");
assert(Boolean(wall), "expected visible wall block");

const inspected = inspectBlock(model, wall!.x, wall!.y, wall!.z);
assert(Boolean(inspected), "expected selected block inspection");
assert(inspected!.block === "minecraft:stone_bricks", "expected block type");
assert(inspected!.paletteIndex === model.palette.indexOf("minecraft:stone_bricks"), "expected palette index");
assert(inspected!.x === wall!.x && inspected!.y === wall!.y && inspected!.z === wall!.z, "expected coordinates");
assert(visibleBlocks.every((block) => block.block !== "minecraft:air"), "expected only visible non-air blocks");

assert(inspectBlock(model, -1, 0, 0) === null, "expected negative coordinate rejection");
assert(inspectBlock(model, model.width, 0, 0) === null, "expected x overflow rejection");
assert(inspectBlock(model, 0, model.height, 0) === null, "expected y overflow rejection");
assert(inspectBlock(model, 0, 0, model.length) === null, "expected z overflow rejection");
assert(inspectBlock(model, 0.5, 0, 0) === null, "expected fractional coordinate rejection");
  `.trim()
);

await build({
  entryPoints: [resolve(entry)],
  outfile: resolve(bundle),
  bundle: true,
  platform: "node",
  format: "esm",
  sourcemap: false,
  logLevel: "silent"
});

await import(`${pathToFileURL(resolve(bundle)).href}?t=${Date.now()}`);
console.log("Block inspection details smoke test passed.");
