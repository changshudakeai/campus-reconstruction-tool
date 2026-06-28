import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const domain = readFileSync("src/domain/externalModel.ts", "utf8");
const service = readFileSync("src/services/externalModelReview.ts", "utf8");
const schematic = readFileSync("src/domain/schematicModel.ts", "utf8");
const writer = readFileSync("src/services/spongeSchematic.ts", "utf8");

for (const marker of [
  "ExternalModelCandidate",
  "classifyExternalModelLicense",
  "recordExternalModelReview",
  "externalModels",
  "ExternalModels"
]) {
  if (!domain.includes(marker) && !service.includes(marker) && !schematic.includes(marker) && !writer.includes(marker)) {
    throw new Error(`Missing external model review marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/external-model-review-entry.ts`;
const bundle = `${smokeDir}/external-model-review-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { gunzipSync } from "node:zlib";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import type { ExternalModelCandidate } from "../../src/domain/externalModel";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { prepareDetailedSchematicExport } from "../../src/services/detailedSchematicExport";
import { classifyExternalModelLicense, recordExternalModelReview } from "../../src/services/externalModelReview";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);
let model = generateSchematicFromBuildingGeometry(geometry);

const eligible: ExternalModelCandidate = {
  id: "3dmr-putuo-library-reference",
  source: "3dmr",
  title: "Putuo Campus Library 3D reference",
  sourceUrl: "https://example.invalid/3dmr/putuo-library",
  author: "Reference Mapper",
  linkedFeatureId: "osm-relation-5699952",
  license: {
    name: "CC-BY-SA-4.0",
    url: "https://creativecommons.org/licenses/by-sa/4.0/",
    allowsAdaptation: true,
    requiresAttribution: true,
    requiresShareAlike: true,
    allowsCommercialUse: true
  }
};

const missingLicense: ExternalModelCandidate = {
  ...eligible,
  id: "wikidata-missing-license",
  source: "wikidata",
  title: "Unlicensed mesh",
  license: null
};

const noDerivatives: ExternalModelCandidate = {
  ...eligible,
  id: "wikidata-no-derivatives",
  source: "wikidata",
  title: "No derivatives mesh",
  license: {
    name: "CC-BY-ND-4.0",
    url: "https://creativecommons.org/licenses/by-nd/4.0/",
    allowsAdaptation: false,
    requiresAttribution: true,
    noDerivatives: true
  }
};

assert(classifyExternalModelLicense(eligible).eligibility === "eligible", "expected CC-BY-SA model to be eligible");
assert(classifyExternalModelLicense(eligible).obligations.some((item) => item.includes("attribution")), "expected attribution obligation");
assert(classifyExternalModelLicense(missingLicense).eligibility === "blocked", "expected missing license to block use");
assert(classifyExternalModelLicense(noDerivatives).eligibility === "blocked", "expected no-derivatives license to block use");

let rejectedPrimary = false;
try {
  recordExternalModelReview(model, noDerivatives, "eligible_primary", "Looks detailed.");
} catch {
  rejectedPrimary = true;
}
assert(rejectedPrimary, "expected blocked license to reject primary use");

model = recordExternalModelReview(model, eligible, "eligible_primary", "Matches the OSM relation and has an adaptation-friendly license.");
const exported = prepareDetailedSchematicExport(model);
const companion = JSON.parse(exported.provenanceJson);
const nbtText = new TextDecoder().decode(gunzipSync(exported.bytes));

assert(companion.provenance.externalModels.length === 1, "expected external model provenance");
assert(companion.provenance.externalModels[0].candidate.author === "Reference Mapper", "expected author provenance");
assert(companion.provenance.externalModels[0].licenseReview.obligations.length >= 2, "expected license obligations");
assert(companion.provenance.externalModels[0].attribution.includes("CC-BY-SA-4.0"), "expected attribution text");
assert(nbtText.includes("ExternalModels"), "expected external model metadata in NBT");
assert(nbtText.includes("Reference Mapper"), "expected author in NBT metadata");
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
console.log("External model review smoke test passed.");
