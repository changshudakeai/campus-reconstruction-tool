import type { CampusTarget } from "../domain/campusTarget";
import type { MapFeature } from "../domain/foundationManifest";
import type { MapCandidate } from "../domain/mapCandidate";
import type { CandidateReviewStatus } from "./candidateReview";
import type { FoundationGeneratorStyle, FoundationStylePack } from "./foundationFeatureGenerators";
import type { FoundationStyleSettings } from "./foundationStyle";
import type { GeometryDraft } from "./geometryEditing";
import {
  loadCampusBuildingDirectory,
  loadCampusBuildingSuppressions,
  replaceCampusBuildingDirectory,
  saveCampusBuildingSuppressions,
  type CampusBuildingNameRecord,
  type CampusBuildingSuppression
} from "./campusBuildingDirectory";

export const CAMPUS_PROJECT_SCHEMA_VERSION = "1.0" as const;
export const DEFAULT_MINECRAFT_TARGET_VERSION = "26.2";

export interface CampusProjectHistoryEntry {
  id: string;
  at: string;
  label: string;
}

export interface CampusProjectFoundationState {
  boundaryDraft: GeometryDraft;
  candidates: MapCandidate[];
  reviews: Record<string, CandidateReviewStatus>;
  manualFeatures: MapFeature[];
  foundationStyle: FoundationStyleSettings;
  foundationStylePack: FoundationStylePack;
  orientationDegrees: number;
  providerSnapshot?: unknown;
  boundaryHistory?: GeometryDraft[];
  featureHistory?: GeometryDraft[];
  reviewUndoStack?: Array<Record<string, CandidateReviewStatus>>;
}

export interface CampusReconstructionProject {
  schemaVersion: typeof CAMPUS_PROJECT_SCHEMA_VERSION;
  minecraftTargetVersion: string;
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  campus: CampusTarget;
  foundation: CampusProjectFoundationState;
  history: CampusProjectHistoryEntry[];
}

export interface PortableCampusProject {
  project: CampusReconstructionProject;
  campusMemory: {
    buildingNames: CampusBuildingNameRecord[];
    suppressions: CampusBuildingSuppression[];
  };
}

const INDEX_KEY = "campus-reconstruction-project:index:v1";
const ACTIVE_PREFIX = "campus-reconstruction-project:active:v1:";
const PROJECT_PREFIX = "campus-reconstruction-project:data:v1:";
const IMPORT_BACKUP_PREFIX = "campus-reconstruction-project:import-backup:v1:";

