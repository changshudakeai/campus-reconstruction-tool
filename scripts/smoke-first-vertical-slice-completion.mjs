import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const app = readFileSync("src/App.tsx", "utf8");
const liveProviders = readFileSync("src/services/liveMapProviders.ts", "utf8");
const buildingProviders = readFileSync("src/adapters/overtureBuildingGeometryProvider.ts", "utf8");
const legacyReuse = readFileSync("docs/legacy-reuse.md", "utf8");

for (const dependency of ["react", "three", "@tauri-apps/api"]) {
  if (!packageJson.dependencies[dependency]) throw new Error(`Missing Modern App Shell dependency: ${dependency}`);
}
for (const marker of ["FoundationModePanel", "DetailedModePanel", "GaodePoiCandidateProvider", "OverpassCandidateProvider", "OvertureBuildingGeometryProvider"]) {
  if (!app.includes(marker) && !liveProviders.includes(marker) && !buildingProviders.includes(marker)) {
    throw new Error(`Missing completion marker: ${marker}`);
  }
}

for (const legacyPath of [
  "../ecnu-mc-replication/cli/mc_build_tools/schematic.py",
  "../ecnu-mc-replication/web/js/scene.js",
  "../ecnu-mc-replication/data/block_colors.json"
]) {
  if (!legacyReuse.includes(legacyPath)) {
    throw new Error(`Missing legacy lineage documentation: ${legacyPath}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/first-vertical-slice-completion-entry.ts`;
const bundle = `${smokeDir}/first-vertical-slice-completion-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { createBuildingSlotHandoffProvider } from "../../src/adapters/buildingSlotHandoffProvider";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { selectRepresentativeBuildingSlot } from "../../src/domain/foundationManifest";
import { acceptCandidate, buildFoundationManifestFromReviews } from "../../src/services/candidateReview";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";
import { exportFoundationManifestJson, parseFoundationManifestJson } from "../../src/services/foundationManifestExport";
import { generateFoundationSchematicFromManifest } from "../../src/services/foundationManifestToSchematic";
import { applyFoundationStyle, DEFAULT_FOUNDATION_STYLE } from "../../src/services/foundationStyle";
import { OnlineMapQueryService, putuoFixtureCandidateProviders } from "../../src/services/onlineMapQuery";
import { listInspectableBlocks, replaceAllMatchingBlocks } from "../../src/services/schematicEditing";
import { writeSpongeV2Schematic } from "../../src/services/spongeSchematic";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const queryService = new OnlineMapQueryService(putuoFixtureCandidateProviders);
const firstQuery = await queryService.queryPutuoCampus();
const cachedQuery = await queryService.queryPutuoCampus();
assert(firstQuery.candidates.length >= 4, "expected renderable offline Map Candidates");
assert(cachedQuery.providerDebug.every((entry) => entry.cacheStatus === "hit"), "expected cached fixture query");

const reviewed = firstQuery.candidates.map(acceptCandidate);
const unstyledManifest = buildFoundationManifestFromReviews(
  {
    schemaVersion: "0.1.0",
    target: { campus: "ECNU Putuo Campus", representativeBuilding: "Putuo Campus Library" },
    mapFeatures: [],
    buildingSlots: [],
    representativeBuildingSlotId: null
  },
  reviewed
);
const manifest = applyFoundationStyle(unstyledManifest, DEFAULT_FOUNDATION_STYLE);
const manifestExport = exportFoundationManifestJson(manifest);
const parsedManifest = parseFoundationManifestJson(manifestExport.json);
const foundation = generateFoundationSchematicFromManifest(parsedManifest, { blocksPerMeter: 0.08 });
const foundationBytes = writeSpongeV2Schematic(foundation.model);
assert(foundationBytes[0] === 10, "expected Foundation Sponge v2 output");
assert(parsedManifest.mapFeatures.length === reviewed.length, "expected reviewed Map Features");

const slot = selectRepresentativeBuildingSlot(parsedManifest);
assert(Boolean(slot), "expected Putuo Library representative slot");
const geometry = await new MinimalArnisAdapter([
  putuoLibraryFixtureProvider,
  createBuildingSlotHandoffProvider(slot!)
]).getBuildingGeometry(buildingSlotToBuildingTarget(slot!));
assert(geometry.footprint.length >= 6, "expected non-rectangular Building Geometry");

const generated = generateSchematicFromBuildingGeometry(geometry);
const visibleBlocks = listInspectableBlocks(generated);
assert(visibleBlocks.length > 0, "expected previewable blocks");
const replacement = replaceAllMatchingBlocks(
  generated,
  "minecraft:stone_bricks",
  "minecraft:mossy_stone_bricks"
);
assert(replacement.replacedCount > 0, "expected Batch Block Replacement");

const detailedExport = prepareDetailedSchematicExport(replacement.model);
const provenance = JSON.parse(detailedExport.provenanceJson);
assert(detailedExport.bytes[0] === 0x1f && detailedExport.bytes[1] === 0x8b, "expected gzip-compressed Detailed Sponge v2 output");
assert(provenance.provenance.handoff.foundationSlotId === slot!.id, "expected handoff provenance");
assert(provenance.provenance.blockReplacements.length === 1, "expected replacement provenance");
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
console.log("First Vertical Slice completion smoke test passed.");
