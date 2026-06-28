import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const providerSource = readFileSync("src/adapters/overpassBuildingGeometryProvider.ts", "utf8");
const defaultsSource = readFileSync("src/adapters/overtureBuildingGeometryProvider.ts", "utf8");
const adapterSource = readFileSync("src/adapters/minimalArnisAdapter.ts", "utf8");

for (const marker of [
  "OverpassBuildingGeometryProvider",
  "osm_overpass",
  "withBuildingGeometryFailureAsEmpty",
  'value !== "missing"'
]) {
  if (!providerSource.includes(marker) && !defaultsSource.includes(marker) && !adapterSource.includes(marker)) {
    throw new Error(`Missing OSM building fallback marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/overpass-building-fallback-entry.ts`;
const bundle = `${smokeDir}/overpass-building-fallback-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { createBuildingSlotHandoffProvider } from "../../src/adapters/buildingSlotHandoffProvider";
import { MinimalArnisAdapter, type BuildingGeometryProvider } from "../../src/adapters/minimalArnisAdapter";
import { withBuildingGeometryFailureAsEmpty } from "../../src/adapters/overtureBuildingGeometryProvider";
import { OverpassBuildingGeometryProvider } from "../../src/adapters/overpassBuildingGeometryProvider";
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";
import {
  createBuildingGeometryObservation,
  pairwiseObservationOverlaps
} from "../../src/services/buildingObservation";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder);
assert(Boolean(slot), "expected representative slot");
const target = buildingSlotToBuildingTarget(slot!);
let requestBody = "";
const osmProvider = new OverpassBuildingGeometryProvider({
  endpoint: "https://example.test/overpass",
  fetchJson: async (_input, init) => {
    requestBody = String(init?.body);
    return new Response(JSON.stringify({
      elements: [
        {
          type: "relation",
          id: 9988,
          tags: {
            building: "university",
            name: "华东师范大学普陀校区图书馆",
            height: "26 m",
            "building:levels": "6",
            "roof:shape": "hipped",
            "roof:material": "tile",
            "building:material": "brick",
            "building:colour": "red"
          },
          members: [
            {
              type: "way", ref: 100, role: "outer",
              geometry: [
                { lon: 121.4085, lat: 31.2284 },
                { lon: 121.4091, lat: 31.2286 },
                { lon: 121.4094, lat: 31.2283 },
                { lon: 121.4092, lat: 31.2279 },
                { lon: 121.4087, lat: 31.2278 },
                { lon: 121.4085, lat: 31.2284 }
              ]
            },
            {
              type: "way", ref: 101, role: "inner",
              geometry: [
                { lon: 121.4088, lat: 31.2282 },
                { lon: 121.4089, lat: 31.2282 },
                { lon: 121.4089, lat: 31.2281 },
                { lon: 121.4088, lat: 31.2282 }
              ]
            },
            {
              type: "way", ref: 102, role: "outer",
              geometry: [
                { lon: 121.4095, lat: 31.2282 },
                { lon: 121.4096, lat: 31.2282 },
                { lon: 121.4096, lat: 31.2281 },
                { lon: 121.4095, lat: 31.2282 }
              ]
            }
          ]
        }
      ]
    }), { status: 200, headers: { "content-type": "application/json" } });
  }
});

const incompleteOverture: BuildingGeometryProvider = {
  source: "overture",
  async fetchBuildingGeometry() {
    const components = [{ exterior: slot!.geometry.points, interiorRings: [] }];
    return {
      footprint: slot!.geometry.points,
      confidence: {
        footprint: "high",
        height: "missing",
        floors: "missing",
        roof: "missing",
        facade: "missing"
      },
      observations: [createBuildingGeometryObservation({
        id: "overture:test-footprint",
        source: "overture",
        sourceFeatureId: "test-footprint",
        name: "Putuo Campus Library",
        components
      })],
      notes: ["Overture feature had only a footprint."]
    };
  }
};

const geometry = await new MinimalArnisAdapter([
  incompleteOverture,
  osmProvider,
  createBuildingSlotHandoffProvider(slot!)
]).getBuildingGeometry(target);

assert(requestBody.includes("%5B%22building%22%5D"), "expected encoded building query");
assert(geometry.footprint.length === slot!.geometry.points.length, "expected Overture footprint priority");
assert(geometry.heightM === 26, "expected OSM height fallback");
assert(geometry.floors === 6, "expected OSM floors fallback");
assert(geometry.roof.shape === "hipped", "expected OSM roof fallback");
assert(geometry.facade.material === "brick", "expected OSM facade fallback");
assert(geometry.confidence.height === "medium", "expected OSM height confidence after missing Overture value");
assert(geometry.provenance.usedSources.join(",") === "overture,osm_overpass,existing_project", "expected source order");
assert(geometry.provenance.notes.some((note) => note.includes("osm:relation:9988")), "expected OSM raw ID provenance");
for (const source of ["overture", "osm_overpass", "existing_project", "arnis_derived"]) {
  assert(
    geometry.provenance.observations.some((observation) => observation.source === source),
    "expected separate " + source + " observation"
  );
}
const osmObservation = geometry.provenance.observations.find((item) => item.source === "osm_overpass");
assert(Boolean(osmObservation), "expected OSM observation");
assert(osmObservation!.components.length === 2, "expected detached OSM relation outer parts");
assert(osmObservation!.components[0].interiorRings.length === 1, "expected OSM relation inner ring");
assert(osmObservation!.tags["building:levels"] === "6", "expected OSM tags retained");
assert(osmObservation!.metrics.areaSquareMeters > 0, "expected measured OSM area");
assert(osmObservation!.metrics.pointCount === 11, "expected complete point count");
const overlaps = pairwiseObservationOverlaps(geometry.provenance.observations);
const expectedPairCount = geometry.provenance.observations.length * (geometry.provenance.observations.length - 1) / 2;
assert(overlaps.length === expectedPairCount, "expected all pairwise comparisons");
assert(overlaps.every((item) => item.score >= 0 && item.score <= 1), "expected normalized overlap scores");

const malformedProvider = new OverpassBuildingGeometryProvider({
  endpoint: "https://example.test/overpass",
  fetchJson: async () => new Response(JSON.stringify({
    elements: [{ type: "relation", id: 7, tags: { building: "yes" }, members: [{ type: "way", ref: 8, role: "outer", geometry: [{ lon: 121.4, lat: 31.2 }] }] }]
  }), { status: 200, headers: { "content-type": "application/json" } })
});
assert(await malformedProvider.fetchBuildingGeometry(target) === null, "expected malformed relation to be ignored");

const failingOsm = withBuildingGeometryFailureAsEmpty(new OverpassBuildingGeometryProvider({
  endpoint: "https://example.test/overpass",
  fetchJson: async () => new Response("unavailable", { status: 503 })
}));
const existingFallback = await new MinimalArnisAdapter([
  failingOsm,
  createBuildingSlotHandoffProvider(slot!)
]).getBuildingGeometry(target);
assert(existingFallback.provenance.usedSources[0] === "existing_project", "expected existing project fallback");
assert(existingFallback.footprint.length === slot!.geometry.points.length, "expected slot fallback footprint");
assert(existingFallback.provenance.handoff?.foundationSlotId === slot!.id, "expected slot handoff provenance");
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
console.log("OSM/Overpass Building Geometry fallback smoke test passed.");
