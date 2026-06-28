import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const domain = readFileSync("src/domain/semanticFeature.ts", "utf8");
const service = readFileSync("src/services/semanticFeaturePreservation.ts", "utf8");
const schematic = readFileSync("src/domain/schematicModel.ts", "utf8");
const writer = readFileSync("src/services/spongeSchematic.ts", "utf8");

for (const marker of [
  "SemanticFeatureAnnotation",
  "applySemanticFeatureAnnotations",
  "semanticFeatures",
  "SemanticFeatures"
]) {
  if (!domain.includes(marker) && !service.includes(marker) && !schematic.includes(marker) && !writer.includes(marker)) {
    throw new Error(`Missing semantic feature marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/semantic-feature-preservation-entry.ts`;
const bundle = `${smokeDir}/semantic-feature-preservation-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { gunzipSync } from "node:zlib";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";
import { applySemanticFeatureAnnotations } from "../../src/services/semanticFeaturePreservation";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const original = generateSchematicFromBuildingGeometry(geometry);
const before = new Uint16Array(original.blockData);
const strengthened = applySemanticFeatureAnnotations(original, [
  {
    id: "south-entrance-presence",
    kind: "entrance_emphasis",
    label: "Main south entrance",
    side: "south",
    heightBand: "lower",
    strength: "visible",
    reason: "Gaode 3D review shows the entrance reads too weakly at Minecraft scale."
  },
  {
    id: "upper-window-band",
    kind: "window_band",
    label: "Upper floor window band",
    side: "south",
    heightBand: "upper",
    strength: "strong",
    reason: "The facade rhythm is an identity-bearing feature."
  },
  {
    id: "roof-ridge",
    kind: "roof_ridge",
    label: "Visible roof ridge",
    side: "center",
    heightBand: "roof",
    strength: "visible",
    reason: "Roof silhouette needs a readable ridge."
  }
]);

assert(strengthened.width === original.width, "expected width unchanged");
assert(strengthened.height === original.height, "expected height unchanged");
assert(strengthened.length === original.length, "expected length unchanged");
let changed = 0;
for (let index = 0; index < before.length; index += 1) if (before[index] !== strengthened.blockData[index]) changed += 1;
assert(changed > 0, "expected semantic features to alter visible blocks");
assert(strengthened.metadata.provenance?.semanticFeatures?.length === 3, "expected semantic feature provenance");
assert(strengthened.metadata.provenance?.semanticFeatures?.every((record) => record.envelopeChanged === false), "expected envelope unchanged records");
assert(strengthened.metadata.provenance?.notes.some((note) => note.includes("semantic feature")), "expected semantic note");
const exported = prepareDetailedSchematicExport(strengthened);
const companion = JSON.parse(exported.provenanceJson);
const nbtText = new TextDecoder().decode(gunzipSync(exported.bytes));
assert(companion.provenance.semanticFeatures.length === 3, "expected semantic features in companion provenance");
assert(nbtText.includes("SemanticFeatures"), "expected SemanticFeatures in NBT metadata");
assert(nbtText.includes("south-entrance-presence"), "expected annotation id in NBT metadata");

let rejectedNoReason = false;
try {
  applySemanticFeatureAnnotations(original, [{
    id: "bad",
    kind: "frame",
    label: "Bad annotation",
    side: "south",
    heightBand: "middle",
    strength: "visible",
    reason: " "
  }]);
} catch {
  rejectedNoReason = true;
}
assert(rejectedNoReason, "expected reason requirement");
`.trim());

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
console.log("Semantic feature preservation smoke test passed.");
