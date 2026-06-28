import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/live-map-providers-entry.ts`;
const bundle = `${smokeDir}/live-map-providers-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { PUTUO_ONLINE_QUERY_TARGET } from "../../src/domain/mapCandidate";
import {
  GaodeJsPoiCandidateProvider,
  GaodePoiCandidateProvider,
  OverpassCandidateProvider,
  createDefaultCandidateProviders,
  gaodePlaceSearchOptions,
  gaodeSearchQueries,
  queryOvertureTile
} from "../../src/services/liveMapProviders";
import { putuoFixtureCandidateProviders } from "../../src/services/onlineMapQuery";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const genericQueries = gaodeSearchQueries("体育馆");
assert(genericQueries.includes("体育馆"), "expected the raw Gaode search term");
assert(genericQueries.length === 1, "expected no implicit campus prefix before a Campus Target is selected");
const fullQueries = gaodeSearchQueries("华东师范大学普陀校区图书馆");
assert(fullQueries.includes("图书馆"), "expected a short building-name query");
const placeSearchOptions = gaodePlaceSearchOptions(2);
assert(placeSearchOptions.city === "上海", "expected a valid Gaode city restriction");
assert(placeSearchOptions.citylimit === true && placeSearchOptions.pageIndex === 2, "expected Gaode pagination options");

function jsonResponse(payload: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: async () => payload
  } as Response;
}

const gaodeProvider = new GaodePoiCandidateProvider({
  apiKey: "test-key",
  fetchJson: async (input) => {
    const url = new URL(String(input));
    assert(url.searchParams.get("keywords") === PUTUO_ONLINE_QUERY_TARGET.query, "expected Gaode keywords query");
    assert(url.searchParams.get("city") === "310000", "expected Shanghai adcode");
    return jsonResponse({
      status: "1",
      pois: [
        {
          id: "B001",
          name: "华东师范大学普陀校区图书馆",
          type: "科教文化服务;学校;高等院校",
          location: "121.409000,31.228200",
          address: "中山北路3663号"
        }
      ]
    });
  }
});

const gaodeCandidates = await gaodeProvider.query(PUTUO_ONLINE_QUERY_TARGET);
assert(gaodeCandidates.length === 1, "expected one Gaode candidate");
assert(gaodeCandidates[0].source === "gaode_poi", "expected Gaode source");
assert(gaodeCandidates[0].geometry.type === "point", "expected Gaode point geometry");
assert(gaodeCandidates[0].name.includes("图书馆"), "expected Gaode POI name");

const gaodeJsProvider = new GaodeJsPoiCandidateProvider({
  apiKey: "test-js-key",
  securityJsCode: "test-security-code",
  searchPoi: async (query, request) => {
    assert(request.pageIndex >= 1, "expected explicit Gaode result pagination");
    assert(request.mode === "text" || request.mode === "nearby", "expected text or nearby search mode");
    return [{
      id: "JS001",
      name: "华东师范大学普陀校区图书馆",
      type: "科教文化服务;学校;高等院校",
      location: { lng: 121.409, lat: 31.2282 },
      address: "中山北路3663号"
    }];
  }
});
const gaodeJsCandidates = await gaodeJsProvider.query(PUTUO_ONLINE_QUERY_TARGET);
assert(gaodeJsCandidates.length === 1, "expected one Gaode JS candidate");
assert(gaodeJsCandidates[0].provenance.rawId === "gaode:js-poi:JS001", "expected Gaode JS provenance");
assert(gaodeJsCandidates[0].geometry.type === "point", "expected Gaode JS point geometry");

const overpassProvider = new OverpassCandidateProvider({
  fetchJson: async (_input, init) => {
    assert(String(init?.body).includes("out%3Ajson") || String(init?.body).includes("data="), "expected Overpass POST body");
    return jsonResponse({
      elements: [
        {
          type: "way",
          id: 42,
          tags: {
            building: "yes",
            name: "Library Test Building"
          },
          geometry: [
            { lon: 121.4085, lat: 31.2284 },
            { lon: 121.4090, lat: 31.2285 },
            { lon: 121.4091, lat: 31.2280 },
            { lon: 121.4085, lat: 31.2284 }
          ]
        },
        {
          type: "way",
          id: 43,
          tags: {
            highway: "service"
          },
          geometry: [
            { lon: 121.4085, lat: 31.2284 },
            { lon: 121.4090, lat: 31.2280 }
          ]
        }
      ]
    });
  }
});

const overpassCandidates = await overpassProvider.query(PUTUO_ONLINE_QUERY_TARGET);
assert(overpassCandidates.length === 2, "expected two Overpass candidates");
assert(overpassCandidates[0].geometry.type === "polygon", "expected closed way polygon");
assert(overpassCandidates[0].kind === "building", "expected building kind");
assert(overpassCandidates[1].geometry.type === "polyline", "expected road polyline");
assert(overpassCandidates[1].kind === "road", "expected road kind");

let tileCalls = 0;
const tileFeatures = await queryOvertureTile("https://overture.test/buildings", { west: 121, south: 31, east: 121.01, north: 31.01 }, 0, async () => {
  tileCalls += 1;
  return jsonResponse({ features: tileCalls === 1 ? Array.from({ length: 200 }, (_, index) => ({ id: \`root-\${index}\` })) : [{ id: \`tile-\${tileCalls}\` }] });
});
assert(tileCalls === 5 && tileFeatures.length === 4, "expected a saturated Overture tile to split into complete sub-queries");

const defaultProviders = createDefaultCandidateProviders(putuoFixtureCandidateProviders);
assert(defaultProviders.some((provider) => provider.source === "osm_overpass"), "expected Overpass provider in defaults");
assert(defaultProviders.some((provider) => provider.source === "gaode_poi"), "expected Gaode provider in defaults");
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
console.log("Live map providers smoke test passed.");
