import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const providerSource = readFileSync("src/adapters/overtureBuildingGeometryProvider.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const envExample = readFileSync(".env.example", "utf8");

for (const marker of [
  "OvertureBuildingGeometryProvider",
  "createDefaultBuildingGeometryProviders",
  "VITE_BUILDING_GEOMETRY_OFFLINE_FIXTURE",
  "createBoundedOvertureRequest"
]) {
  if (!providerSource.includes(marker) && !app.includes(marker) && !envExample.includes(marker)) {
    throw new Error(`Missing Overture provider marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/overture-building-provider-entry.ts`;
const bundle = `${smokeDir}/overture-building-provider-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import {
  OvertureBuildingGeometryProvider,
  withBuildingGeometryFallback
} from "../../src/adapters/overtureBuildingGeometryProvider";
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

let requestedUrl = "";
const liveProvider = new OvertureBuildingGeometryProvider({
  endpoint: "https://example.test/overture/buildings",
  fetchJson: async (input) => {
    requestedUrl = String(input);
    return new Response(JSON.stringify({
      features: [
        {
          id: "far-building",
          geometry: {
            type: "Polygon",
            coordinates: [[[121.42, 31.24], [121.421, 31.24], [121.421, 31.241], [121.42, 31.24]]]
          },
          properties: { height: 40 }
        },
        {
          id: "putuo-library-live",
          geometry: {
            type: "MultiPolygon",
            coordinates: [
              [
                [[121.4085, 31.2284], [121.4091, 31.2286], [121.4094, 31.2283], [121.4092, 31.2279], [121.4087, 31.2278], [121.4084, 31.2281], [121.4085, 31.2284]],
                [[121.4088, 31.2282], [121.4089, 31.2282], [121.4089, 31.2281], [121.4088, 31.2282]]
              ],
              [
                [[121.4095, 31.2282], [121.4096, 31.2282], [121.4096, 31.2281], [121.4095, 31.2282]]
              ]
            ]
          },
          properties: {
            height: 24,
            num_floors: "6",
            roof_shape: "hipped",
            roof_material: "tile",
            roof_orientation: "long_axis",
            facade_material: "brick",
            facade_color: "warm_red"
          }
        }
      ]
    }), { status: 200, headers: { "content-type": "application/json" } });
  }
});

const liveGeometry = await new MinimalArnisAdapter([liveProvider]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const url = new URL(requestedUrl);
assert(url.searchParams.get("lng") === String(PUTUO_LIBRARY_TARGET.approximateCenter.lng), "expected target longitude");
assert(url.searchParams.get("lat") === String(PUTUO_LIBRARY_TARGET.approximateCenter.lat), "expected target latitude");
assert(url.searchParams.get("theme") === "buildings", "expected buildings theme");
assert(url.searchParams.has("bbox"), "expected bounded bbox");
assert(url.searchParams.get("limit") === "20", "expected strict result limit");
assert(liveGeometry.footprint.length === 6, "expected closing coordinate removed");
assert(liveGeometry.heightM === 24, "expected live height");
assert(liveGeometry.floors === 6, "expected numeric floor parsing");
assert(liveGeometry.roof.material === "tile", "expected roof material");
assert(liveGeometry.facade.material === "brick", "expected facade material");
assert(liveGeometry.provenance.usedSources[0] === "overture", "expected Overture source");
assert(liveGeometry.provenance.notes.some((note) => note.includes("putuo-library-live")), "expected feature ID provenance");
assert(liveGeometry.provenance.sourceRecords.length === 1, "expected source record");
assert(liveGeometry.provenance.sourceRecords[0].featureId === "putuo-library-live", "expected raw feature ID");
assert(liveGeometry.provenance.sourceRecords[0].queryLimit === 20, "expected query limit provenance");
assert(liveGeometry.provenance.sourceRecords[0].components.length === 2, "expected detached part preserved");
assert(liveGeometry.provenance.sourceRecords[0].components[0].interiorRings.length === 1, "expected interior ring preserved");

const failingProvider = new OvertureBuildingGeometryProvider({
  endpoint: "https://example.test/overture/buildings",
  fetchJson: async () => new Response("unavailable", { status: 503 })
});
const fallbackProvider = withBuildingGeometryFallback(failingProvider, putuoLibraryFixtureProvider);
const fallbackGeometry = await new MinimalArnisAdapter([fallbackProvider]).getBuildingGeometry(PUTUO_LIBRARY_TARGET);
assert(fallbackGeometry.heightM === 22, "expected fixture height after live failure");
assert(fallbackGeometry.provenance.notes.some((note) => note.includes("Fixture")), "expected fixture provenance");
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
console.log("Overture Building Geometry provider smoke test passed.");
