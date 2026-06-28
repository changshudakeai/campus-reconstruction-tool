import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const service = readFileSync("src/services/foundationManifestExport.ts", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "exportFoundationManifestJson",
  "parseFoundationManifestJson",
  ".foundation_manifest.json",
  "Export Foundation .schem + Manifest",
  "exportedManifestFeatures"
]) {
  if (!app.includes(marker) && !service.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing foundation manifest export marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/foundation-manifest-export-entry.ts`;
const bundle = `${smokeDir}/foundation-manifest-export-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { foundationManifestPlaceholder } from "../../src/domain/foundationManifest";
import {
  exportFoundationManifestJson,
  parseFoundationManifestJson
} from "../../src/services/foundationManifestExport";
import {
  applyFoundationStyle,
  DEFAULT_FOUNDATION_STYLE,
  updateFeatureBlockStyle
} from "../../src/services/foundationStyle";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const styledManifest = applyFoundationStyle(
  foundationManifestPlaceholder,
  updateFeatureBlockStyle(DEFAULT_FOUNDATION_STYLE, "building", "bricks")
);
const exported = exportFoundationManifestJson(
  styledManifest,
  "ecnu_putuo_foundation.foundation_manifest.json"
);
const parsed = parseFoundationManifestJson(exported.json);
const libraryFeature = parsed.mapFeatures.find((feature) => feature.kind === "building");
const librarySlot = parsed.buildingSlots.find((slot) => slot.geometryRole === "representative-building");

assert(exported.fileName.endsWith(".foundation_manifest.json"), "expected manifest handoff file name");
assert(exported.json.endsWith("\\n"), "expected pretty JSON to end with newline");
assert(parsed.schemaVersion === "0.1.0", "expected schema version");
assert(exported.featureCount === parsed.mapFeatures.length, "expected feature count to match JSON");
assert(exported.slotCount === parsed.buildingSlots.length, "expected slot count to match JSON");
assert(parsed.target.campus === "ECNU Putuo Campus", "expected target campus");
assert(Boolean(libraryFeature), "expected exported building feature");
assert(libraryFeature?.block === "bricks", "expected selected block style to be preserved");
assert(libraryFeature?.geometry.points.length, "expected feature coordinates to be preserved");
assert(libraryFeature?.provenance.rawId, "expected feature provenance to be preserved");
assert(Boolean(librarySlot), "expected representative building slot");
assert(librarySlot?.sourceFeatureId, "expected slot source feature link");
assert(librarySlot?.geometry.points.length, "expected slot geometry to be preserved");
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
console.log("Foundation manifest export smoke test passed.");
