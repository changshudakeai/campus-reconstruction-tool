import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const styles = readFileSync("src/styles.css", "utf8");

for (const marker of [
  "GeometryEditorPanel",
  "onCommitDraft",
  "geometry-editor-canvas",
  "draftToManualFeature"
]) {
  if (!app.includes(marker) && !styles.includes(marker)) {
    throw new Error(`Missing geometry editor marker: ${marker}`);
  }
}
for (const marker of ["const overlays = useMemo", "const featureEditorPoints = useMemo", "const featureVisibleKinds = useMemo"]) {
  if (!app.includes(marker)) throw new Error(`Boundary editing must not rebuild unchanged feature-map data: ${marker}`);
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/geometry-editing-entry.ts`;
const bundle = `${smokeDir}/geometry-editing-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { PUTUO_ONLINE_QUERY_TARGET } from "../../src/domain/mapCandidate";
import {
  MAX_CAMPUS_BOUNDARY_POINTS,
  addPointToDraft,
  boundaryDraftFromCandidate,
  createEmptyGeometryDraft,
  draftCanClose,
  draftToManualFeature,
  geometryDraftFromCandidate,
  moveDraftPoint,
  removeLastDraftPoint
} from "../../src/services/geometryEditing";
import { polygonCandidate } from "../../src/services/mapCandidateFactory";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const candidate = polygonCandidate({
  id: "candidate-test-building",
  name: "Test Building",
  kind: "building",
  source: "manual_drawing",
  confidence: "manual",
  query: "test",
  rawId: "manual:test",
  notes: [],
  points: [
    [121.0, 31.0],
    [121.1, 31.0],
    [121.1, 31.1]
  ]
});

const candidateDraft = geometryDraftFromCandidate(candidate);
assert(candidateDraft.kind === "building", "expected candidate kind to carry into draft");
assert(candidateDraft.points.length === 3, "expected candidate geometry points in draft");

const denseBoundaryPoints = Array.from({ length: 80 }, (_, index) => {
  const angle = index / 80 * Math.PI * 2;
  return [121 + Math.cos(angle) * 0.01, 31 + Math.sin(angle) * 0.01] as [number, number];
});
const denseBoundary = polygonCandidate({
  id: "candidate-dense-campus",
  name: "Dense Campus",
  kind: "campus",
  source: "manual_drawing",
  confidence: "manual",
  query: "test",
  rawId: "manual:dense-campus",
  notes: [],
  points: denseBoundaryPoints
});
const originalDensePointCount = denseBoundary.geometry.points.length;
const limitedBoundaryDraft = boundaryDraftFromCandidate(denseBoundary);
assert(limitedBoundaryDraft.points.length === MAX_CAMPUS_BOUNDARY_POINTS, "expected dense campus boundaries to be limited to 50 editable points");
assert(denseBoundary.geometry.points.length === originalDensePointCount, "editing a boundary must not mutate its source candidate");

let draft = createEmptyGeometryDraft(PUTUO_ONLINE_QUERY_TARGET);
draft = addPointToDraft(draft, { lng: 121.4105, lat: 31.2265 });
assert(draft.points.length === 4, "expected added draft point");

draft = moveDraftPoint(draft, 0, { lng: 0.0001, lat: -0.0001 });
assert(draft.points[0].lng > PUTUO_ONLINE_QUERY_TARGET.center.lng - 0.001, "expected point nudge");
assert(draftCanClose(draft), "expected draft to be closable");

const feature = draftToManualFeature(draft, 1);
assert(feature.reviewed, "expected manual feature to be reviewed");
assert(feature.geometry.type === "polygon", "expected closed manual polygon");
assert(feature.provenance.source === "manual_drawing", "expected manual provenance");

draft = removeLastDraftPoint(draft);
assert(draft.points.length === 3, "expected undo last point");
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
console.log("Geometry editing smoke test passed.");
