import { readFileSync } from "node:fs";

const i18n = readFileSync("src/i18n.ts", "utf8");
const app = readFileSync("src/App.tsx", "utf8");
const styles = readFileSync("src/styles.css", "utf8");

const requiredI18nMarkers = [
  'export type Language = "en" | "zh-CN"',
  "English",
  "简体中文",
  "Minecraft 校园复刻工具",
  "地基模式",
  "精细建筑模式",
  "地图候选",
  "批量方块替换",
  "导出更新后的 .schem"
];

const requiredAppMarkers = [
  "LanguageSelector",
  "setLanguage",
  "translations[language]",
  "languageOptions.map",
  "sourceLabel(source, t)",
  "confidenceLabel(candidate.confidence, t)",
  "previewHint={t.previewHint}"
];

for (const marker of requiredI18nMarkers) {
  if (!i18n.includes(marker)) {
    throw new Error(`Missing i18n marker: ${marker}`);
  }
}

for (const marker of requiredAppMarkers) {
  if (!app.includes(marker)) {
    throw new Error(`Missing app i18n wiring marker: ${marker}`);
  }
}

if (!styles.includes(".language-selector")) {
  throw new Error("Missing language selector styles.");
}

console.log("i18n smoke test passed.");
