import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const previewer = readFileSync("src/components/SchematicPreviewer.tsx", "utf8");
const app = readFileSync("src/App.tsx", "utf8");

for (const marker of [
  'powerPreference: "low-power"',
  "Math.min(window.devicePixelRatio, 2)",
  'controls.addEventListener("change", renderPreview)',
  'controls.removeEventListener("change", renderPreview)',
  "renderPreviewRef.current?.()",
  "renderable.geometry instanceof THREE.BufferGeometry",
  "<SchematicPreviewer"
]) {
  if (!previewer.includes(marker) && !app.includes(marker)) {
    throw new Error(`Missing Three.js rendering marker: ${marker}`);
  }
}

if (previewer.includes("requestAnimationFrame") || previewer.includes("cancelAnimationFrame")) {
  throw new Error("Previewer should render on demand instead of running a permanent animation loop.");
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/three-preview-rendering-entry.ts`;
const bundle = `${smokeDir}/three-preview-rendering-bundle.mjs`;
mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { MinimalArnisAdapter } from "../../src/adapters/minimalArnisAdapter";
import { putuoLibraryFixtureProvider } from "../../src/adapters/putuoLibraryFixtureProvider";
import { PUTUO_LIBRARY_TARGET } from "../../src/domain/buildingGeometry";
import { generateSchematicFromBuildingGeometry } from "../../src/services/buildingGeometryToSchematic";
import { listInspectableBlocks } from "../../src/services/schematicEditing";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const geometry = await new MinimalArnisAdapter([putuoLibraryFixtureProvider])
  .getBuildingGeometry(PUTUO_LIBRARY_TARGET);
const model = generateSchematicFromBuildingGeometry(geometry);
const instances = listInspectableBlocks(model);
const paletteGroups = new Map<number, number>();
for (const block of instances) {
  paletteGroups.set(block.paletteIndex, (paletteGroups.get(block.paletteIndex) ?? 0) + 1);
}

assert(instances.length > 0, "expected visible block instances for Three.js");
assert(paletteGroups.size >= 4, "expected multiple InstancedMesh palette groups");
assert(instances.every((block) => block.block !== "minecraft:air"), "expected air blocks omitted");
assert(instances.every((block) => block.x >= 0 && block.x < model.width), "expected valid x coordinates");
assert(instances.every((block) => block.y >= 0 && block.y < model.height), "expected valid y coordinates");
assert(instances.every((block) => block.z >= 0 && block.z < model.length), "expected valid z coordinates");
  `.trim()
);

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
console.log("Three.js schematic rendering smoke test passed.");
