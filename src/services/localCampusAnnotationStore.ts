import type { CampusTarget } from "../domain/campusTarget";
import {
  loadCampusBuildingSuppressions,
  saveCampusBuildingSuppressions,
  type CampusBuildingNameRecord,
  type CampusBuildingSuppression
} from "./campusBuildingDirectory";
import { invokeDesktop } from "./tauriInvoke";

interface LocalAnnotationFile {
  schemaVersion: "0.1.0";
  campus: string;
  aliases: string[];
  buildings: CampusBuildingNameRecord[];
  suppressedBuildings?: CampusBuildingSuppression[];
}

export async function loadLocalCampusAnnotationFile(campus: CampusTarget) {
  try {
    const json = await invokeDesktop<string | null>("load_local_campus_annotations", {
      campusKey: campusKey(campus)
    });
    if (!json) return [];
    const payload = JSON.parse(json) as LocalAnnotationFile;
    if (payload.schemaVersion !== "0.1.0") return [];
    const legacySuppressed = payload.buildings
      .filter((record) => record.status === "excluded")
      .map((record) => ({
        sourceId: record.sourceId,
        wgs84: record.wgs84,
        deletedAt: record.updatedAt,
        reason: record.classificationReason ?? "Migrated legacy exclusion."
      }));
    saveCampusBuildingSuppressions(campus, [
      ...loadCampusBuildingSuppressions(campus),
      ...(payload.suppressedBuildings ?? []),
      ...legacySuppressed
    ]);
    return payload.buildings.filter((record) => record.status !== "excluded");
  } catch {
    return [];
  }
}

export async function persistLocalCampusAnnotationFile(
  campus: CampusTarget,
  buildings: CampusBuildingNameRecord[],
  suppressedBuildings = loadCampusBuildingSuppressions(campus)
) {
  const payload: LocalAnnotationFile = {
    schemaVersion: "0.1.0",
    campus: campus.canonicalName,
    aliases: campus.aliases,
    buildings: buildings.filter((record) => record.status !== "excluded"),
    suppressedBuildings
  };
  return invokeDesktop<string>("save_local_campus_annotations", {
    campusKey: campusKey(campus),
    json: `${JSON.stringify(payload, null, 2)}\n`
  });
}

export function campusAnnotationExportJson(
  campus: CampusTarget,
  buildings: CampusBuildingNameRecord[]
) {
  return `${JSON.stringify({
    schemaVersion: "0.1.0",
    campus: campus.canonicalName,
    aliases: campus.aliases,
    buildings: buildings.filter((record) => record.status !== "excluded"),
    suppressedBuildings: loadCampusBuildingSuppressions(campus)
  }, null, 2)}\n`;
}

function campusKey(campus: CampusTarget) {
  let hash = 2166136261;
  for (const character of campus.canonicalName) {
    hash ^= character.charCodeAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return `campus-${(hash >>> 0).toString(16)}`;
}