export function createCampusProject(input: {
  name: string;
  campus: CampusTarget;
  foundation: CampusProjectFoundationState;
  minecraftTargetVersion?: string;
}): CampusReconstructionProject {
  const now = new Date().toISOString();
  return {
    schemaVersion: CAMPUS_PROJECT_SCHEMA_VERSION,
    minecraftTargetVersion: input.minecraftTargetVersion ?? DEFAULT_MINECRAFT_TARGET_VERSION,
    id: `campus-project-${safeId(input.campus.canonicalName)}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
    name: input.name.trim() || input.campus.canonicalName,
    createdAt: now,
    updatedAt: now,
    campus: input.campus,
    foundation: input.foundation,
    history: []
  };
}

export function listCampusProjects(campus?: CampusTarget): CampusReconstructionProject[] {
  return readIndex()
    .map(loadCampusProject)
    .filter((project): project is CampusReconstructionProject => Boolean(project))
    .filter((project) => !campus || project.campus.canonicalName === campus.canonicalName)
    .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
}

export function loadCampusProject(id: string): CampusReconstructionProject | null {
  try {
    const raw = localStorage.getItem(`${PROJECT_PREFIX}${id}`);
    return raw ? validateProject(JSON.parse(raw)) : null;
  } catch {
    return null;
  }
}

export function loadActiveCampusProject(campus: CampusTarget): CampusReconstructionProject | null {
  const id = localStorage.getItem(activeKey(campus));
  return id ? loadCampusProject(id) : listCampusProjects(campus)[0] ?? null;
}

export function saveCampusProject(project: CampusReconstructionProject): CampusReconstructionProject {
  const next = { ...project, updatedAt: new Date().toISOString(), history: project.history.slice(-50) };
  localStorage.setItem(`${PROJECT_PREFIX}${next.id}`, JSON.stringify(next));
  const index = readIndex();
  if (!index.includes(next.id)) localStorage.setItem(INDEX_KEY, JSON.stringify([...index, next.id]));
  localStorage.setItem(activeKey(next.campus), next.id);
  return next;
}

export function duplicateCampusProject(project: CampusReconstructionProject, name: string) {
  const duplicate = createCampusProject({
    name,
    campus: project.campus,
    foundation: project.foundation,
    minecraftTargetVersion: project.minecraftTargetVersion
  });
  duplicate.history = [...project.history, historyEntry(`Saved as ${duplicate.name}`)].slice(-50);
  return saveCampusProject(duplicate);
}

export function appendProjectHistory(project: CampusReconstructionProject, label: string) {
  return { ...project, history: [...project.history, historyEntry(label)].slice(-50) };
}

export function exportCampusProjectJson(project: CampusReconstructionProject) {
  const portable: PortableCampusProject = {
    project,
    campusMemory: {
      buildingNames: loadCampusBuildingDirectory(project.campus),
      suppressions: loadCampusBuildingSuppressions(project.campus)
    }
  };
  return `${JSON.stringify(portable, null, 2)}\n`;
}

export function importCampusProjectJson(json: string, expectedCampus?: CampusTarget): CampusReconstructionProject {
  const portable = JSON.parse(json) as Partial<PortableCampusProject>;
  const project = validateProject(portable.project);
  if (expectedCampus && project.campus.canonicalName !== expectedCampus.canonicalName) throw new Error("Imported project belongs to a different Campus Target.");
  const imported = createCampusProject({
    name: uniqueImportedProjectName(project.campus, project.name),
    campus: project.campus,
    foundation: project.foundation,
    minecraftTargetVersion: project.minecraftTargetVersion
  });
  return saveCampusProject({
    ...imported,
    history: [...project.history, historyEntry(`Imported project copy from ${project.name}`)].slice(-50)
  });
}

export function loadImportBackup(campus: CampusTarget): PortableCampusProject["campusMemory"] | null {
  try {
    const raw = localStorage.getItem(`${IMPORT_BACKUP_PREFIX}${safeId(campus.canonicalName)}`);
    return raw ? JSON.parse(raw) as PortableCampusProject["campusMemory"] : null;
  } catch {
    return null;
  }
}

function backupCampusMemory(campus: CampusTarget) {
  localStorage.setItem(`${IMPORT_BACKUP_PREFIX}${safeId(campus.canonicalName)}`, JSON.stringify({
    buildingNames: loadCampusBuildingDirectory(campus),
    suppressions: loadCampusBuildingSuppressions(campus)
  } satisfies PortableCampusProject["campusMemory"]));
}

function replaceCampusMemory(campus: CampusTarget, names: CampusBuildingNameRecord[], suppressions: CampusBuildingSuppression[]) {
  replaceCampusBuildingDirectory(campus, names);
  saveCampusBuildingSuppressions(campus, suppressions);
}

function uniqueImportedProjectName(campus: CampusTarget, name: string) {
  const existingNames = new Set(listCampusProjects(campus).map((project) => project.name));
  if (!existingNames.has(name)) return name;
  const base = `${name} (imported)`;
  if (!existingNames.has(base)) return base;
  let index = 2;
  while (existingNames.has(`${base} ${index}`)) index += 1;
  return `${base} ${index}`;
}

function validateProject(value: unknown): CampusReconstructionProject {
  const project = value as Partial<CampusReconstructionProject> | null;
  if (!project || project.schemaVersion !== CAMPUS_PROJECT_SCHEMA_VERSION) throw new Error("Unsupported Campus Reconstruction Project schema version.");
  if (!project.id || !project.name || !project.campus?.canonicalName || !project.foundation) throw new Error("Invalid Campus Reconstruction Project shape.");
  if (!Array.isArray(project.foundation.candidates) || !Array.isArray(project.foundation.manualFeatures) || !project.foundation.reviews) throw new Error("Invalid Campus Reconstruction Project candidate snapshot.");
  return { ...project, history: Array.isArray(project.history) ? project.history.slice(-50) : [] } as CampusReconstructionProject;
}

function historyEntry(label: string): CampusProjectHistoryEntry {
  const at = new Date().toISOString();
  return { id: `history-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`, at, label };
}

function readIndex(): string[] {
  try {
    const raw = localStorage.getItem(INDEX_KEY);
    return raw ? JSON.parse(raw) as string[] : [];
  } catch {
    return [];
  }
}

function activeKey(campus: CampusTarget) { return `${ACTIVE_PREFIX}${safeId(campus.canonicalName)}`; }
function safeId(value: string) { return encodeURIComponent(value.toLowerCase().replace(/\s+/g, "-")); }

// Keeps the type import alive in declaration output when style packs evolve independently.
export type CampusProjectGeneratorStyle = FoundationGeneratorStyle;
