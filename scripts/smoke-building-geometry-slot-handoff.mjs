import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const domain = readFileSync("src/domain/buildingGeometry.ts", "utf8");
const adapter = readFileSync("src/adapters/minimalArnisAdapter.ts", "utf8");
const provider = readFileSync("src/adapters/buildingSlotHandoffProvider.ts", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "BuildingGeometryHandoff",
  "createBuildingSlotHandoffProvider",
  "existing_project",
  "provenance.handoff",
  "buildingGeometryHandoff"
]) {
  if (
    !app.includes(marker) &&
    !domain.includes(marker) &&
    !adapter.includes(marker) &&
    !provider.includes(marker) &&
    !i18n.includes(marker)
  ) {
    throw new Error(`Missing Building Geometry handoff marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/building-geometry-slot-handoff-entry.ts`;
const bundle = `${smokeDir}/building-geometry-slot-handoff-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import { createBuildingSlotHandoffProvider } from "../../src/adapters/buildingSlotHandoffProvider";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected representative Building Slot");
const target = buildingSlotToBuildingTarget(slot!);
const slotProvider = createBuildingSlotHandoffProvider(slot!);

const mergedAdapter = new MinimalArnisAdapter([putuoLibraryFixtureProvider, slotProvider]);
const mergedGeometry = await mergedAdapter.getBuildingGeometry(target);

assert(mergedGeometry.buildingName === slot!.name, "expected selected slot building name");
assert(mergedGeometry.provenance.usedSources.includes("overture"), "expected Overture source");
assert(mergedGeometry.provenance.usedSources.includes("existing_project"), "expected Foundation handoff source");
assert(mergedGeometry.provenance.handoff?.foundationSlotId === slot!.id, "expected handoff slot id");
assert(mergedGeometry.provenance.handoff?.sourceFeatureId === slot!.sourceFeatureId, "expected handoff source feature id");
assert(mergedGeometry.provenance.handoff?.selectedBlock === slot!.selectedBlock, "expected handoff selected block");
assert(mergedGeometry.provenance.handoff?.rawId === slot!.provenance.rawId, "expected handoff rawId");
assert(
  mergedGeometry.provenance.handoff?.approximateWidthMeters === slot!.dimensions.approximateWidthMeters,
  "expected handoff width"
);
assert(
  mergedGeometry.provenance.notes.some((note) => note.includes(slot!.id)),
  "expected handoff note to mention slot id"
);
assert(mergedGeometry.heightM === 22, "expected Overture fixture height to remain preferred");
assert(mergedGeometry.roof.shape === "hipped", "expected Overture fixture roof to remain preferred");

const fallbackAdapter = new MinimalArnisAdapter([slotProvider]);
const fallbackGeometry = await fallbackAdapter.getBuildingGeometry(target);
assert(fallbackGeometry.provenance.usedSources.length === 1, "expected one fallback source");
assert(fallbackGeometry.provenance.usedSources[0] === "existing_project", "expected slot handoff fallback source");
assert(fallbackGeometry.footprint.length === slot!.geometry.points.length, "expected slot footprint fallback");
assert(fallbackGeometry.confidence.footprint === slot!.confidence, "expected slot footprint confidence");
assert(fallbackGeometry.provenance.missingFields.includes("heightM"), "expected missing height without Overture");
assert(fallbackGeometry.provenance.handoff?.foundationSlotId === slot!.id, "expected fallback handoff");
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
console.log("Building Geometry slot handoff smoke test passed.");
