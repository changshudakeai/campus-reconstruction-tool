import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/building-location-anchor-entry.ts`;
const bundle = `${smokeDir}/building-location-anchor-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { PUTUO_LIBRARY_SEARCH_TARGET } from "../../src/domain/mapCandidate";
import {
  buildingTargetFromLocationAnchors,
  gaodeMapClickToLocationAnchor,
  gaodeCandidateToLocationAnchor,
  openGeodataAnchorFromGaode,
  gcj02ToWgs84,
  wgs84ToGcj02
} from "../../src/services/buildingLocationAnchor";
import { pointCandidate } from "../../src/services/mapCandidateFactory";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

assert(
  PUTUO_LIBRARY_SEARCH_TARGET.query === "华东师范大学普陀校区图书馆",
  "Detailed Building search must use the exact building name"
);

const converted = gcj02ToWgs84({ lng: 121.409, lat: 31.2282 });
assert(Math.abs(converted.lng - 121.4043487) < 0.000001, "expected Shanghai GCJ-02 longitude conversion");
assert(Math.abs(converted.lat - 31.2300354) < 0.000001, "expected Shanghai GCJ-02 latitude conversion");
assert(Math.abs(converted.lng - 121.409) > 0.004, "raw Gaode longitude must not pass through unchanged");

const candidate = pointCandidate({
  id: "candidate-gaode-js-poi-library",
  name: "华东师范大学普陀校区图书馆",
  kind: "building",
  source: "gaode_poi",
  confidence: "high",
  query: PUTUO_LIBRARY_SEARCH_TARGET.query,
  rawId: "gaode:js-poi:library",
  notes: ["live"],
  coordinateSystem: "GCJ-02",
  point: [121.409, 31.2282]
});
const gaode = gaodeCandidateToLocationAnchor(candidate);
const open = openGeodataAnchorFromGaode(gaode);
const roundTrip = wgs84ToGcj02(open.position);
assert(Math.abs(roundTrip.lng - gaode.position.lng) < 0.000001, "expected WGS-84 candidate longitude to map back to Gaode");
assert(Math.abs(roundTrip.lat - gaode.position.lat) < 0.000001, "expected WGS-84 candidate latitude to map back to Gaode");
const target = buildingTargetFromLocationAnchors(gaode, open);
assert(gaode.coordinateSystem === "GCJ-02", "expected original Gaode coordinate system");
assert(open.coordinateSystem === "WGS-84", "expected open-geodata coordinate system");
assert(open.derivedFromPoiId === "gaode:js-poi:library", "expected POI lineage");
assert(open.transformation === "gcj02-to-wgs84-iterative-v1", "expected transformation lineage");
assert(target.aliases.includes(candidate.name), "expected the confirmed POI name to remain an alias");

const gymCandidate = pointCandidate({
  id: "candidate-gaode-js-poi-gym",
  name: "华东师范大学普陀校区体育馆",
  kind: "building",
  source: "gaode_poi",
  confidence: "high",
  query: "华东师范大学普陀校区体育馆",
  rawId: "gaode:js-poi:gym",
  notes: ["live"],
  coordinateSystem: "GCJ-02",
  point: [121.409, 31.2282]
});
const gymAnchor = gaodeCandidateToLocationAnchor(gymCandidate);
const gymTarget = buildingTargetFromLocationAnchors(gymAnchor, openGeodataAnchorFromGaode(gymAnchor));
assert(gymTarget.aliases.some((alias) => alias.includes("体育馆")), "expected aliases to follow the current search target");
assert(!gymTarget.aliases.some((alias) => alias.includes("图书馆") || alias.toLowerCase().includes("library")), "searching for another building must not inject library aliases");
const clicked = gaodeMapClickToLocationAnchor({
  name: "华东师范大学普陀校区体育馆",
  query: "体育馆",
  point: { lng: 121.41, lat: 31.23 }
});
assert(clicked.acquisition === "map_click", "expected map-click acquisition provenance");
assert(clicked.poiId.startsWith("gaode:map-click:"), "expected stable synthetic Gaode anchor identity");
`.trim());

await build({
  entryPoints: [resolve(entry)],
  outfile: resolve(bundle),
  bundle: true,
  platform: "node",
  format: "esm",
  logLevel: "silent"
});
await import(`${pathToFileURL(resolve(bundle)).href}?t=${Date.now()}`);
console.log("Building location anchor smoke test passed.");
