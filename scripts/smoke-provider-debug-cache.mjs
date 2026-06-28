import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const app = readFileSync("src/App.tsx", "utf8");
const styles = readFileSync("src/styles.css", "utf8");
const i18n = readFileSync("src/i18n.ts", "utf8");

for (const marker of [
  "ProviderDebugPanel",
  "providerDebug",
  "Provider debug",
  "provider-debug-panel",
  "cache-pill"
]) {
  if (!app.includes(marker) && !styles.includes(marker) && !i18n.includes(marker)) {
    throw new Error(`Missing provider debug marker: ${marker}`);
  }
}

const smokeDir = ".scratch/runtime-smoke";
const entry = `${smokeDir}/provider-debug-cache-entry.ts`;
const bundle = `${smokeDir}/provider-debug-cache-bundle.mjs`;
const entryPath = resolve(entry);
const bundlePath = resolve(bundle);

mkdirSync(smokeDir, { recursive: true });
writeFileSync(
  entry,
  `
import { PUTUO_ONLINE_QUERY_TARGET } from "../../src/domain/mapCandidate";
import { OnlineMapQueryService } from "../../src/services/onlineMapQuery";
import { polygonCandidate } from "../../src/services/mapCandidateFactory";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

let queryCount = 0;
const provider = {
  source: "osm_overpass" as const,
  async query(target) {
    queryCount += 1;
    return [
      polygonCandidate({
        id: "debug-cache-building",
        name: "Debug Cache Building",
        kind: "building",
        source: "osm_overpass",
        confidence: "high",
        query: target.query,
        rawId: "osm:way:debug-cache",
        notes: ["Debug cache provenance note."],
        points: [
          [121.4085, 31.2284],
          [121.4090, 31.2284],
          [121.4090, 31.2280],
          [121.4085, 31.2280]
        ]
      })
    ];
  }
};

const service = new OnlineMapQueryService([provider], ["osm_overpass"]);
const first = await service.queryPutuoCampus(PUTUO_ONLINE_QUERY_TARGET);
const second = await service.queryPutuoCampus(PUTUO_ONLINE_QUERY_TARGET);

assert(queryCount === 1, "expected provider to be queried once because second call is cached");
assert(first.providerDebug[0].cacheStatus === "miss", "expected first provider debug cache miss");
assert(second.providerDebug[0].cacheStatus === "hit", "expected second provider debug cache hit");
assert(second.providerDebug[0].count === 1, "expected cached candidate count");
assert(second.providerDebug[0].rawIds.includes("osm:way:debug-cache"), "expected rawId in debug entry");
assert(
  second.providerDebug[0].notesPreview.includes("Debug cache provenance note."),
  "expected provenance note in debug entry"
);
assert(second.sourceSummaries[0].cacheStatus === "hit", "expected source summary cache hit");

service.clearCache();
const third = await service.queryPutuoCampus(PUTUO_ONLINE_QUERY_TARGET);
assert(queryCount === 2, "expected clearCache to force a provider query");
assert(third.providerDebug[0].cacheStatus === "miss", "expected cache miss after clearCache");
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
console.log("Provider debug cache smoke test passed.");
