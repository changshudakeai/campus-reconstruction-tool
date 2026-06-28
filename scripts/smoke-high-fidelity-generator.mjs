import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const generator = readFileSync("src/services/buildingGeometryToSchematic.ts", "utf8");
const model = readFileSync("src/domain/schematicModel.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");
for (const marker of [
  "SchematicGenerationReport",
  "BuildingMaterialStyle",
  "measureRasterizationFidelity",
  "institutional-window-bays-with-floor-bands",
  "selectEntranceCells"
]) {
  if (!generator.includes(marker) && !model.includes(marker) && !app.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing high-fidelity generator marker: ${marker}`);
  }
}
for (const marker of ["footprintIoU", "semanticBlockCounts", "roofAssumption", "entranceEmphasis"]) {
  if (!app.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing generation report UI marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/high-fidelity-generator-entry.ts`;
const bundle = `${smokeDir}/high-fidelity-generator-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { countBlocks } from "../../src/domain/schematicModel";
import {
  generateSchematicFromBuildingGeometry,
  schematicPaletteIndexes
} from "../../src/services/buildingGeometryToSchematic";
import { applyManualBuildingGeometryCorrection } from "../../src/services/manualBuildingGeometryCorrection";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider])
  .getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const model = generateSchematicFromBuildingGeometry(geometry);
const report = model.metadata.generationReport;
assert(Boolean(report), "expected structured generation report");
assert(report!.dimensions.widthBlocks === model.width, "expected reported width");
assert(report!.dimensions.heightBlocks === model.height, "expected reported height");
assert(report!.dimensions.lengthBlocks === model.length, "expected reported length");
assert(report!.orientationDegrees === geometry.orientationDegrees, "expected orientation report");
assert(report!.floorCount === geometry.floors, "expected floor-count report");
assert(report!.floorSpacingBlocks >= 3, "expected usable floor spacing");
assert(report!.roof.shape === "hipped" && report!.roof.heightBlocks >= 3, "expected roof silhouette report");
assert(report!.facadeRhythm === "institutional-window-bays-with-floor-bands", "expected explicit facade rule");
assert(report!.entrance.side === "south" && report!.entrance.widthBlocks > 0, "expected entrance emphasis");
assert(countBlocks(model, schematicPaletteIndexes.ENTRANCE) > 0, "expected entrance blocks");
assert(countBlocks(model, schematicPaletteIndexes.ACCENT) > 0, "expected floor-band accent blocks");
assert(countBlocks(model, schematicPaletteIndexes.GLASS) > 0, "expected window bays");
assert(countBlocks(model, schematicPaletteIndexes.FLOOR) > 0, "expected interior floors");
assert(countBlocks(model, schematicPaletteIndexes.ROOF) > 0, "expected roof blocks");
assert(report!.semanticBlockCounts.entrance === countBlocks(model, schematicPaletteIndexes.ENTRANCE), "expected semantic entrance count");
assert(report!.semanticBlockCounts.windows === countBlocks(model, schematicPaletteIndexes.GLASS), "expected semantic window count");
assert(Object.values(report!.blockCounts).reduce((sum, count) => sum + count, 0) === model.blockData.length, "expected complete block counts");
assert(report!.fidelity.footprintIoU >= 0.85, "expected footprint IoU tolerance");
assert(report!.fidelity.areaErrorPercent <= 15, "expected raster area tolerance");
assert(report!.fidelity.widthErrorMeters <= 3, "expected raster width tolerance");
assert(report!.fidelity.lengthErrorMeters <= 3, "expected raster length tolerance");
assert(report!.fidelity.orientationErrorDegrees <= 1, "expected orientation tolerance");

const restyled = generateSchematicFromBuildingGeometry(geometry, {
  materialStyle: {
    wall: "minecraft:bricks",
    roof: "minecraft:oxidized_cut_copper_slab"
  }
});
assert(restyled.palette.includes("minecraft:bricks"), "expected independent wall style");
assert(restyled.palette.includes("minecraft:oxidized_cut_copper_slab"), "expected independent roof style");
assert(restyled.width === model.width && restyled.length === model.length, "expected style not to alter geometry");
assert(restyled.metadata.generationReport?.fidelity.footprintIoU === report!.fidelity.footprintIoU, "expected style not to alter fidelity");

const flatGeometry = applyManualBuildingGeometryCorrection(geometry, {
  reason: "Compare the reviewed flat-roof alternative.",
  roof: { shape: "flat" }
}).geometry;
const flat = generateSchematicFromBuildingGeometry(flatGeometry);
assert(flat.metadata.generationReport?.roof.heightBlocks === 2, "expected flat cap height");
assert(model.metadata.generationReport!.roof.heightBlocks > flat.metadata.generationReport!.roof.heightBlocks, "expected distinct roof silhouettes");

const detachedPart = geometry.footprint.slice(0, 3).map((point) => ({
  lng: point.lng + 0.0008,
  lat: point.lat + 0.00035
}));
const multiGeometry = applyManualBuildingGeometryCorrection(geometry, {
  reason: "Preserve the detached library annex massing.",
  footprintComponents: [
    { exterior: geometry.footprint, interiorRings: [] },
    { exterior: detachedPart, interiorRings: [] }
  ]
}).geometry;
const multi = generateSchematicFromBuildingGeometry(multiGeometry);
assert(countBlocks(multi, schematicPaletteIndexes.FOUNDATION) > countBlocks(model, schematicPaletteIndexes.FOUNDATION), "expected detached massing occupancy");

for (const invalidOptions of [
  { blocksPerMeter: 0 },
  { paddingBlocks: 0 },
  { fallbackHeightBlocks: 1 },
  { materialStyle: { wall: "stone" } },
  { materialStyle: { wall: "minecraft:glass" } }
]) {
  let rejected = false;
  try {
    generateSchematicFromBuildingGeometry(geometry, invalidOptions);
  } catch {
    rejected = true;
  }
  assert(rejected, "expected invalid generation parameter rejection");
}

const unsupportedRoof = structuredClone(geometry);
unsupportedRoof.roof.shape = "imaginary";
let rejectedRoof = false;
try {
  generateSchematicFromBuildingGeometry(unsupportedRoof);
} catch {
  rejectedRoof = true;
}
assert(rejectedRoof, "expected unsupported roof rejection");
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
console.log("High-fidelity Representative Building generator smoke test passed.");
