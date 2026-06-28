import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const domain = readFileSync(join(root, "src/domain/mapCandidate.ts"), "utf8");
const service = readFileSync(join(root, "src/services/onlineMapQuery.ts"), "utf8");
const app = readFileSync(join(root, "src/App.tsx"), "utf8");
const i18n = readFileSync(join(root, "src/i18n.ts"), "utf8");

for (const marker of [
  "MAP_CANDIDATE_SOURCE_PRIORITY",
  '"arnis_open_geodata"',
  '"overture"',
  '"osm_overpass"',
  '"gaode_poi"',
  '"gaode_aoi"',
  "isAoiCandidate"
]) {
  if (!domain.includes(marker) && !service.includes(marker)) {
    throw new Error(`Missing online map query marker: ${marker}`);
  }
}

for (const marker of ["Map Candidates", "Source priority", "AOI Candidate", "Query Putuo Campus"]) {
  if (!app.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing Foundation Mode UI marker: ${marker}`);
  }
}

console.log("Online Map Query smoke check passed.");
