import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const domain = readFileSync(join(root, "src/domain/buildingGeometry.ts"), "utf8");
const adapter = readFileSync(join(root, "src/adapters/minimalArnisAdapter.ts"), "utf8");
const fixture = readFileSync(join(root, "src/adapters/putuoLibraryFixtureProvider.ts"), "utf8");

for (const marker of [
  '"overture"',
  '"osm_overpass"',
  '"existing_project"',
  '"manual_correction"',
  "PUTUO_LIBRARY_TARGET",
  "BuildingGeometry",
  "MinimalArnisAdapter"
]) {
  if (!domain.includes(marker) && !adapter.includes(marker)) {
    throw new Error(`Missing adapter marker: ${marker}`);
  }
}

for (const marker of ["hipped", "warm_light", "Putuo Campus Library"]) {
  if (!fixture.includes(marker)) {
    throw new Error(`Missing Putuo Library fixture marker: ${marker}`);
  }
}

console.log("Minimal Arnis Adapter smoke check passed.");
