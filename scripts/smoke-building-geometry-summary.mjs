import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const styles = readFileSync("src/styles.css", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "BuildingGeometrySummary",
  "geometry-facts-grid",
  "fieldConfidence",
  "geometryProvenance",
  "missingFields",
  "usedSources"
]) {
  if (!app.includes(marker) && !styles.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing Building Geometry summary marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/building-geometry-summary-entry.ts`;
const bundle = `${smokeDir}/building-geometry-summary-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { createBuildingSlotHandoffProvider } from "../../src/adapters/buildingSlotHandoffProvider";
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected representative library slot");
const target = buildingSlotToBuildingTarget(slot!);
const slotProvider = createBuildingSlotHandoffProvider(slot!);

const completeGeometry = await new MinimalArnisAdapter([
  putuoLibraryFixtureProvider,
  slotProvider
]).getBuildingGeometry(target);

assert(completeGeometry.footprint.length === 6, "expected non-rectangular Overture footprint");
assert(completeGeometry.heightM === 22, "expected height hint");
assert(completeGeometry.floors === 5, "expected floor count");
assert(completeGeometry.roof.shape === "hipped", "expected roof shape hint");
assert(completeGeometry.roof.material === "tile", "expected roof material hint");
assert(completeGeometry.roof.orientation === "long_axis", "expected roof orientation hint");
assert(completeGeometry.facade.material === "stone", "expected facade material hint");
assert(completeGeometry.facade.color === "warm_light", "expected facade color hint");
assert(completeGeometry.confidence.footprint === "high", "expected footprint confidence");
assert(completeGeometry.confidence.height === "medium", "expected height confidence");
assert(completeGeometry.confidence.floors === "medium", "expected floors confidence");
assert(completeGeometry.confidence.roof === "medium", "expected roof confidence");
assert(completeGeometry.confidence.facade === "low", "expected facade confidence");
assert(completeGeometry.provenance.sourcePriority[0] === "overture", "expected Overture priority");
assert(completeGeometry.provenance.usedSources.includes("existing_project"), "expected slot source");
assert(completeGeometry.provenance.missingFields.length === 0, "expected complete fixture fields");
assert(completeGeometry.provenance.notes.length >= 2, "expected explainable source notes");

const fallbackGeometry = await new MinimalArnisAdapter([slotProvider]).getBuildingGeometry(target);
assert(fallbackGeometry.confidence.height === "missing", "expected missing height confidence");
assert(fallbackGeometry.provenance.missingFields.includes("heightM"), "expected missing height field");
assert(fallbackGeometry.provenance.missingFields.includes("roof.shape"), "expected missing roof field");
assert(fallbackGeometry.provenance.missingFields.includes("facade"), "expected missing facade field");
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
console.log("Building Geometry summary smoke test passed.");
