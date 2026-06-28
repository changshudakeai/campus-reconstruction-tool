import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const styles = readFileSync("src/styles.css", "utf8");

for (const marker of [
  "FoundationStylePanel",
  "updateRoadWidthStyle",
  "updateFeatureBlockStyle",
  "MinecraftBlockPicker",
  "foundation-style-panel"
]) {
  if (!app.includes(marker) && !styles.includes(marker)) {
    throw new Error(`Missing foundation style marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/foundation-style-controls-entry.ts`;
const bundle = `${smokeDir}/foundation-style-controls-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { foundationManifestPlaceholder } from "../../src/domain/foundationManifest";
import { countBlocks } from "../../src/domain/schematicModel";
import { generateFoundationSchematicFromManifest } from "../../src/services/foundationManifestToSchematic";
import {
  applyFoundationStyle,
  DEFAULT_FOUNDATION_STYLE,
  updateFeatureBlockStyle,
  updateRoadWidthStyle
} from "../../src/services/foundationStyle";
import { polylineCandidate } from "../../src/services/mapCandidateFactory";
import { acceptCandidate, buildFoundationManifestFromReviews } from "../../src/services/candidateReview";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

let style = updateFeatureBlockStyle(DEFAULT_FOUNDATION_STYLE, "building", "bricks");
style = updateRoadWidthStyle(style, 9);

const styledManifest = applyFoundationStyle(foundationManifestPlaceholder, style);
const library = styledManifest.mapFeatures.find((feature) => feature.kind === "building");
assert(library?.block === "bricks", "expected building style override");

const roadCandidate = polylineCandidate({
  id: "road-style-test",
  name: "Road Style Test",
  kind: "road",
  source: "manual_drawing",
  confidence: "manual",
  query: "test",
  rawId: "manual:road",
  notes: [],
  points: [
    [121.408, 31.228],
    [121.410, 31.228]
  ]
});
const roadManifest = buildFoundationManifestFromReviews(
  foundationManifestPlaceholder,
  [acceptCandidate(roadCandidate)],
  []
);
const narrow = generateFoundationSchematicFromManifest(roadManifest, { roadWidthBlocks: 2 });
const wide = generateFoundationSchematicFromManifest(roadManifest, { roadWidthBlocks: 9 });
const roadBlockIndex = narrow.model.palette.indexOf("minecraft:gray_concrete");
const wideRoadBlockIndex = wide.model.palette.indexOf("minecraft:gray_concrete");

assert(roadBlockIndex >= 0, "expected road block in narrow palette");
assert(wideRoadBlockIndex >= 0, "expected road block in wide palette");
assert(
  countBlocks(wide.model, wideRoadBlockIndex) > countBlocks(narrow.model, roadBlockIndex),
  "expected wider road export to contain more road blocks"
);
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
console.log("Foundation style controls smoke test passed.");
