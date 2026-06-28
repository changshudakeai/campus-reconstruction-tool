import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const domain = readFileSync("src/domain/foundationManifest.ts", "utf8");
const review = readFileSync("src/services/candidateReview.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "representativeBuildingSlotId",
  "isPutuoLibraryName",
  "chooseRepresentativeBuildingSlot",
  "selectRepresentativeBuildingSlot",
  "representativeSlotId"
]) {
  if (!domain.includes(marker) && !review.includes(marker) && !app.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing representative building marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/representative-building-selection-entry.ts`;
const bundle = `${smokeDir}/representative-building-selection-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import {
  foundationManifestPlaceholder,
  isPutuoLibraryName,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import {
  acceptCandidate,
  buildFoundationManifestFromReviews
} from "../../src/services/candidateReview";
import {
  exportFoundationManifestJson,
  parseFoundationManifestJson
} from "../../src/services/foundationManifestExport";
import { polygonCandidate } from "../../src/services/mapCandidateFactory";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function buildingCandidate(id: string, name: string) {
  return polygonCandidate({
    id,
    name,
    kind: "building",
    source: "overture",
    confidence: "high",
    query: "ECNU Putuo Campus",
    rawId: \`overture:building:\${id}\`,
    notes: ["Representative selection test."],
    points: [
      [121.40854, 31.22844],
      [121.40903, 31.22858],
      [121.40938, 31.22830],
      [121.40922, 31.22794],
      [121.40874, 31.22785],
      [121.40847, 31.22812]
    ]
  });
}

assert(isPutuoLibraryName("Putuo Campus Library"), "expected English library name match");
assert(isPutuoLibraryName("华东师范大学普陀校区图书馆"), "expected Chinese library name match");

const sportsHall = buildingCandidate("sports-hall", "Putuo Campus Sports Hall");
const chineseLibrary = buildingCandidate("chinese-library", "华东师范大学普陀校区图书馆");
const manifest = buildFoundationManifestFromReviews(
  foundationManifestPlaceholder,
  [acceptCandidate(sportsHall), acceptCandidate(chineseLibrary)],
  []
);
const selectedSlot = selectRepresentativeBuildingSlot(manifest);

assert(manifest.buildingSlots[0]?.name === "Putuo Campus Sports Hall", "expected non-library slot first");
assert(selectedSlot?.name === "华东师范大学普陀校区图书馆", "expected Putuo library to be selected");
assert(manifest.representativeBuildingSlotId === selectedSlot?.id, "expected representative slot id to point at selected slot");
assert(selectedSlot?.geometryRole === "representative-building", "expected representative geometry role");

const parsed = parseFoundationManifestJson(exportFoundationManifestJson(manifest).json);
assert(
  selectRepresentativeBuildingSlot(parsed)?.name === "华东师范大学普陀校区图书馆",
  "expected exported manifest to preserve representative selection"
);

const brokenManifest = JSON.parse(JSON.stringify(parsed));
brokenManifest.representativeBuildingSlotId = "slot-does-not-exist";
let rejectedBrokenRepresentative = false;
try {
  parseFoundationManifestJson(JSON.stringify(brokenManifest));
} catch {
  rejectedBrokenRepresentative = true;
}
assert(rejectedBrokenRepresentative, "expected parser to reject missing representative slot id");
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
console.log("Representative building selection smoke test passed.");
