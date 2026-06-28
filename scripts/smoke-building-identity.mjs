import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/building-identity-entry.ts`;
const bundle = `${smokeDir}/building-identity-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entry, `
import { MinimalArnisAdapter, type BuildingGeometryProvider } from "../../src/adapters/minimalArnisAdapter";
import { createBuildingGeometryObservation } from "../../src/services/buildingObservation";
import {
  applyObservationReviewDecision,
  resolveBuildingIdentity
} from "../../src/services/buildingIdentity";
import { buildingSlotToBuildingTarget } from "../../src/services/buildingSlotTarget";
import {
  foundationManifestPlaceholder,
  selectRepresentativeBuildingSlot
} from "../../src/domain/foundationManifest";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const slot = selectRepresentativeBuildingSlot(foundationManifestPlaceholder)!;
const target = buildingSlotToBuildingTarget(slot);
const shiftedCorrect = slot.geometry.points.map((point) => ({ lng: point.lng + 0.00003, lat: point.lat }));
const tinyNearest = [
  { lng: target.approximateCenter.lng - 0.00004, lat: target.approximateCenter.lat - 0.00004 },
  { lng: target.approximateCenter.lng + 0.00004, lat: target.approximateCenter.lat - 0.00004 },
  { lng: target.approximateCenter.lng + 0.00004, lat: target.approximateCenter.lat + 0.00004 },
  { lng: target.approximateCenter.lng - 0.00004, lat: target.approximateCenter.lat + 0.00004 }
];
const correct = createBuildingGeometryObservation({
  id: "overture:correct-library",
  source: "overture",
  sourceFeatureId: "correct-library",
  name: "华东师范大学普陀校区图书馆",
  components: [{ exterior: shiftedCorrect, interiorRings: [] }]
});
const distractor = createBuildingGeometryObservation({
  id: "overture:nearest-distractor",
  source: "overture",
  sourceFeatureId: "nearest-distractor",
  name: "Campus Utility Building",
  components: [{ exterior: tinyNearest, interiorRings: [] }]
});
const farAlias = createBuildingGeometryObservation({
  id: "osm:far-library-name",
  source: "osm_overpass",
  sourceFeatureId: "far-library-name",
  name: "Putuo Campus Library",
  components: [{
    exterior: shiftedCorrect.map((point) => ({ lng: point.lng + 0.01, lat: point.lat + 0.01 })),
    interiorRings: []
  }]
});

const resolution = resolveBuildingIdentity(target, [distractor, farAlias, correct]);
assert(resolution.selectedObservationId === correct.id, "expected overlap and aliases to beat nearest-feature distance");
assert(resolution.matches[0].observationId === correct.id, "expected correct library ranked first");
assert(resolution.matches.find((match) => match.observationId === farAlias.id)?.confidence === "rejected", "expected excessive distance rejection");
assert(resolution.matches.find((match) => match.observationId === correct.id)?.reasons.some((reason) => reason.criterion === "name" && reason.score === 1), "expected Chinese alias reason");

const secondPlausible = createBuildingGeometryObservation({
  id: "osm:ambiguous-library",
  source: "osm_overpass",
  sourceFeatureId: "ambiguous-library",
  name: "ECNU Putuo Library",
  components: [{ exterior: shiftedCorrect.map((point) => ({ lng: point.lng + 0.00001, lat: point.lat })), interiorRings: [] }]
});
const ambiguous = resolveBuildingIdentity(target, [correct, secondPlausible]);
assert(ambiguous.ambiguous, "expected close identity scores to require review");
assert(ambiguous.selectedObservationId === null, "expected no silent selection for ambiguous matches");
assert(ambiguous.matches.filter((match) => match.reviewRequired).length === 2, "expected both ambiguous candidates marked for review");

const provider: BuildingGeometryProvider = {
  source: "overture",
  async fetchBuildingGeometry() {
    return {
      footprint: shiftedCorrect,
      observations: [correct, distractor],
      confidence: { footprint: "high" }
    };
  }
};
const geometry = await new MinimalArnisAdapter([provider]).getBuildingGeometry(target);
const accepted = applyObservationReviewDecision(geometry, correct.id, "accepted");
assert(accepted.provenance.observationReviews[correct.id] === "accepted", "expected accepted review state");
assert(accepted.provenance.identityResolution.selectedObservationId === correct.id, "expected accepted observation selection");
const rejected = applyObservationReviewDecision(accepted, correct.id, "rejected");
assert(rejected.provenance.observationReviews[correct.id] === "rejected", "expected rejected review state");
assert(rejected.provenance.notes.some((note) => note.includes("marked rejected")), "expected review provenance note");
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
console.log("Representative Building identity matching smoke test passed.");
