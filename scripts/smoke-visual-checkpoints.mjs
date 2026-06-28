import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const previewer = readFileSync("src/components/SchematicPreviewer.tsx", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
for (const marker of ["PREVIEW_CAMERA_VIEWS", "footprintOverlay", "preserveDrawingBuffer", "captureLabel", "ObservedBuildingEvidencePanel", "GeneratedBuildingInterpretationPanel", "t.advancedDataAndProvenance", "recordResultComparison", "referenceComparison"]) {
  if (!previewer.includes(marker) && !app.includes(marker)) throw new Error(`Missing visual checkpoint marker: ${marker}`);
}
if (app.includes("fixtureBaselineModel")) throw new Error("Fixture Test Assets must not appear as a product baseline.");
if (app.includes("Staged visual review")) throw new Error("Redundant staged visual review must not appear in the product UI.");

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/visual-checkpoints-entry.ts`;
const bundle = `${smokeDir}/visual-checkpoints-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";
import { PREVIEW_CAMERA_VIEWS, VISUAL_CHECKPOINTS, recordCapturedView, recordCheckpointDecision, recordResultComparison, visualReviewFor } from "../../src/services/visualCheckpoints";

function assert(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);
let model = generateSchematicFromBuildingGeometry(geometry);
assert((model.metadata.generationReport?.footprintOverlay.length ?? 0) > 0, "expected geographic overlay geometry");
for (const view of PREVIEW_CAMERA_VIEWS) model = recordCapturedView(model, view);
for (const checkpoint of VISUAL_CHECKPOINTS.filter((item) => item !== "recognizability")) {
  model = recordCheckpointDecision(model, checkpoint, "approved", checkpoint + " visually checked");
}
model = recordCheckpointDecision(model, "roof", "rejected", "silhouette needs a lower ridge");
let review = visualReviewFor(model);
assert(review.capturedViews.length === 4, "expected all fixed views retained");
assert(review.checkpoints.roof.decision === "rejected", "expected rejection retained");
assert(review.checkpoints.roof.note.includes("lower ridge"), "expected rejection note retained");
assert(review.checkpoints.recognizability.decision === "pending", "expected human decision pending");
model = recordResultComparison(model, {
  evidence: {
    source: "arnis_reference_reconstruction",
    label: "Arnis 2.9.0 Putuo Library reference",
    description: "Reference bbox, source fingerprint, and fixed views from the user-confirmed upstream Arnis reconstruction.",
    capturedViews: review.capturedViews,
    notes: ["Synthetic fixtures are not accepted as product comparison evidence."]
  },
  outcome: "differs",
  summary: "Current result differs from the Arnis Reference Reconstruction in roof silhouette and facade rhythm.",
  correctionNotes: ["Lower the roof ridge.", "Strengthen facade rhythm before Axiom acceptance."]
});
review = visualReviewFor(model);
assert(review.resultComparison?.evidence.source === "arnis_reference_reconstruction", "expected Arnis reference comparison source");
assert(review.resultComparison?.correctionNotes.length === 2, "expected correction notes for differing comparison");
let rejectedFixtureEvidence = false;
try {
  recordResultComparison(model, {
    evidence: { source: "fixture_baseline" as any, label: "Fixture", description: "Synthetic fixture" },
    outcome: "matches",
    summary: "Fixture is not real evidence."
  });
} catch {
  rejectedFixtureEvidence = true;
}
assert(rejectedFixtureEvidence, "expected fixture comparison evidence rejection");
let rejectedEmptyNote = false;
try { recordCheckpointDecision(model, "materials", "rejected", "   "); } catch { rejectedEmptyNote = true; }
assert(rejectedEmptyNote, "expected rejection note requirement");
const provenance = JSON.parse(prepareDetailedSchematicExport(model).provenanceJson);
assert(provenance.provenance.visualReview.capturedViews.length === 4, "expected captures in export provenance");
assert(provenance.provenance.visualReview.checkpoints.roof.note.includes("lower ridge"), "expected notes in export provenance");
assert(provenance.provenance.visualReview.resultComparison.evidence.source === "arnis_reference_reconstruction", "expected comparison source in export provenance");
assert(provenance.provenance.visualReview.resultComparison.correctionNotes[0].includes("Lower"), "expected comparison correction notes in export provenance");
`.trim());
await build({ entryPoints: [resolve(entry)], outfile: resolve(bundle), bundle: true, platform: "node", format: "esm", sourcemap: false, logLevel: "silent" });
await import(`${pathToFileURL(resolve(bundle)).href}?t=${Date.now()}`);
console.log("Visual checkpoints smoke test passed.");
