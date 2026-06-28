import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const review = readFileSync(join(root, "src/services/candidateReview.ts"), "utf8");
const manifest = readFileSync(join(root, "src/domain/foundationManifest.ts"), "utf8");
const app = readFileSync(join(root, "src/App.tsx"), "utf8");
const i18n = readFileSync(join(root, "src/i18n.ts"), "utf8");

for (const marker of [
  "acceptCandidate",
  "rejectCandidate",
  "buildFoundationManifestFromReviews",
  "mapFeatureToBuildingSlot",
  "makeManualPutuoBoundaryFeature"
]) {
  if (!review.includes(marker)) {
    throw new Error(`Missing candidate review marker: ${marker}`);
  }
}

for (const marker of ["geometry", "provenance", "replacementPolicy"]) {
  if (!manifest.includes(marker)) {
    throw new Error(`Missing manifest contract marker: ${marker}`);
  }
}

for (const marker of ["Accept", "Reject", "Manual boundary", "Reviewed Features"]) {
  if (!app.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing review UI marker: ${marker}`);
  }
}

console.log("Candidate review smoke check passed.");
