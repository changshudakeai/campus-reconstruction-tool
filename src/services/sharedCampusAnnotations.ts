import type { CampusTarget } from "../domain/campusTarget";
import type { CampusBuildingNameRecord } from "./campusBuildingDirectory";

interface AnnotationIndexEntry {
  campus: string;
  aliases?: string[];
  file: string;
}

interface AnnotationFile {
  schemaVersion: "0.1.0";
  campus: string;
  buildings: CampusBuildingNameRecord[];
}

const viteEnv = (import.meta as ImportMeta & { env?: Record<string, string | undefined> }).env ?? {};

export async function loadSharedCampusAnnotations(campus: CampusTarget): Promise<CampusBuildingNameRecord[]> {
  const base = (viteEnv.VITE_CAMPUS_ANNOTATION_BASE_URL ?? "/campus-building-annotations").replace(/\/$/, "");
  const indexResponse = await fetch(`${base}/index.json`, { cache: "no-cache" });
  if (!indexResponse.ok) return [];
  const index = await indexResponse.json() as { campuses?: AnnotationIndexEntry[] };
  const names = new Set([campus.canonicalName, ...campus.aliases]);
  const entry = index.campuses?.find((item) => names.has(item.campus) || item.aliases?.some((alias) => names.has(alias)));
  if (!entry) return [];
  const response = await fetch(`${base}/${entry.file}`, { cache: "no-cache" });
  if (!response.ok) return [];
  const payload = await response.json() as AnnotationFile;
  return payload.schemaVersion === "0.1.0" && Array.isArray(payload.buildings)
    ? payload.buildings.map((record) => ({ ...record, nameSource: "shared_annotation" }))
    : [];
}
