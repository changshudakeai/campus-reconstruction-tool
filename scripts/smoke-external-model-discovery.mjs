import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const discovery = readFileSync("src/services/externalModelDiscovery.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
for (const marker of [
  "externalModelCandidatesFromArnis",
  "summarizeExternalModelCandidates",
  "externalModelSummary",
  "ExternalModelReviewPanel",
  "recordExternalModelReview",
  "saveExternalModelReview",
  "eligible_primary",
  "externalModelCandidates"
]) {
  if (!discovery.includes(marker) && !app.includes(marker)) {
    throw new Error(`Missing external model discovery marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/external-model-discovery-entry.ts`;
const bundle = `${smokeDir}/external-model-discovery-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import type { ArnisBuildingCandidate } from "../../src/adapters/arnisRustCoreAdapter";
import { buildingGeometryFromArnisCandidate } from "../../src/adapters/arnisRustCoreAdapter";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { externalModelCandidatesFromArnis, summarizeExternalModelCandidates } from "../../src/services/externalModelDiscovery";
import { classifyExternalModelLicense, recordExternalModelReview } from "../../src/services/externalModelReview";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const candidate: ArnisBuildingCandidate = {
  id: "osm:5699952",
  source: "osm_overpass",
  name: "图书馆",
  tags: {
    building: "university",
    name: "图书馆",
    wikidata: "Q123456",
    "wikidata:license": "CC-BY-4.0",
    "wikidata:license_url": "https://creativecommons.org/licenses/by/4.0/",
    "wikidata:model_author": "Wikidata Mapper",
    "3dmr": "putuo-library-reference",
    "3dmr:license": "CC-BY-SA-4.0",
    "3dmr:license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
    "3dmr:author": "3DMR Mapper"
  },
  components: [{
    exterior: [
      { lng: 121.4018, lat: 31.2301 },
      { lng: 121.4022, lat: 31.2301 },
      { lng: 121.4022, lat: 31.2305 },
      { lng: 121.4018, lat: 31.2305 }
    ],
    interiorRings: []
  }],
  heightM: 24,
  floors: 5,
  roofShape: "hipped",
  identityConfidence: "high",
  distanceM: 18,
  widthM: 42,
  lengthM: 58,
  parts: []
};

const discovered = externalModelCandidatesFromArnis(candidate);
assert(discovered.length === 2, "expected 3DMR and Wikidata external candidates");
assert(discovered.some((item) => item.source === "3dmr"), "expected 3DMR candidate");
assert(discovered.some((item) => item.wikidataId === "Q123456"), "expected Wikidata candidate");
assert(discovered.every((item) => classifyExternalModelLicense(item).eligibility === "eligible"), "expected eligible licenses");
const summary = summarizeExternalModelCandidates(candidate);
assert(summary.total === 2 && summary.eligible === 2 && summary.blocked === 0, "expected eligible summary");

const geometry = buildingGeometryFromArnisCandidate(PUTUO_LIBRARY_TARGET, candidate);
let model = generateSchematicFromBuildingGeometry(geometry);
for (const externalModel of discovered) {
  model = recordExternalModelReview(model, externalModel, "pending", "Discovered from selected candidate tags.");
}
assert(model.metadata.provenance?.externalModels?.length === 2, "expected pending external model provenance");
assert(model.metadata.provenance?.externalModels?.every((item) => item.decision === "pending"), "expected pending decisions");

const blockedCandidate: ArnisBuildingCandidate = {
  ...candidate,
  tags: {
    ...candidate.tags,
    "3dmr": "blocked-model",
    "3dmr:license": "CC-BY-ND-4.0",
    "3dmr:license_url": "https://creativecommons.org/licenses/by-nd/4.0/"
  }
};
const blockedSummary = summarizeExternalModelCandidates(blockedCandidate);
assert(blockedSummary.blocked >= 1, "expected blocked no-derivatives candidate");
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
console.log("External model discovery smoke test passed.");
