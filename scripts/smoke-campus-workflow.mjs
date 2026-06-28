import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entryPath = resolve(`${smokeDir}/campus-workflow-entry.ts`);
const bundlePath = resolve(`${smokeDir}/campus-workflow-bundle.mjs`);
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entryPath, `
import { campusOnlineQueryTarget, campusTargetFromGaodeCandidate } from "../../src/domain/campusTarget";
import { filterBuildingCandidatesToCampus, filterCampusCandidates } from "../../src/services/campusCandidateFilter";
import { loadCampusBuildingDirectory, saveCampusBuildingName } from "../../src/services/campusBuildingDirectory";
import { reverseGeocodeBuildingCandidate } from "../../src/services/campusBuildingNaming";

const values = new Map<string, string>();
globalThis.localStorage = {
  getItem: (key: string) => values.get(key) ?? null,
  setItem: (key: string, value: string) => { values.set(key, value); },
  removeItem: (key: string) => { values.delete(key); },
  clear: () => values.clear(), key: () => null, length: 0
} as Storage;

const candidate = {
  id: "gaode-campus", name: "华东师范大学中北校区", kind: "campus", source: "gaode_poi", confidence: "medium",
  geometry: { type: "point", points: [{ lng: 121.40468, lat: 31.22794 }] },
  provenance: { source: "gaode_poi", sourceLabel: "Gaode POI", query: "华东师范大学中北校区", rawId: "gaode:campus:ecnu", notes: [], coordinateSystem: "GCJ-02" },
  editable: true, accepted: false
} as const;
const campus = campusTargetFromGaodeCandidate(candidate, "华东师范大学中北校区");
if (campus.canonicalName !== "华东师范大学普陀校区") throw new Error("campus alias was not canonicalized");
if (!campus.aliases.includes("华东师范大学中北校区")) throw new Error("campus alias was not retained");
const query = campusOnlineQueryTarget(campus, "华东师范大学普陀校区 图书馆");
if (query.campus !== campus.canonicalName || query.center.lng !== campus.openCenter.lng || query.gaodeCenter?.lng !== campus.center.lng) throw new Error("provider-specific campus coordinates were not propagated");
saveCampusBuildingName(campus, "osm:way:42", "文史楼");
const directory = loadCampusBuildingDirectory(campus);
if (directory.length !== 1 || directory[0].name !== "文史楼") throw new Error("campus building name was not persisted");
const filtered = filterCampusCandidates([
  candidate,
  { ...candidate, id: "parking", name: "华东师范大学普陀校区停车场" },
  { ...candidate, id: "canteen", name: "华东师范大学普陀校区河东食堂" },
  { ...candidate, id: "affiliate", name: "华东师范大学第二附属中学(普陀校区)" }
], "华东师范大学");
if (filtered.length !== 1 || filtered[0].id !== candidate.id) throw new Error("non-campus POIs leaked into campus selection");
const insideBuilding = { ...candidate, id: "inside", name: "华东师范大学普陀校区图书馆", geometry: { type: "point", points: [{ ...campus.center }] } } as const;
const outsideBuilding = { ...insideBuilding, id: "outside", geometry: { type: "point", points: [{ lng: campus.center.lng + 0.03, lat: campus.center.lat }] } } as const;
const unrelatedNearby = { ...insideBuilding, id: "unrelated", name: "校外商场" } as const;
const scopedBuildings = filterBuildingCandidatesToCampus([insideBuilding, outsideBuilding, unrelatedNearby], campus);
if (scopedBuildings.length !== 1 || scopedBuildings[0].id !== "inside") throw new Error("Gaode building results were not strictly campus-scoped");
let reverseCalls = 0;
const openBuilding = { ...insideBuilding, provenance: { ...insideBuilding.provenance, rawId: "osm:way:42" }, geometry: { type: "polygon", points: [
  { lng: campus.openCenter.lng, lat: campus.openCenter.lat },
  { lng: campus.openCenter.lng + 0.0001, lat: campus.openCenter.lat },
  { lng: campus.openCenter.lng, lat: campus.openCenter.lat + 0.0001 }
] } } as const;
const fakeReverse = async () => { reverseCalls += 1; return { name: "测试楼", formattedAddress: "校内" }; };
await reverseGeocodeBuildingCandidate(openBuilding, campus, fakeReverse);
const cachedReverse = await reverseGeocodeBuildingCandidate(openBuilding, campus, fakeReverse);
if (reverseCalls !== 1 || !cachedReverse.cached) throw new Error("reverse geocoding repeated a cached API call");
console.log("Campus Target and building directory smoke test passed.");
`);
await build({ entryPoints: [entryPath], outfile: bundlePath, bundle: true, platform: "node", format: "esm", logLevel: "silent" });
await import(`${pathToFileURL(bundlePath).href}?t=${Date.now()}`);
