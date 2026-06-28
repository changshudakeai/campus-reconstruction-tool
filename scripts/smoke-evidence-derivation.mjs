import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const derivation = readFileSync("src/services/buildingGeometryDerivation.ts", "utf8");
const domain = readFileSync("src/domain/buildingGeometry.ts", "utf8");
for (const marker of [
  "arnis-explicit-height-overrides-levels",
  "arnis-roof-shape-synonyms",
  "fieldDecisions",
  "contradictions",
  "qualityScore"
]) {
  if (!derivation.includes(marker) && !domain.includes(marker)) {
    throw new Error(`Missing evidence derivation marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/evidence-derivation-entry.ts`;
const bundle = `${smokeDir}/evidence-derivation-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { MinimalArnisAdapter, type BuildingGeometryProvider } from "../../src/adapters/minimalArnisAdapter";
import { createBuildingGeometryObservation } from "../../src/services/buildingObservation";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const overtureFootprint = [
  { lng: 121.4085, lat: 31.2284 },
  { lng: 121.4091, lat: 31.2285 },
  { lng: 121.4094, lat: 31.2281 },
  { lng: 121.4087, lat: 31.2279 }
];
const osmFootprint = overtureFootprint.map((point, index) => ({
  lng: point.lng + 0.00035 + (index === 0 ? 0.00001 : 0),
  lat: point.lat + 0.00015
}));

function observation(
  id: string,
  source: "overture" | "osm_overpass",
  footprint: typeof overtureFootprint,
  tags: Record<string, string>
) {
  return createBuildingGeometryObservation({
    id,
    source,
    sourceFeatureId: id,
    name: source === "overture" ? "Putuo Campus Library" : "Nearby academic building",
    tags,
    components: [{ exterior: footprint, interiorRings: [] }]
  });
}

const overtureProvider: BuildingGeometryProvider = {
  source: "overture",
  async fetchBuildingGeometry() {
    return {
      footprint: overtureFootprint,
      heightM: 24,
      roof: { shape: "gable" },
      confidence: { footprint: "high", height: "high", roof: "high" },
      observations: [observation("overture:library", "overture", overtureFootprint, {
        height: "26 m",
        min_height: "2m",
        building: "university",
        "roof:shape": "gable"
      })]
    };
  }
};

const osmProvider: BuildingGeometryProvider = {
  source: "osm_overpass",
  async fetchBuildingGeometry() {
    return {
      footprint: osmFootprint,
      heightM: 20,
      floors: 6,
      roof: { material: "tile" },
      facade: { material: "brick" },
      confidence: { footprint: "medium", height: "medium", floors: "high", roof: "high", facade: "high" },
      observations: [observation("osm:library", "osm_overpass", osmFootprint, {
        "building:levels": "6",
        building: "library"
      })]
    };
  }
};

const manualProvider: BuildingGeometryProvider = {
  source: "manual_correction",
  async fetchBuildingGeometry() {
    return {
      heightM: 18,
      facade: { color: "warm_cream" },
      confidence: { height: "manual", facade: "manual" }
    };
  }
};

const geometry = await new MinimalArnisAdapter([
  overtureProvider,
  osmProvider,
  manualProvider
]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);

const decision = (field: string) => geometry.provenance.fieldDecisions.find((item) => item.field === field);
assert(decision("footprint")?.source === "overture", "expected Overture footprint evidence");
assert(decision("floors")?.source === "osm_overpass", "expected OSM floor evidence");
assert(decision("roof.material")?.source === "osm_overpass", "expected OSM roof material evidence");
assert(decision("facade.material")?.source === "osm_overpass", "expected OSM facade material evidence");
assert(decision("facade.color")?.source === "manual_correction", "expected manual facade color evidence");
assert(decision("heightM")?.source === "manual_correction", "expected stronger manual height evidence");
assert(geometry.heightM === 18, "expected Arnis raw-tag interpretation not to overwrite stronger manual height evidence");
assert(geometry.roof.shape === "gabled", "expected Arnis roof synonym normalization");
assert(decision("roof.shape")?.ruleId === "arnis-roof-shape-synonyms", "expected explainable roof rule");

const heightConflict = geometry.provenance.contradictions.find((item) => item.field === "heightM");
assert(heightConflict?.candidates.length === 3, "expected all contradictory height observations");
assert(heightConflict?.candidates.every((item) => item.confidence && item.qualityScore > 0), "expected contradiction confidence and quality");
assert(heightConflict?.candidates.some((item) => item.source === "manual_correction"), "expected manual attribution in contradiction");

const derived = geometry.provenance.observations.find((item) => item.source === "arnis_derived");
assert(Boolean(derived), "expected distinct Arnis-derived observation");
assert(derived?.id.startsWith("arnis-derived:"), "expected traceable Arnis-derived observation ID");
assert(geometry.provenance.arnisRuleDecisions.every((item) => item.upstreamReference.includes("7d2a0e")), "expected pinned upstream rule lineage");
assert(geometry.provenance.arnisRuleDecisions.some((item) => item.ruleId === "arnis-school-institutional-bands"), "expected institutional facade rule");
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
console.log("Evidence-driven Building Geometry derivation smoke test passed.");
