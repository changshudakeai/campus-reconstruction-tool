import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entryPath = resolve(`${smokeDir}/tauri-runtime-guard-entry.ts`);
const bundlePath = resolve(`${smokeDir}/tauri-runtime-guard-bundle.mjs`);
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entryPath, `
import { DESKTOP_REQUIRED_MESSAGE, invokeDesktop } from "../../src/services/tauriInvoke";
import { saveExportBundleToChosenFolder } from "../../src/services/desktopExport";
let message = "";
try { await invokeDesktop("query_building_candidates", {}); }
catch (error) { message = error instanceof Error ? error.message : String(error); }
if (message !== DESKTOP_REQUIRED_MESSAGE) throw new Error("browser execution did not receive the desktop runtime guidance");
if (message.includes("reading 'invoke'")) throw new Error("raw Tauri invoke error leaked through the runtime guard");
let exportMessage = "";
try { await saveExportBundleToChosenFolder([{ fileName: "test.schem", bytes: new Uint8Array([1]) }]); }
catch (error) { exportMessage = error instanceof Error ? error.message : String(error); }
if (exportMessage !== DESKTOP_REQUIRED_MESSAGE) throw new Error("browser export did not receive desktop runtime guidance");
console.log("Tauri runtime guard smoke test passed.");
`);
await build({ entryPoints: [entryPath], outfile: bundlePath, bundle: true, platform: "node", format: "esm", logLevel: "silent" });
await import(`${pathToFileURL(bundlePath).href}?t=${Date.now()}`);
