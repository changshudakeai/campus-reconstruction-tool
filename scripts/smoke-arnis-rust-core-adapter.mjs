import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const tauri = readFileSync("src-tauri/src/lib.rs", "utf8");
const core = readFileSync("src-tauri/crates/arnis-core/src/lib.rs", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
for (const marker of [
  "query_building_candidates",
  "generate_building",
  "BlockSink",
  "block_runs",
  "building_part_count",
  "primaryArnisCandidate",
  "nearbyArnisCandidates",
  "t.confirmReviewedSlotAndGenerate",
  "t.showAllNearbyCandidates",
]) {
  if (!tauri.includes(marker) && !core.includes(marker) && !app.includes(marker)) throw new Error(`Missing Arnis Rust Core marker: ${marker}`);
}
if (app.includes("createDefaultBuildingGeometryProviders(slot)")) throw new Error("Current Result must not silently use the placeholder provider chain.");

const dir = ".scratch/runtime-smoke";
mkdirSync(dir, { recursive: true });
writeFileSync(`${dir}/arnis-rle-entry.ts`, `
import { buildingGeometryFromArnisCandidate, decodeBlockRuns } from "../../src/adapters/arnisRustCoreAdapter";
function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const result = decodeBlockRuns({ width: 2, height: 2, length: 2, palette: ["minecraft:air", "minecraft:stone"], blockRuns: [
  { paletteIndex: 0, runLength: 2 }, { paletteIndex: 1, runLength: 4 }, { paletteIndex: 0, runLength: 2 }
] });
assert(result.length === 8, "expected declared dimensions");
assert(Array.from(result).join(",") === "0,0,1,1,1,1,0,0", "expected stable RLE decode");
let rejected = false; try { decodeBlockRuns({ width: 1, height: 1, length: 1, palette: ["minecraft:air"], blockRuns: [] }); } catch { rejected = true; }
assert(rejected, "expected incomplete RLE rejection");
const target = {
  name: "Test Building", campus: "ECNU Putuo Campus", aliases: ["Test Building"],
  approximateCenter: { lng: 121, lat: 31 }
};
const candidate = {
  id: "osm:test", source: "osm_overpass", name: "Test Building", tags: { building: "yes" },
  components: [{
    exterior: [{ lng: 121, lat: 31 }, { lng: 121.001, lat: 31 }, { lng: 121.001, lat: 30.999 }],
    interiorRings: [[{ lng: 121.0002, lat: 30.9998 }, { lng: 121.0004, lat: 30.9998 }, { lng: 121.0004, lat: 30.9996 }]]
  }],
  heightM: null, floors: null, roofShape: null, identityConfidence: "high",
  distanceM: 1, widthM: 10, lengthM: 10, parts: []
};
const geometry = buildingGeometryFromArnisCandidate(target, candidate);
assert(geometry.footprintComponents[0].interiorRings.length === 1, "expected complete Arnis interior rings in Building Geometry");
`.trim());
await build({ entryPoints:[resolve(`${dir}/arnis-rle-entry.ts`)], outfile:resolve(`${dir}/arnis-rle-bundle.mjs`), bundle:true, platform:"node", format:"esm", logLevel:"silent", external:["@tauri-apps/api/core"] });
await import(`${pathToFileURL(resolve(`${dir}/arnis-rle-bundle.mjs`)).href}?t=${Date.now()}`);
console.log("Arnis Rust Core adapter smoke test passed.");
