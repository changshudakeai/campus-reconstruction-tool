import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));

const requiredFiles = [
  "package.json",
  "index.html",
  "src/App.tsx",
  "src/main.tsx",
  "src/domain/foundationManifest.ts",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/src/main.rs"
];

for (const file of requiredFiles) {
  readFileSync(join(root, file), "utf8");
}

const app = readFileSync(join(root, "src/App.tsx"), "utf8");
const i18n = readFileSync(join(root, "src/i18n.ts"), "utf8");
const manifest = readFileSync(join(root, "src/domain/foundationManifest.ts"), "utf8");

const requiredStrings = [
  "Foundation Mode",
  "Detailed Building Mode",
  "ECNU Putuo Campus",
  "First Vertical Slice",
  "Putuo Campus Library",
  "foundationManifestPlaceholder"
];

for (const text of requiredStrings) {
  if (!app.includes(text) && !manifest.includes(text) && !i18n.includes(text)) {
    throw new Error(`Missing app shell marker: ${text}`);
  }
}

console.log("App shell smoke check passed.");
