import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const service = readFileSync("src/services/buildingSlotTarget.ts", "utf8");
const styles = readFileSync("src/styles.css", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "buildingSlotToBuildingTarget",
  "DetailedSlotSummary",
  "selectedBuildingSlot",
  "adapterTargetCenter",
  "detailed-slot-summary"
]) {
  if (!app.includes(marker) && !service.includes(marker) && !styles.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing Detailed Mode slot selection marker: ${marker}`);
  }
}

if (app.includes("DetailedModePanel slotName")) {
  throw new Error("DetailedModePanel should consume a BuildingSlot, not a slotName prop.");
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/detailed-mode-slot-selection-entry.ts`;
const bundle = `${smokeDir}/detailed-mode-slot-selection-bundle.mjs`;
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
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected representative Building Slot");

const target = buildingSlotToBuildingTarget(slot!);
const bounds = slot!.dimensions.bounds;
const expectedLng = (bounds.minLng + bounds.maxLng) / 2;
const expectedLat = (bounds.minLat + bounds.maxLat) / 2;

assert(target.name === slot!.name, "expected target name from Building Slot");
assert(target.campus === "ECNU Putuo Campus", "expected target campus");
assert(Math.abs(target.approximateCenter.lng - expectedLng) < 0.000001, "expected target center lng from slot bounds");
assert(Math.abs(target.approximateCenter.lat - expectedLat) < 0.000001, "expected target center lat from slot bounds");
assert(target.aliases.includes(slot!.name), "expected slot name alias");
assert(target.aliases.includes(slot!.provenance.rawId), "expected provenance rawId alias");
assert(target.aliases.includes("图书馆"), "expected Chinese library alias");

const adapter = new MinimalArnisAdapter([putuoLibraryFixtureProvider]);
const geometry = await adapter.getBuildingGeometry(target);
assert(geometry.target.name === slot!.name, "expected adapter target to preserve selected slot name");
assert(geometry.target.approximateCenter.lng === target.approximateCenter.lng, "expected adapter target center");
assert(geometry.buildingName === slot!.name, "expected geometry building name from selected slot");
assert(geometry.footprint.length >= 3, "expected generated Building Geometry footprint");
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
console.log("Detailed Mode slot selection smoke test passed.");
