import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const styles = readFileSync("src/styles.css", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "FoundationExportPreviewPanel",
  "Export size preview",
  "foundation-export-preview",
  "export-preview-grid"
]) {
  if (!app.includes(marker) && !styles.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing foundation export preview marker: ${marker}`);
  }
}

if (app.includes("const foundationModel = reviewedManifest.mapFeatures.length")) {
  throw new Error("Foundation 3D models must not be generated during render");
}
if (app.includes("const foundationPreview = makeFoundationExportPreview")) {
  throw new Error("Foundation export-size raster estimation must not run during render");
}
if ((app.match(/function generateFoundation3dPreview\(\)/g) ?? []).length < 2) {
  throw new Error("Both foundation workflows need an explicit 3D preview action");
}
if (!i18n.includes('generateFoundation3dPreview:')) {
  throw new Error("Missing Generate foundation 3D preview label");
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/foundation-export-preview-entry.ts`;
const bundle = `${smokeDir}/foundation-export-preview-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { foundationManifestPlaceholder } from "../../src/domain/foundationManifest";
import {
  generateFoundationSchematicFromManifest,
  previewFoundationSchematicExport
} from "../../src/services/foundationManifestToSchematic";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function nonAirBlocks(blockData: Uint16Array) {
  let total = 0;
  for (const block of blockData) {
    if (block !== 0) total += 1;
  }
  return total;
}

const preview = previewFoundationSchematicExport(foundationManifestPlaceholder);
const exportResult = generateFoundationSchematicFromManifest(foundationManifestPlaceholder);
const model = exportResult.model;

assert(preview.width === model.width, "expected preview width to match export");
assert(preview.height === model.height, "expected preview height to match export");
assert(preview.length === model.length, "expected preview length to match export");
assert(preview.totalBlocks === model.blockData.length, "expected preview total block volume to match export");
assert(preview.estimatedNonAirBlocks === nonAirBlocks(model.blockData), "expected exact non-air estimate");
assert(preview.reviewedFeatureCount === exportResult.featureCount, "expected preview feature count to match export");
assert(preview.paletteSize === model.palette.length, "expected preview palette size to match export");
assert(preview.risk === "ready", "expected default Putuo foundation preview to be ready-sized");

const largePreview = previewFoundationSchematicExport(foundationManifestPlaceholder, {
  blocksPerMeter: 2
});
assert(largePreview.totalBlocks > preview.totalBlocks, "expected larger scale to increase total blocks");
assert(largePreview.risk === "large" || largePreview.risk === "very_large", "expected large scale risk");
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
console.log("Foundation export preview smoke test passed.");
