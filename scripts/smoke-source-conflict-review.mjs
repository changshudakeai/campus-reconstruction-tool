import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const service = readFileSync("src/services/sourceConflictReview.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const writer = readFileSync("src/services/spongeSchematic.ts", "utf8");
for (const marker of [
  "detectSourceConflicts",
  "SourceConflictReviewPanel",
  "recordSourceConflictDecision",
  "SourceConflicts",
  "sourceConflictReview"
]) {
  if (!service.includes(marker) && !app.includes(marker) && !writer.includes(marker)) {
    throw new Error(`Missing source conflict marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/source-conflict-review-entry.ts`;
const bundle = `${smokeDir}/source-conflict-review-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { gunzipSync } from "node:zlib";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import type { ExternalModelCandidate } from "../../src/domain/externalModel";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";
import { recordExternalModelReview } from "../../src/services/externalModelReview";
import { detectSourceConflicts, recordSourceConflictDecision, sourceConflictsForReview } from "../../src/services/sourceConflictReview";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);
let model = generateSchematicFromBuildingGeometry(geometry);
const externalModel: ExternalModelCandidate = {
  id: "conflicting-reference-model",
  source: "3dmr",
  title: "Conflicting Putuo Library mesh",
  sourceUrl: "https://example.invalid/3dmr/conflicting-putuo-library",
  author: "Reference Mapper",
  license: {
    name: "CC-BY-4.0",
    url: "https://creativecommons.org/licenses/by/4.0/",
    allowsAdaptation: true,
    requiresAttribution: true
  },
  dimensionsMeters: {
    width: model.metadata.generationReport!.dimensions.footprintWidthMeters * 1.8,
    length: model.metadata.generationReport!.dimensions.footprintLengthMeters * 0.55,
    height: model.height / model.metadata.generationReport!.blocksPerMeter * 1.7
  }
};
model = recordExternalModelReview(model, externalModel, "pending", "Detected from external model source.");
const detected = detectSourceConflicts(model);
assert(detected.length === 1, "expected one source conflict");
assert(detected[0].kind === "dimension_mismatch", "expected dimension mismatch");
assert(detected[0].severity === "blocking", "expected blocking severity");
assert(detected[0].evidence.length >= 3, "expected dimension evidence");
model = recordSourceConflictDecision(model, detected[0], "supporting_only", "External mesh dimensions disagree with accepted open footprint.");
const reviewed = sourceConflictsForReview(model);
assert(reviewed[0].decision === "supporting_only", "expected stored decision");
assert(reviewed[0].decisionReason?.includes("disagree"), "expected decision reason");
const exported = prepareDetailedSchematicExport(model);
const companion = JSON.parse(exported.provenanceJson);
const nbtText = new TextDecoder().decode(gunzipSync(exported.bytes));
assert(companion.provenance.sourceConflicts[0].decision === "supporting_only", "expected conflict decision in provenance");
assert(nbtText.includes("SourceConflicts"), "expected SourceConflicts in NBT metadata");
assert(nbtText.includes("supporting_only"), "expected conflict decision in NBT metadata");

let rejectedEmptyReason = false;
try {
  recordSourceConflictDecision(model, detected[0], "rejected", "   ");
} catch {
  rejectedEmptyReason = true;
}
assert(rejectedEmptyReason, "expected conflict decision reason requirement");
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
console.log("Source conflict review smoke test passed.");
