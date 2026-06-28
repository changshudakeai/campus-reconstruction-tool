import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const exportService = readFileSync("src/services/detailedSchematicExport.ts", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "prepareDetailedSchematicExport",
  "exportResult.fileName",
  "exportResult.bytes",
  "detailedExportReady",
  'role="status"'
]) {
  if (!app.includes(marker) && !exportService.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing detailed schematic export marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/detailed-schematic-export-entry.ts`;
const bundle = `${smokeDir}/detailed-schematic-export-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { gunzipSync } from "node:zlib";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";
import { replaceAllMatchingBlocks } from "../../src/services/schematicEditing";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider])
  .getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const generated = generateSchematicFromBuildingGeometry(geometry);
const replacement = replaceAllMatchingBlocks(
  generated,
  "minecraft:stone_bricks",
  "minecraft:mossy_stone_bricks"
);
const result = prepareDetailedSchematicExport(replacement.model);
const uncompressed = gunzipSync(result.bytes);
const decoded = new TextDecoder().decode(uncompressed);

assert(result.fileName === "putuo_campus_library.schem", "expected stable schem filename");
assert(result.bytes[0] === 0x1f && result.bytes[1] === 0x8b, "expected gzip header");
assert(uncompressed[0] === 10, "expected root TAG_Compound after gunzip");
assert(decoded.includes("Schematic"), "expected Sponge root name");
assert(decoded.includes("Version"), "expected Sponge version tag");
assert(decoded.includes("PaletteMax"), "expected Sponge palette metadata");
assert(decoded.includes("BlockData"), "expected Sponge block data");
assert(decoded.includes("minecraft:mossy_stone_bricks"), "expected updated replacement material");
assert(result.width === replacement.model.width, "expected exported width");
assert(result.height === replacement.model.height, "expected exported height");
assert(result.length === replacement.model.length, "expected exported length");
assert(result.paletteSize === replacement.model.palette.length, "expected exported palette size");
assert(result.nonAirBlocks > 0, "expected non-air export blocks");
assert(result.bytes.length > 100, "expected non-empty NBT payload");

let rejectedWrongGenerator = false;
try {
  prepareDetailedSchematicExport({
    ...replacement.model,
    metadata: { ...replacement.model.metadata, generator: "foundation-manifest-to-schematic" }
  });
} catch {
  rejectedWrongGenerator = true;
}
assert(rejectedWrongGenerator, "expected Foundation model rejection");

let rejectedBadBlockData = false;
try {
  prepareDetailedSchematicExport({ ...replacement.model, blockData: new Uint16Array(1) });
} catch {
  rejectedBadBlockData = true;
}
assert(rejectedBadBlockData, "expected malformed block data rejection");
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
console.log("Detailed Axiom-compatible schematic export smoke test passed.");
