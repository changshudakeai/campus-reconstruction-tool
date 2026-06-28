import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entryPath = resolve(`${smokeDir}/campus-naming-classification-entry.ts`);
const bundlePath = resolve(`${smokeDir}/campus-naming-classification-bundle.mjs`);
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entryPath, `
import { findCampusBuildingRecordForGeometry, findCampusBuildingSuppression, loadCampusBuildingDirectory, loadCampusBuildingSuppressions, saveCampusBuildingName, suppressCampusBuilding } from "../../src/services/campusBuildingDirectory";
import { isCampusAffiliatedName, mapWithConcurrency, reverseGeocodeBuildingCandidate } from "../../src/services/campusBuildingNaming";

const values = new Map<string, string>();
globalThis.localStorage = {
  getItem: (key: string) => values.get(key) ?? null,
  setItem: (key: string, value: string) => { values.set(key, value); },
  removeItem: (key: string) => { values.delete(key); },
  clear: () => values.clear(), key: () => null, length: 0
} as Storage;

const campus = {
  id: "campus", schoolName: "华东师范大学", canonicalName: "华东师范大学普陀校区",
  aliases: ["华东师范大学中北校区"], center: { lng: 121.40468, lat: 31.22794 },
  openCenter: { lng: 121.40003, lat: 31.22977 }, radiusM: 900, gaodePoiId: "gaode:campus"
};
if (!isCampusAffiliatedName("华东师范大学普陀校区图书馆", campus)) throw new Error("campus prefix rejected");
if (!isCampusAffiliatedName("华东师范大学文史楼", campus)) throw new Error("school prefix rejected");
if (isCampusAffiliatedName("长风公园游客中心", campus)) throw new Error("off-campus name accepted");

let active = 0;
let maxActive = 0;
const mapped = await mapWithConcurrency(Array.from({ length: 10 }, (_, index) => index), 4, async (value) => {
  active += 1;
  maxActive = Math.max(maxActive, active);
  await new Promise((resolve) => setTimeout(resolve, 5));
  active -= 1;
  return value * 2;
});
if (mapped.length !== 10 || maxActive !== 4) throw new Error("reverse-geocode concurrency limit failed");

const records = [{
  sourceId: "osm:42", name: "华东师范大学文史楼", updatedAt: new Date().toISOString(),
  wgs84: { lng: 121.40003, lat: 31.22977 }
}];
const exterior = [
  { lng: 121.3998, lat: 31.2296 }, { lng: 121.4003, lat: 31.2296 },
  { lng: 121.4003, lat: 31.2300 }, { lng: 121.3998, lat: 31.2300 }
];
const spatial = findCampusBuildingRecordForGeometry(
  records, "overture:different-id", [exterior], { lng: 121.4005, lat: 31.2302 }
);
if (spatial?.name !== "华东师范大学文史楼") throw new Error("cross-source footprint name match failed");

saveCampusBuildingName(campus, "overture:outside", "长风公园游客中心", {
  wgs84: campus.openCenter, gcj02: campus.center
});
const suppressed = suppressCampusBuilding(campus, "overture:outside", {
  wgs84: campus.openCenter, reason: "off-campus"
});
if (loadCampusBuildingDirectory(campus).length !== 0) throw new Error("deleted building remained in local directory");
if (!findCampusBuildingSuppression(suppressed, "overture:outside")) throw new Error("suppression not persisted");
if (loadCampusBuildingSuppressions(campus).length !== 1) throw new Error("suppression missing on next load");

const candidate = {
  id: "candidate", name: "Overture building", kind: "building", source: "overture", confidence: "medium",
  geometry: { type: "polygon", points: exterior },
  provenance: { source: "overture", sourceLabel: "Overture", query: campus.canonicalName, rawId: "overture:campus-building", notes: [] },
  editable: true, accepted: false
} as const;
const reverse = await reverseGeocodeBuildingCandidate(candidate, campus, async () => ({
  name: "长风公园游客中心",
  formattedAddress: "",
  candidates: [
    { name: "长风公园游客中心", distanceM: 5, type: "公园" },
    { name: "华东师范大学普陀校区文史楼", distanceM: 45, type: "科教文化" }
  ]
}));
if (reverse.record?.name !== "华东师范大学普陀校区文史楼") {
  throw new Error("nearest campus-affiliated POI was not preferred");
}
const unmatched = await reverseGeocodeBuildingCandidate({ ...candidate, id: "unmatched", provenance: { ...candidate.provenance, rawId: "overture:unmatched" } }, campus, async () => ({
  name: "Unrelated visitor center", formattedAddress: "", candidates: []
}));
if (unmatched.record !== null) throw new Error("non-campus reverse-geocode name must not become a confirmed name");
console.log("Campus naming classification smoke test passed.");
`);
await build({ entryPoints: [entryPath], outfile: bundlePath, bundle: true, platform: "node", format: "esm", logLevel: "silent" });
await import(`${pathToFileURL(bundlePath).href}?t=${Date.now()}`);
