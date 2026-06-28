import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const domain = readFileSync("src/domain/foundationManifest.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const service = readFileSync("src/services/foundationManifestExport.ts", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "GeometryDimensions",
  "summarizeGeometry",
  "selectedBlock",
  "assertGeometryDimensions",
  "coordinateBounds",
  "Selected Blocks"
]) {
  if (
    !domain.includes(marker) &&
    !app.includes(marker) &&
    !service.includes(marker) &&
    !i18n.includes(marker)
  ) {
    throw new Error(`Missing Foundation Manifest contract marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/foundation-manifest-contract-entry.ts`;
const bundle = `${smokeDir}/foundation-manifest-contract-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { foundationManifestPlaceholder } from "../../src/domain/foundationManifest";
import { acceptCandidate, buildFoundationManifestFromReviews } from "../../src/services/candidateReview";
import {
  exportFoundationManifestJson,
  parseFoundationManifestJson
} from "../../src/services/foundationManifestExport";
import {
  applyFoundationStyle,
  DEFAULT_FOUNDATION_STYLE,
  updateFeatureBlockStyle
} from "../../src/services/foundationStyle";
import { polygonCandidate } from "../../src/services/mapCandidateFactory";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const buildingCandidate = polygonCandidate({
  id: "contract-library",
  name: "Contract Library",
  kind: "building",
  source: "overture",
  confidence: "high",
  query: "ECNU Putuo Campus",
  rawId: "overture:building:contract-library",
  notes: ["Contract test building."],
  points: [
    [121.40854, 31.22844],
    [121.40903, 31.22858],
    [121.40938, 31.22830],
    [121.40922, 31.22794],
    [121.40874, 31.22785],
    [121.40847, 31.22812]
  ]
});

const reviewedManifest = buildFoundationManifestFromReviews(
  foundationManifestPlaceholder,
  [acceptCandidate(buildingCandidate)],
  []
);
const styledManifest = applyFoundationStyle(
  reviewedManifest,
  updateFeatureBlockStyle(DEFAULT_FOUNDATION_STYLE, "building", "bricks")
);
const parsed = parseFoundationManifestJson(exportFoundationManifestJson(styledManifest).json);
const feature = parsed.mapFeatures.find((item) => item.id === "feature-contract-library");
const slot = parsed.buildingSlots.find((item) => item.sourceFeatureId === "feature-contract-library");

assert(Boolean(feature), "expected building feature in manifest contract");
assert(Boolean(slot), "expected building slot in manifest contract");
assert(feature?.block === "bricks", "expected selected Map Feature block");
assert(slot?.selectedBlock === "bricks", "expected selected Building Slot block");
assert(feature?.confidence === "high", "expected feature confidence");
assert(slot?.confidence === "high", "expected slot confidence");
assert(feature?.provenance.rawId === "overture:building:contract-library", "expected feature provenance rawId");
assert(slot?.provenance.rawId === "overture:building:contract-library", "expected slot provenance rawId");
assert(feature?.geometry.points.length === 6, "expected feature coordinates");
assert(slot?.geometry.points.length === 6, "expected slot coordinates");
assert(feature?.dimensions.pointCount === 6, "expected feature dimensions point count");
assert(slot?.dimensions.pointCount === 6, "expected slot dimensions point count");
assert((feature?.dimensions.approximateWidthMeters ?? 0) > 0, "expected feature width meters");
assert((feature?.dimensions.approximateLengthMeters ?? 0) > 0, "expected feature length meters");
assert(slot?.dimensions.bounds.minLng === feature?.dimensions.bounds.minLng, "expected slot bounds from feature");

const brokenManifest = JSON.parse(JSON.stringify(parsed));
delete brokenManifest.buildingSlots[0].selectedBlock;
let rejectedBrokenManifest = false;
try {
  parseFoundationManifestJson(JSON.stringify(brokenManifest));
} catch {
  rejectedBrokenManifest = true;
}
assert(rejectedBrokenManifest, "expected parser to reject missing selectedBlock");

const brokenDimensionsManifest = JSON.parse(JSON.stringify(parsed));
delete brokenDimensionsManifest.mapFeatures[0].dimensions;
let rejectedBrokenDimensions = false;
try {
  parseFoundationManifestJson(JSON.stringify(brokenDimensionsManifest));
} catch {
  rejectedBrokenDimensions = true;
}
assert(rejectedBrokenDimensions, "expected parser to reject missing dimensions");
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
console.log("Foundation Manifest contract smoke test passed.");
