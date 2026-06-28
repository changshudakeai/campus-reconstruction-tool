import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const generator = readFileSync("src/services/buildingGeometryToSchematic.ts", "utf8");
const modelDomain = readFileSync("src/domain/schematicModel.ts", "utf8");

for (const marker of [
  "generateSchematicWithArnisCore(geometry",
  "setSchematicModel(schematic)",
  'generator: "building-geometry-to-schematic"',
  "Building Geometry must include at least one polygon footprint component"
]) {
  if (!app.includes(marker) && !generator.includes(marker) && !modelDomain.includes(marker)) {
    throw new Error(`Missing Detailed Mode schematic handoff marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/detailed-schematic-handoff-entry.ts`;
const bundle = `${smokeDir}/detailed-schematic-handoff-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { createBuildingSlotHandoffProvider } from "../../src/adapters/buildingSlotHandoffProvider";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import {
  countBlocks,
  type SchematicModel
} from "../../src/domain/schematicModel";
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import {
  generateSchematicFromBuildingGeometry,
  schematicPaletteIndexes
} from "../../src/services/buildingGeometryToSchematic";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";
import { applyManualBuildingGeometryCorrection } from "../../src/services/manualBuildingGeometryCorrection";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function assertPreviewableModel(model: SchematicModel) {
  assert(model.width > 0 && model.height > 0 && model.length > 0, "expected positive dimensions");
  assert(model.blockData.length === model.width * model.height * model.length, "expected dense block data");
  assert(model.palette[0] === "minecraft:air", "expected air palette origin");
  assert(countBlocks(model, schematicPaletteIndexes.WALL) > 0, "expected generated walls");
  assert(countBlocks(model, schematicPaletteIndexes.ROOF) > 0, "expected generated roof");
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected Putuo Library Building Slot");
const target = buildingSlotToBuildingTarget(slot!);
const geometry = await new MinimalArnisAdapter([
  putuoLibraryFixtureProvider,
  createBuildingSlotHandoffProvider(slot!)
]).getBuildingGeometry(target);
const model = generateSchematicFromBuildingGeometry(geometry);

assertPreviewableModel(model);
assert(model.metadata.generator === "building-geometry-to-schematic", "expected geometry generator metadata");
assert(model.metadata.sourceBuilding === slot!.name, "expected selected Building Slot name");
assert(model.metadata.nonRectangularFootprint, "expected library non-rectangular footprint");
assert(model.metadata.roofShape === "hipped", "expected roof hint handoff");

const correctedGeometry = applyManualBuildingGeometryCorrection(geometry, {
  reason: "Detailed handoff regression correction.",
  heightM: 30,
  floors: 7,
  roof: { shape: "flat" }
}).geometry;
const correctedModel = generateSchematicFromBuildingGeometry(correctedGeometry);

assertPreviewableModel(correctedModel);
assert(correctedModel.height !== model.height, "expected corrected height to regenerate model dimensions");
assert(correctedModel.metadata.roofShape === "flat", "expected corrected roof handoff");
assert(correctedModel.blockData !== model.blockData, "expected a fresh schematic model");
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
console.log("Detailed Building Geometry to SchematicModel handoff smoke test passed.");
