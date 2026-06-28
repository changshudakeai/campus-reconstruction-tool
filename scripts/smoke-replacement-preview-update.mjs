import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const previewer = readFileSync("src/components/SchematicPreviewer.tsx", "utf8");

for (const marker of [
  "setSchematicModel(result.model)",
  "setSelectedBlock(null)",
  "model={schematicModel}",
  "}, [model]);",
  'className="replacement-result" role="status"'
]) {
  if (!app.includes(marker) && !previewer.includes(marker)) {
    throw new Error(`Missing replacement preview update marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/replacement-preview-update-entry.ts`;
const bundle = `${smokeDir}/replacement-preview-update-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { listInspectableBlocks, replaceAllMatchingBlocks } from "../../src/services/schematicEditing";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider])
  .getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const originalModel = generateSchematicFromBuildingGeometry(geometry);
const originalVisible = listInspectableBlocks(originalModel);
const originalWalls = originalVisible.filter((block) => block.block === "minecraft:stone_bricks");
assert(originalWalls.length > 0, "expected original wall instances");

const replacement = replaceAllMatchingBlocks(
  originalModel,
  "minecraft:stone_bricks",
  "minecraft:mossy_stone_bricks"
);
const updatedVisible = listInspectableBlocks(replacement.model);
const oldWallInstances = updatedVisible.filter((block) => block.block === "minecraft:stone_bricks");
const newWallInstances = updatedVisible.filter((block) => block.block === "minecraft:mossy_stone_bricks");

assert(replacement.model !== originalModel, "expected new model identity to trigger React preview effect");
assert(oldWallInstances.length === 0, "expected old material absent from updated preview instances");
assert(newWallInstances.length === originalWalls.length, "expected all replacement instances in updated preview");
assert(updatedVisible.length === originalVisible.length, "expected material replacement to preserve visible volume");
assert(replacement.replacedCount === newWallInstances.length, "expected status count to match preview instances");
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
console.log("Batch replacement preview update smoke test passed.");
