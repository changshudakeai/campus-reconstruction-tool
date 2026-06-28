import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const modelDomain = readFileSync("src/domain/schematicModel.ts", "utf8");
const axiomService = readFileSync("src/services/axiomAcceptance.ts", "utf8");
const exportService = readFileSync("src/services/detailedSchematicExport.ts", "utf8");
const writer = readFileSync("src/services/spongeSchematic.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");

for (const marker of [
  "AxiomAcceptanceRecord",
  "checkAxiomPlacement",
  "recordAxiomAcceptance",
  "axiomPlacement",
  "AxiomAcceptance",
  "axiomAcceptance"
]) {
  if (!modelDomain.includes(marker) && !axiomService.includes(marker) && !exportService.includes(marker) && !writer.includes(marker) && !app.includes(marker)) {
    throw new Error(`Missing Axiom acceptance marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/axiom-acceptance-entry.ts`;
const bundle = `${smokeDir}/axiom-acceptance-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { gunzipSync } from "node:zlib";
import { createBuildingSlotHandoffProvider } from "../../src/adapters/buildingSlotHandoffProvider";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";
import { checkAxiomPlacement, recordAxiomAcceptance } from "../../src/services/axiomAcceptance";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected representative slot");
const target = buildingSlotToBuildingTarget(slot!);
const geometry = await new MinimalArnisAdapter([
  putuoLibraryFixtureProvider,
  createBuildingSlotHandoffProvider(slot!)
]).getBuildingGeometry(target);
let model = generateSchematicFromBuildingGeometry(geometry);

const placement = checkAxiomPlacement(model);
assert(placement.origin.x === 0 && placement.origin.y === 0 && placement.origin.z === 0, "expected stable schematic origin");
assert(placement.blocksPerMeter !== null, "expected scale in placement check");
assert(placement.orientationDegrees !== null, "expected orientation in placement check");
assert(placement.expectedSlotDimensions.widthBlocks !== null, "expected Building Slot width check");
assert(placement.widthDeltaBlocks !== null, "expected width delta");
assert(["fits", "exceeds", "unknown"].includes(placement.status), "expected placement status");

let rejectedFailedWithoutNotes = false;
try {
  recordAxiomAcceptance(model, {
    minecraftVersion: "1.21.4",
    axiomVersion: "4.5.0",
    importResult: "failed",
    orientationCheck: "failed",
    scaleCheck: "pending",
    paletteCheck: "pending",
    blockPlacementCheck: "pending",
    screenshots: [],
    correctionNotes: []
  });
} catch {
  rejectedFailedWithoutNotes = true;
}
assert(rejectedFailedWithoutNotes, "expected failed import to require correction notes");

let rejectedSuccessWithoutScreenshot = false;
try {
  recordAxiomAcceptance(model, {
    minecraftVersion: "1.21.4",
    axiomVersion: "4.5.0",
    importResult: "succeeded",
    orientationCheck: "passed",
    scaleCheck: "passed",
    paletteCheck: "passed",
    blockPlacementCheck: "passed",
    screenshots: [],
    correctionNotes: []
  });
} catch {
  rejectedSuccessWithoutScreenshot = true;
}
assert(rejectedSuccessWithoutScreenshot, "expected successful import to require screenshots");

model = recordAxiomAcceptance(model, {
  minecraftVersion: "1.21.4",
  axiomVersion: "4.5.0",
  importResult: "succeeded",
  orientationCheck: "passed",
  scaleCheck: "passed",
  paletteCheck: "passed",
  blockPlacementCheck: "passed",
  screenshots: [{ view: "axiom", uri: "artifacts/axiom/putuo-library-perspective.png", note: "Perspective import view" }],
  correctionNotes: []
});

const exported = prepareDetailedSchematicExport(model);
const companion = JSON.parse(exported.provenanceJson);
const nbtText = new TextDecoder().decode(gunzipSync(exported.bytes));

assert(companion.schematic.axiomPlacement.status === placement.status, "expected placement in export companion");
assert(companion.schematic.axiomAcceptance.importResult === "succeeded", "expected acceptance summary in export companion");
assert(companion.provenance.axiomAcceptance.minecraftVersion === "1.21.4", "expected Minecraft version provenance");
assert(companion.provenance.axiomAcceptance.axiomVersion === "4.5.0", "expected Axiom version provenance");
assert(companion.provenance.axiomAcceptance.screenshots[0].uri.includes("putuo-library"), "expected screenshot provenance");
assert(nbtText.includes("AxiomAcceptance"), "expected Axiom acceptance in NBT metadata");
assert(nbtText.includes("1.21.4"), "expected Minecraft version in NBT metadata");
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
console.log("Axiom acceptance smoke test passed.");
