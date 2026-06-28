import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const modelDomain = readFileSync("src/domain/schematicModel.ts", "utf8");
const writer = readFileSync("src/services/spongeSchematic.ts", "utf8");
const exportService = readFileSync("src/services/detailedSchematicExport.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");

for (const marker of [
  "SchematicProvenance",
  "blockReplacements",
  "SourcePriority",
  "UsedSources",
  "FoundationSlotId",
  "provenanceFileName",
  "provenanceJson",
  "recordedEdits"
]) {
  if (!modelDomain.includes(marker) && !writer.includes(marker) && !exportService.includes(marker) && !app.includes(marker)) {
    throw new Error(`Missing export provenance marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/export-provenance-entry.ts`;
const bundle = `${smokeDir}/export-provenance-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { createBuildingSlotHandoffProvider } from "../../src/adapters/buildingSlotHandoffProvider";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";
import { applyManualBuildingGeometryCorrection } from "../../src/services/manualBuildingGeometryCorrection";
import { replaceAllMatchingBlocks } from "../../src/services/schematicEditing";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected representative slot");
const target = buildingSlotToBuildingTarget(slot!);
const automaticGeometry = await new MinimalArnisAdapter([
  putuoLibraryFixtureProvider,
  createBuildingSlotHandoffProvider(slot!)
]).getBuildingGeometry(target);
const correctedGeometry = applyManualBuildingGeometryCorrection(automaticGeometry, {
  reason: "Export provenance regression correction.",
  heightM: 28,
  facade: { material: "red_brick" }
}).geometry;
const generatedModel = generateSchematicFromBuildingGeometry(correctedGeometry);
const editedModel = replaceAllMatchingBlocks(
  generatedModel,
  "minecraft:stone_bricks",
  "minecraft:bricks"
).model;
const exported = prepareDetailedSchematicExport(editedModel);
const companion = JSON.parse(exported.provenanceJson);
const nbtText = new TextDecoder().decode(exported.bytes);

assert(exported.provenanceFileName === "putuo_campus_library.provenance.json", "expected provenance filename");
assert(companion.schemaVersion === "0.1.0", "expected provenance schema version");
assert(companion.schematic.fileName === exported.fileName, "expected linked schematic filename");
assert(companion.schematic.generationReport.fidelity.footprintIoU > 0.8, "expected exported fidelity report");
assert(companion.provenance.usedSources.includes("overture"), "expected Overture source");
assert(companion.provenance.usedSources.includes("existing_project"), "expected Manifest source");
assert(companion.provenance.usedSources.includes("manual_correction"), "expected manual source");
assert(companion.provenance.handoff.foundationSlotId === slot!.id, "expected Foundation Slot handoff");
assert(companion.provenance.handoff.sourceFeatureId === slot!.sourceFeatureId, "expected source feature handoff");
assert(companion.provenance.notes.some((note: string) => note.includes("Manual correction")), "expected manual note");
assert(companion.provenance.notes.some((note: string) => note.includes("Batch replacement")), "expected replacement note");
assert(companion.provenance.corrections[0].reason === "Export provenance regression correction.", "expected correction reason export");
assert(companion.provenance.generationAssumptions.some((item: { field: string }) => item.field === "floorSpacingMeters"), "expected generation assumption export");
assert(companion.provenance.geometryValidation.valid, "expected geometry validation export");
assert(companion.provenance.blockReplacements.length === 1, "expected one replacement record");
assert(companion.provenance.blockReplacements[0].sourceBlock === "minecraft:stone_bricks", "expected replacement source");
assert(companion.provenance.blockReplacements[0].replacementBlock === "minecraft:bricks", "expected replacement target");
assert(companion.provenance.blockReplacements[0].replacedCount > 0, "expected replacement count");
assert(generatedModel.metadata.provenance?.blockReplacements.length === 0, "expected original provenance unchanged");

for (const marker of [
  "SourcePriority",
  "UsedSources",
  "manual_correction",
  "FoundationSlotId",
  slot!.id,
  "BlockReplacements",
  "GenerationAssumptions",
  "GeometryCorrections",
  "minecraft:bricks"
]) {
  assert(nbtText.includes(marker), "expected NBT provenance marker: " + marker);
}
  `.trim()
);

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
console.log("Detailed export provenance smoke test passed.");
