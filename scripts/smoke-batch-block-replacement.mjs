import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const editing = readFileSync("src/services/schematicEditing.ts", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "countMatchingBlocks",
  "matchingBlockCount",
  "matchingBlocks",
  'sourceBlock === "minecraft:air"',
  "sourceBlock === replacementBlock"
]) {
  if (!app.includes(marker) && !editing.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing batch replacement marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/batch-block-replacement-entry.ts`;
const bundle = `${smokeDir}/batch-block-replacement-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { countMatchingBlocks, replaceAllMatchingBlocks } from "../../src/services/schematicEditing";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider])
  .getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const model = generateSchematicFromBuildingGeometry(geometry);
const source = "minecraft:stone_bricks" as const;
const replacement = "minecraft:bricks" as const;
const sourceIndex = model.palette.indexOf(source);
const originalData = new Uint16Array(model.blockData);
const matching = countMatchingBlocks(model, source);
const result = replaceAllMatchingBlocks(model, source, replacement);

assert(matching > 0, "expected matching wall blocks");
assert(result.replacedCount === matching, "expected count to match actual replacements");
assert(result.model !== model, "expected immutable model result");
assert(result.model.blockData !== model.blockData, "expected cloned block data");
assert(model.blockData.every((value, index) => value === originalData[index]), "expected source model unchanged");
assert(countMatchingBlocks(result.model, source) === 0, "expected all source blocks replaced");
assert(countMatchingBlocks(result.model, replacement) === matching, "expected replacement count");
assert(result.model.palette.includes(replacement), "expected replacement palette entry");
assert(sourceIndex >= 0, "expected source palette entry");

const sameType = replaceAllMatchingBlocks(model, source, source);
assert(sameType.replacedCount === 0, "expected same-type no-op");
assert(sameType.model.blockData !== model.blockData, "expected safe clone for same-type no-op");

const airSource = replaceAllMatchingBlocks(model, "minecraft:air", replacement);
assert(airSource.replacedCount === 0, "expected air source protection");
assert(countMatchingBlocks(model, "minecraft:air") === 0, "expected air excluded from match count");

const absent = replaceAllMatchingBlocks(model, "minecraft:diamond_block", replacement);
assert(absent.replacedCount === 0, "expected absent source no-op");
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
console.log("Batch Block Replacement smoke test passed.");
