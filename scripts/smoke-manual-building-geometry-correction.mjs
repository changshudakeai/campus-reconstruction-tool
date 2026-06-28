import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const service = readFileSync("src/services/manualBuildingGeometryCorrection.ts", "utf8");
const styles = readFileSync("src/styles.css", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "ManualBuildingGeometryPanel",
  "applyManualBuildingGeometryCorrection",
  "manual_correction",
  "useReviewedSlotFootprint",
  "manual-geometry-panel"
]) {
  if (!app.includes(marker) && !service.includes(marker) && !styles.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing manual correction marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/manual-building-correction-entry.ts`;
const bundle = `${smokeDir}/manual-building-correction-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { MinimalArnisAdapter, type BuildingGeometryProvider } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { applyManualBuildingGeometryCorrection } from "../../src/services/manualBuildingGeometryCorrection";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected representative slot");
const original = await new MinimalArnisAdapter([putuoLibraryFixtureProvider])
  .getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const originalSchematic = generateSchematicFromBuildingGeometry(original);
const result = applyManualBuildingGeometryCorrection(original, {
  reason: "Reviewed slot footprint and measured library massing.",
  footprint: slot!.geometry.points,
  heightM: 30,
  floors: 7,
  roof: { shape: "flat", material: "copper" },
  facade: { material: "red_brick", color: "dark_red" }
});
const corrected = result.geometry;
const correctedSchematic = generateSchematicFromBuildingGeometry(corrected);

assert(result.correctedFields.includes("footprint"), "expected footprint correction");
assert(result.correctedFields.includes("heightM"), "expected height correction");
assert(corrected.heightM === 30, "expected manual height override");
assert(corrected.floors === 7, "expected manual floor override");
assert(corrected.roof.shape === "flat", "expected manual roof override");
assert(corrected.roof.material === "copper", "expected manual roof material");
assert(corrected.roof.orientation === original.roof.orientation, "expected untouched roof orientation");
assert(corrected.facade.material === "red_brick", "expected manual facade override");
assert(corrected.confidence.footprint === "manual", "expected manual footprint confidence");
assert(corrected.confidence.height === "manual", "expected manual height confidence");
assert(corrected.confidence.roof === "manual", "expected manual roof confidence");
assert(corrected.provenance.usedSources.at(-1) === "manual_correction", "expected manual source last");
assert(corrected.provenance.notes.at(-1)?.includes("heightM"), "expected correction provenance note");
assert(corrected.provenance.notes.at(-1)?.includes("Reviewed slot footprint"), "expected retained correction reason");
assert(corrected.provenance.corrections.length === 1, "expected one structured correction record");
assert(corrected.provenance.corrections[0].reason.includes("measured library massing"), "expected structured reason");
assert(corrected.provenance.corrections[0].before.heightM === 22, "expected before value provenance");
assert(corrected.provenance.corrections[0].after.heightM === 30, "expected after value provenance");
assert(corrected.provenance.missingFields.length === 0, "expected missing fields recomputed");
assert(corrected.footprintComponents.length === 1, "expected corrected footprint component");
assert(corrected.orientationDegrees !== 0, "expected measured footprint orientation");
assert(corrected.scale.areaSquareMeters > 1, "expected measured real-world scale");
assert(corrected.floorSpacingMeters === 30 / 7, "expected reconciled floor spacing");
assert(corrected.validation.valid, "expected validated corrected geometry");
assert(corrected.provenance.generationAssumptions.some((item) => item.field === "floorSpacingMeters"), "expected floor-spacing assumption");
assert(correctedSchematic.height !== originalSchematic.height, "expected regenerated schematic height");
assert(correctedSchematic.metadata.provenance?.corrections.length === 1, "expected correction provenance retained during regeneration");
assert(correctedSchematic.metadata.provenance?.observations.length === original.provenance.observations.length, "expected source observations retained during regeneration");

const second = applyManualBuildingGeometryCorrection(corrected, {
  reason: "Adjusted roof material after facade review.",
  roof: { material: "oxidized_copper" }
});
assert(second.geometry.provenance.corrections.length === 2, "expected accumulated correction history");
assert(second.geometry.provenance.corrections[0].reason.includes("massing"), "expected first correction retained");

const detachedPart = slot!.geometry.points.slice(0, 3).map((point) => ({
  lng: point.lng + 0.0007,
  lat: point.lat + 0.0003
}));
const multiPart = applyManualBuildingGeometryCorrection(corrected, {
  reason: "Preserved the reviewed detached library annex.",
  footprintComponents: [
    { exterior: slot!.geometry.points, interiorRings: [] },
    { exterior: detachedPart, interiorRings: [] }
  ]
});
assert(multiPart.geometry.footprintComponents.length === 2, "expected detached footprint component retention");
assert(multiPart.geometry.validation.componentCount === 2, "expected multi-component validation");
assert(multiPart.geometry.validation.valid, "expected valid multi-component geometry");

const incompleteProvider: BuildingGeometryProvider = {
  source: "existing_project",
  async fetchBuildingGeometry() {
    return { footprint: slot!.geometry.points };
  }
};
const incomplete = await new MinimalArnisAdapter([incompleteProvider]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);
assert(incomplete.provenance.missingFields.includes("heightM"), "expected first-class missing height");
assert(incomplete.provenance.generationAssumptions.some((item) => item.field === "heightM"), "expected explicit height fallback assumption");
assert(incomplete.provenance.generationAssumptions.some((item) => item.field === "floors"), "expected explicit floor fallback assumption");
assert(incomplete.provenance.generationAssumptions.some((item) => item.field === "roof.shape"), "expected explicit roof fallback assumption");

const noOp = applyManualBuildingGeometryCorrection(original, {});
assert(noOp.correctedFields.length === 0, "expected empty correction to be a no-op");
assert(!noOp.geometry.provenance.usedSources.includes("manual_correction"), "expected no manual source for no-op");

let rejectedInvalidHeight = false;
try {
  applyManualBuildingGeometryCorrection(original, { reason: "Invalid test", heightM: -1 });
} catch {
  rejectedInvalidHeight = true;
}
assert(rejectedInvalidHeight, "expected invalid height rejection");

let rejectedInvalidFloors = false;
try {
  applyManualBuildingGeometryCorrection(original, { reason: "Invalid test", floors: 2.5 });
} catch {
  rejectedInvalidFloors = true;
}
assert(rejectedInvalidFloors, "expected invalid floor rejection");

let rejectedMissingReason = false;
try {
  applyManualBuildingGeometryCorrection(original, { heightM: 24 });
} catch {
  rejectedMissingReason = true;
}
assert(rejectedMissingReason, "expected correction reason requirement");

let rejectedImplausibleFloorSpacing = false;
try {
  applyManualBuildingGeometryCorrection(original, {
    reason: "Implausible geometry test",
    heightM: 10,
    floors: 8
  });
} catch {
  rejectedImplausibleFloorSpacing = true;
}
assert(rejectedImplausibleFloorSpacing, "expected implausible floor spacing rejection");

let rejectedInvalidInteriorRing = false;
try {
  applyManualBuildingGeometryCorrection(original, {
    reason: "Invalid ring test",
    footprintComponents: [{
      exterior: slot!.geometry.points,
      interiorRings: [[slot!.geometry.points[0], slot!.geometry.points[1]]]
    }]
  });
} catch {
  rejectedInvalidInteriorRing = true;
}
assert(rejectedInvalidInteriorRing, "expected invalid interior-ring rejection");
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
console.log("Manual Building Geometry correction smoke test passed.");
