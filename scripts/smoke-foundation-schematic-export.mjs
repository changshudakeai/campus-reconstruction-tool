import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/foundation-schematic-export-entry.ts`;
const bundle = `${smokeDir}/foundation-schematic-export-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { gunzipSync } from "node:zlib";
import { foundationManifestPlaceholder } from "../../src/domain/foundationManifest";
import { countBlocks } from "../../src/domain/schematicModel";
import { generateFoundationSchematicFromManifest } from "../../src/services/foundationManifestToSchematic";
import { gzipBytes } from "../../src/services/gzip";
import { writeSpongeV2Schematic } from "../../src/services/spongeSchematic";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const result = generateFoundationSchematicFromManifest(foundationManifestPlaceholder);
const model = result.model;
const rawBytes = writeSpongeV2Schematic(model);
const bytes = gzipBytes(rawBytes);
const uncompressed = gunzipSync(bytes);

const grass = model.palette.indexOf("minecraft:grass_block");
const quartz = model.palette.indexOf("minecraft:quartz_block");

assert(model.metadata.generator === "foundation-manifest-to-schematic", "expected foundation generator metadata");
assert(model.width > 100, "expected campus-scale width");
assert(model.length > 100, "expected campus-scale length");
assert(model.blockData.length === model.width * model.height * model.length, "expected blockData shape");
assert(grass >= 0, "expected campus grass block palette");
assert(quartz >= 0, "expected building quartz block palette");
assert(countBlocks(model, grass) > 0, "expected campus ground blocks");
assert(countBlocks(model, quartz) > 0, "expected building footprint blocks");
assert(bytes[0] === 0x1f && bytes[1] === 0x8b, "expected gzip header");
assert(uncompressed[0] === 10, "expected Sponge root compound after gunzip");
assert(new TextDecoder().decode(uncompressed).includes("PaletteMax"), "expected Sponge v2 PaletteMax");
assert(new TextDecoder().decode(uncompressed).includes("foundation-manifest-to-schematic"), "expected foundation metadata in export");
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
console.log("Foundation schematic export smoke test passed.");
