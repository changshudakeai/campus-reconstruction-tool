import {
  Blocks,
  Building2,
  CheckCircle2,
  DatabaseZap,
  FileJson2,
  Languages,
  Map,
  MapPinned,
  PencilLine,
  Replace,
  Route,
  Search,
  SquareStack,
  Tag,
  WandSparkles
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  buildingGeometryFromArnisCandidate,
  generateSchematicWithArnisCore,
  queryArnisBuildingCandidates,
  type ArnisBuildingCandidate
} from "./adapters/arnisRustCoreAdapter";
import { SchematicPreviewer } from "./components/SchematicPreviewer";
import { Gaode3DReference } from "./components/Gaode3DReference";
import { GaodeCampusBoundaryEditor, type GaodeMapCaptureRequest, type ReviewMapOverlay } from "./components/GaodeCampusBoundaryEditor";
import { MinecraftBlockPicker } from "./components/MinecraftBlockPicker";
import type {
  BuildingSlot,
  FoundationManifest,
  MapFeature,
  MapFeatureKind
} from "./domain/foundationManifest";
import type {
  BuildingGeometry,
  BuildingGeometryObservation,
  BuildingTarget,
  GeometryConfidence
} from "./domain/buildingGeometry";
import {
  campusOnlineQueryTarget,
  campusTargetFromGaodeCandidate,
  type CampusTarget
} from "./domain/campusTarget";
import {
  foundationManifestPlaceholder,
  createEmptyFoundationManifest
} from "./domain/foundationManifest";
import {
  CandidateSource,
  isAoiCandidate,
  MapCandidate,
  PUTUO_LIBRARY_SEARCH_TARGET,
  PUTUO_ONLINE_QUERY_TARGET
} from "./domain/mapCandidate";
import type {
  MinecraftBlockName,
  PreviewCameraView,
  SchematicModel,
  VisualCheckpointKind,
  VisualComparisonOutcome,
  AxiomCheckDecision,
  AxiomImportResult
} from "./domain/schematicModel";
import type { ExternalModelProvenance, ExternalModelReviewDecision } from "./domain/externalModel";
import type { SourceConflictDecision, SourceConflictRecord } from "./domain/sourceConflict";
import { checkAxiomPlacement, recordAxiomAcceptance } from "./services/axiomAcceptance";
import { externalModelCandidatesFromArnis, summarizeExternalModelCandidates } from "./services/externalModelDiscovery";
import { classifyExternalModelLicense, recordExternalModelReview } from "./services/externalModelReview";
import { recordSourceConflictDecision, sourceConflictsForReview } from "./services/sourceConflictReview";
import { generateSchematicFromBuildingGeometry } from "./services/buildingGeometryToSchematic";
import { pairwiseObservationOverlaps } from "./services/buildingObservation";
import { applyObservationReviewDecision } from "./services/buildingIdentity";
import { buildingSlotToBuildingTarget } from "./services/buildingSlotTarget";
import { prepareDetailedSchematicExport } from "./services/detailedSchematicExport";
import {
  PREVIEW_CAMERA_VIEWS,
  recordCapturedView,
  recordResultComparison,
  visualReviewFor
} from "./services/visualCheckpoints";
import {
  applyManualBuildingGeometryCorrection,
  type ManualBuildingGeometryCorrection
} from "./services/manualBuildingGeometryCorrection";
import {
  generateFoundationSchematicFromManifest,
  previewGeneratedFoundationSchematic,
  type FoundationSchematicPreview
} from "./services/foundationManifestToSchematic";
import { exportFoundationManifestJson } from "./services/foundationManifestExport";
import { saveExportBundleToChosenFolder, utf8Bytes } from "./services/desktopExport";
import {
  applyFoundationStyle,
  DEFAULT_FOUNDATION_STYLE,
  FEATURE_KINDS,
  type FoundationStyleSettings,
  updateFeatureBlockStyle,
  updateRoadWidthStyle
} from "./services/foundationStyle";
import {
  MAX_CAMPUS_BOUNDARY_POINTS,
  addPointToDraft,
  boundaryDraftFromCandidate,
  createEmptyGeometryDraft,
  draftCanCommit,
  draftCanClose,
  draftToManualFeature,
  geometryDraftFromCandidate,
  limitCampusBoundaryDraft,
  type GeometryDraft,
  moveDraftPoint,
  removeDraftPoint,
  replaceDraftPoints,
  removeLastDraftPoint
} from "./services/geometryEditing";
import {
  acceptCandidate,
  buildFoundationManifestFromReviews,
  CandidateReviewStatus,
  makeManualPutuoBoundaryFeature,
  rejectCandidate,
  ReviewedCandidate
} from "./services/candidateReview";
import {
  defaultOnlineMapQueryService,
  OnlineMapQueryResult
} from "./services/onlineMapQuery";
import { clearCampusBuildingProviderCache, createCampusBuildingProvider, createLiveGaodePoiProvider, loadSavedGaodeConfig, mergeCampusBuildingCandidates, saveSavedGaodeConfig } from "./services/liveMapProviders";
import { filterBuildingCandidatesToCampus, filterCampusCandidates } from "./services/campusCandidateFilter";
import {
  canonicalBuildingSourceId,
  findCampusBuildingRecordForGeometry,
  findCampusBuildingSuppression,
  isIncludedCampusBuildingRecord,
  loadCampusBuildingDirectory,
  loadCampusBuildingSuppressions,
  mergeCampusBuildingDirectories,
  suppressCampusBuilding,
  saveCampusBuildingName,
  type CampusBuildingNameRecord,
  type CampusBuildingSuppression
} from "./services/campusBuildingDirectory";
import { loadSharedCampusAnnotations } from "./services/sharedCampusAnnotations";
import {
  candidateCenter,
  candidateInsideCampus,
  clearReverseGeocodeBuildingCandidateCache,
  isCampusAffiliatedName,
  mapWithConcurrency,
  reverseGeocodeBuildingCandidate
} from "./services/campusBuildingNaming";
import { pointCandidate, polygonCandidate } from "./services/mapCandidateFactory";
import { queryCampusBoundaryCandidates } from "./services/campusBoundary";
import {
  featureCounts,
  scopeCandidatesToBoundary,
  targetFromCampusBoundary
} from "./services/campusFeatureReview";
import { queryDeterministicVisualFeatures, queryVisualFeatureProvider } from "./services/visualFeatureProvider";
import { DEFAULT_ARNIS_FOUNDATION_STYLE_PACK, FOUNDATION_STYLE_PRESETS, parseFoundationStylePack, type FoundationStylePack } from "./services/foundationFeatureGenerators";
import { campusAnnotationExportJson, loadLocalCampusAnnotationFile, persistLocalCampusAnnotationFile } from "./services/localCampusAnnotationStore";
import {
  createCampusProject,
  duplicateCampusProject,
  exportCampusProjectJson,
  importCampusProjectJson,
  listCampusProjects,
  loadActiveCampusProject,
  loadCampusProject,
  saveCampusProject,
  type CampusReconstructionProject
} from "./services/campusProjectStore";
import {
  buildingTargetFromLocationAnchors,
  gaodeCandidateToLocationAnchor,
  gaodeMapClickToLocationAnchor,
  gcj02ToWgs84,
  openGeodataAnchorFromGaode,
  wgs84ToGcj02,
  type GaodeLocationAnchor,
  type OpenGeodataQueryAnchor
} from "./services/buildingLocationAnchor";
import {
  type BlockInspection,
  countMatchingBlocks,
  replaceAllMatchingBlocks
} from "./services/schematicEditing";
import { gzipBytes } from "./services/gzip";
import { writeSpongeV2Schematic } from "./services/spongeSchematic";
import {
  languageOptions,
  translations,
  type Language,
  type Translation
} from "./i18n";

type Mode = "foundation" | "detailed";

interface BuildingSlotRefinement {
  id: string;
  slotId: string;
  version: number;
  status: "draft" | "confirmed" | "archived";
  model: SchematicModel;
  createdAt: string;
}

interface ManualCorrectionDraft {
  reason: string;
  useSlotFootprint: boolean;
  heightM: string;
  floors: string;
  roofShape: string;
  roofMaterial: string;
  roofOrientation: string;
  facadeMaterial: string;
  facadeColor: string;
}

const EMPTY_MANUAL_CORRECTION: ManualCorrectionDraft = {
  reason: "",
  useSlotFootprint: false,
  heightM: "",
  floors: "",
  roofShape: "",
  roofMaterial: "",
  roofOrientation: "",
  facadeMaterial: "",
  facadeColor: ""
};

function seamCards(t: Translation) {
  return [
  {
    title: t.seamOnlineTitle,
    body: t.seamOnlineBody,
    icon: Search
  },
  {
    title: t.seamManifestTitle,
    body: t.seamManifestBody,
    icon: FileJson2
  },
  {
    title: t.seamAdapterTitle,
    body: t.seamAdapterBody,
    icon: DatabaseZap
  },
  {
    title: t.seamPreviewerTitle,
    body: t.seamPreviewerBody,
    icon: Replace
  }
];
}

export function App() {
  const [activeMode, setActiveMode] = useState<Mode>("foundation");
  const [language, setLanguage] = useState<Language>("zh-CN");
  const [manifest, setManifest] = useState(() => createEmptyFoundationManifest("未选择校区"));
  const [campusTarget, setCampusTarget] = useState<CampusTarget | null>(null);
  const [campusDirectorySeed, setCampusDirectorySeed] = useState<CampusBuildingNameRecord[]>([]);
  const [slotRefinements, setSlotRefinements] = useState<Record<string, BuildingSlotRefinement[]>>({});
  const saveActiveFoundationProjectRef = useRef<(() => string | null) | null>(null);
  const t = translations[language];

  function confirmCampus(campus: CampusTarget) {
    setCampusTarget(campus);
    setManifest(createEmptyFoundationManifest(campus.canonicalName, campus));
    setActiveMode("foundation");
    const local = loadCampusBuildingDirectory(campus);
    setCampusDirectorySeed(local);
    void Promise.all([loadSharedCampusAnnotations(campus).catch(() => []), loadLocalCampusAnnotationFile(campus)])
      .then(([shared, disk]) => setCampusDirectorySeed(mergeCampusBuildingDirectories(shared, mergeCampusBuildingDirectories(disk, local))));
  }

  function clearCampus() {
    const saveProject = saveActiveFoundationProjectRef.current;
    if (saveProject && window.confirm("切换校区前是否保存当前复刻方案？")) saveProject();
    setCampusTarget(null);
    setManifest(createEmptyFoundationManifest("未选择校区"));
    setCampusDirectorySeed([]);
    setSlotRefinements({});
    saveActiveFoundationProjectRef.current = null;
  }

  function confirmSlotRefinement(slot: BuildingSlot, model: SchematicModel) {
    const version = (slotRefinements[slot.id]?.length ?? 0) + 1;
    const confirmed: BuildingSlotRefinement = { id: `${slot.id}-v${version}`, slotId: slot.id, version, status: "confirmed", model: structuredClone(model), createdAt: new Date().toISOString() };
    setSlotRefinements((current) => {
      const history = current[slot.id] ?? [];
      return { ...current, [slot.id]: [...history, confirmed] };
    });
    setManifest((current) => ({ ...current, buildingSlots: current.buildingSlots.map((item) => item.id === slot.id ? { ...item, currentRefinementId: confirmed.id, currentRefinementVersion: confirmed.version, refinementStatus: "refined" } : item) }));
  }

  function updateFoundationManifest(next: FoundationManifest) {
    setManifest((current) => ({
      ...next,
      buildingSlots: next.buildingSlots.map((slot) => {
        const previous = current.buildingSlots.find((item) => item.id === slot.id);
        return previous?.currentRefinementId ? { ...slot, currentRefinementId: previous.currentRefinementId, currentRefinementVersion: previous.currentRefinementVersion, refinementStatus: previous.refinementStatus } : slot;
      })
    }));
  }

  return (
    <main className="app-shell">
      <aside className="rail" aria-label={t.primaryWorkflow}>
        <div className="brand-mark">
          <Blocks aria-hidden="true" />
        </div>
        <button
          className={activeMode === "foundation" ? "rail-button active" : "rail-button"}
          onClick={() => setActiveMode("foundation")}
          disabled={!campusTarget}
          aria-pressed={activeMode === "foundation"}
          title={t.foundationMode}
        >
          <Map aria-hidden="true" />
        </button>
        <button
          className={activeMode === "detailed" ? "rail-button active" : "rail-button"}
          onClick={() => setActiveMode("detailed")}
          disabled={!campusTarget}
          aria-pressed={activeMode === "detailed"}
          title={t.detailedMode}
        >
          <Building2 aria-hidden="true" />
        </button>
      </aside>

      <section className="workspace">
        <header className="topline">
          <div>
            <p className="eyebrow">{campusTarget?.canonicalName ?? t.campusEyebrow}</p>
            <h1>{t.appTitle}</h1>
          </div>
          <div className="topline-actions">
            {campusTarget ? <button className="secondary-action" onClick={clearCampus}>{t.changeCampus}</button> : null}
            <LanguageSelector language={language} onLanguageChange={setLanguage} t={t} />
            <div className="target-chip">
              <CheckCircle2 aria-hidden="true" />
              {campusTarget?.canonicalName ?? t.firstVerticalSlice}
            </div>
          </div>
        </header>

        {!campusTarget ? (
          <CampusTargetSelector t={t} onConfirm={confirmCampus} />
        ) : <section className="mode-switch" aria-label={t.modeSelector}>
          <button
            className={activeMode === "foundation" ? "mode-tab active" : "mode-tab"}
            onClick={() => setActiveMode("foundation")}
          >
            <Map aria-hidden="true" />
            {t.foundationMode}
          </button>
          <button
            className={activeMode === "detailed" ? "mode-tab active" : "mode-tab"}
            onClick={() => setActiveMode("detailed")}
          >
            <Building2 aria-hidden="true" />
            {t.detailedMode}
          </button>
        </section>}

        {campusTarget ? <section className="hero-panel">
          {activeMode === "foundation" ? (
            <FoundationModePanel key={`foundation-${campusTarget.id}`} campus={campusTarget} refinedSlotIds={Object.keys(slotRefinements)} onManifestChange={updateFoundationManifest} onRegisterProjectSave={(save) => { saveActiveFoundationProjectRef.current = save; }} t={t} />
          ) : (
            <DetailedModeWorkspace key={`detailed-${campusTarget.id}`} campus={campusTarget} slots={manifest.buildingSlots} refinements={slotRefinements} onConfirmRefinement={confirmSlotRefinement} initialBuildingDirectory={campusDirectorySeed} t={t} />
          )}
        </section> : null}

      </section>
    </main>
  );
}

function CampusTargetSelector({ t, onConfirm }: { t: Translation; onConfirm: (campus: CampusTarget) => void }) {
  const [query, setQuery] = useState("华东师范大学普陀校区");
  const [state, setState] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [candidates, setCandidates] = useState<MapCandidate[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [gaodeConfig, setGaodeConfig] = useState(() => loadSavedGaodeConfig());
  const [gaodeConfigMessage, setGaodeConfigMessage] = useState<string | null>(null);

  function saveGaodeConfig() {
    const saved = saveSavedGaodeConfig(gaodeConfig);
    setGaodeConfig(saved);
    setGaodeConfigMessage("高德 API Key 已保存到本机配置记忆。");
  }

  async function search() {
    const trimmed = query.trim();
    if (!trimmed) return;
    try {
      setState("loading");
      setError(null);
      const results = await createLiveGaodePoiProvider().query({
        query: trimmed,
        campus: trimmed,
        center: PUTUO_ONLINE_QUERY_TARGET.center,
        radiusM: 5_000
      });
      const campusCandidates = filterCampusCandidates(results, trimmed);
      setCandidates(campusCandidates);
      setState("ready");
      if (!campusCandidates.length) setError(t.noCampusCandidates);
    } catch (reason) {
      setState("error");
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  return (
    <section className="campus-target-gate" aria-label={t.selectCampus}>
      <div className="panel-icon"><MapPinned aria-hidden="true" /></div>
      <p className="eyebrow">{t.startWithCampus}</p>
      <h2>{t.selectCampus}</h2>
      <p>{t.selectCampusHelp}</p>
      <div className="campus-search-row">
        <label><span>{t.schoolAndCampus}</span><input value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void search(); }} /></label>
        <button className="primary-action" onClick={search} disabled={state === "loading"}><Search aria-hidden="true" />{state === "loading" ? t.querying : t.searchCampus}</button>
      </div>
      <details className="workflow-stage-disclosure gaode-config-panel">
        <summary>高德 API Key 配置</summary>
        <p>填入自己的高德 Web Service Key；如需显示/截图高德 3D 地图，也填 JS API Key 与安全密钥。保存后会记住在本机。</p>
        <div className="project-toolbar">
          <label><span>Web Service Key</span><input value={gaodeConfig.webServiceKey ?? ""} onChange={(event) => setGaodeConfig((current) => ({ ...current, webServiceKey: event.target.value }))} placeholder="用于 POI / 逆地理编码" /></label>
          <label><span>JS API Key</span><input value={gaodeConfig.jsApiKey ?? ""} onChange={(event) => setGaodeConfig((current) => ({ ...current, jsApiKey: event.target.value }))} placeholder="可留空，默认复用 Web Service Key" /></label>
          <label><span>安全密钥 securityJsCode</span><input value={gaodeConfig.securityJsCode ?? ""} onChange={(event) => setGaodeConfig((current) => ({ ...current, securityJsCode: event.target.value }))} placeholder="高德 Web 端安全密钥" /></label>
          <div className="candidate-actions"><button className="secondary-action compact-action" onClick={saveGaodeConfig}>保存高德配置</button></div>
          {gaodeConfigMessage ? <p className="naming-message">{gaodeConfigMessage}</p> : null}
        </div>
      </details>
      {error ? <p className="schematic-error">{error}</p> : null}
      {candidates.length ? <div className="campus-candidate-grid">
        {candidates.slice(0, 10).map((candidate) => <article className="candidate-card" key={candidate.id}>
          <strong>{candidate.name}</strong>
          <p>{candidate.provenance.notes.join(" · ")}</p>
          <button className="primary-action compact-action" onClick={() => onConfirm(campusTargetFromGaodeCandidate(candidate, query))}>{t.useThisCampus}</button>
        </article>)}
      </div> : null}
    </section>
  );
}

function LanguageSelector({
  language,
  onLanguageChange,
  t
}: {
  language: Language;
  onLanguageChange: (language: Language) => void;
  t: Translation;
}) {
  return (
    <label className="language-selector">
      <Languages aria-hidden="true" />
      <span>{t.language}</span>
      <select
        value={language}
        onChange={(event) => onLanguageChange(event.target.value as Language)}
      >
        {languageOptions.map((option) => (
          <option value={option.code} key={option.code}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function FoundationModePanel({
  campus,
  refinedSlotIds,
  onManifestChange,
  onRegisterProjectSave,
  t
}: {
  campus: CampusTarget;
  refinedSlotIds: string[];
  onManifestChange: (manifest: typeof foundationManifestPlaceholder) => void;
  onRegisterProjectSave: (save: (() => string | null) | null) => void;
  t: Translation;
}) {
  const target = campusOnlineQueryTarget(campus);
  const aliveRef = useRef(true);
  const namingRunKeyRef = useRef("");
  const featureMapRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    aliveRef.current = true;
    return () => { aliveRef.current = false; };
  }, []);
  const baseManifest = createEmptyFoundationManifest(campus.canonicalName, campus);
  const emptyBoundary = { ...createEmptyGeometryDraft(target), name: `${campus.canonicalName} boundary`, kind: "campus" as const, points: [] };
  const emptyFeature = { ...createEmptyGeometryDraft(target), name: t.manualFeature, kind: "building" as const, points: [] };
  const featureKinds: MapFeatureKind[] = ["building", "water", "sports", "vegetation", "road"];
  type FeatureReviewStep = "orientation" | MapFeatureKind;
  const featureReviewSteps: FeatureReviewStep[] = ["orientation", ...featureKinds];
  const [project, setProject] = useState<CampusReconstructionProject>(() => loadActiveCampusProject(campus) ?? createCampusProject({
    name: `${campus.canonicalName} 方案 1`, campus, foundation: {
      boundaryDraft: emptyBoundary, candidates: [], reviews: {}, manualFeatures: [],
      foundationStyle: DEFAULT_FOUNDATION_STYLE, foundationStylePack: DEFAULT_ARNIS_FOUNDATION_STYLE_PACK,
      orientationDegrees: campus.orientationDegrees
    }
  }));
  const [projectName, setProjectName] = useState(project.name);
  const [projectBlocksPerMeter, setProjectBlocksPerMeter] = useState(project.campus.blocksPerMeter ?? campus.blocksPerMeter);
  const [availableProjects, setAvailableProjects] = useState(() => listCampusProjects(campus));
  const [projectMessage, setProjectMessage] = useState<string | null>(null);
  const [boundaryDraft, setBoundaryDraft] = useState<GeometryDraft>(() => limitCampusBoundaryDraft(project.foundation.boundaryDraft ?? emptyBoundary));
  const [boundaryHistory, setBoundaryHistory] = useState<GeometryDraft[]>(() => (project.foundation.boundaryHistory ?? []).map(limitCampusBoundaryDraft));
  const [boundaryPointIndex, setBoundaryPointIndex] = useState(0);
  const [boundaryShapeSelected, setBoundaryShapeSelected] = useState(false);
  const [boundaryCandidates, setBoundaryCandidates] = useState<MapCandidate[]>([]);
  const [boundaryState, setBoundaryState] = useState<"idle" | "loading" | "ready" | "manual" | "error">("idle");
  const [boundaryMessage, setBoundaryMessage] = useState<string | null>(null);
  const [queryState, setQueryState] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [queryError, setQueryError] = useState<string | null>(null);
  const [queryResult, setQueryResult] = useState<OnlineMapQueryResult | null>((project.foundation.providerSnapshot as OnlineMapQueryResult | undefined) ?? null);
  const [candidates, setCandidates] = useState<MapCandidate[]>(project.foundation.candidates);
  const [reviews, setReviews] = useState<Record<string, CandidateReviewStatus>>(project.foundation.reviews);
  const [manualFeatures, setManualFeatures] = useState<MapFeature[]>(project.foundation.manualFeatures);
  const [featureDraft, setFeatureDraft] = useState<GeometryDraft>(emptyFeature);
  const [featureHistory, setFeatureHistory] = useState<GeometryDraft[]>(project.foundation.featureHistory ?? []);
  const [featurePointIndex, setFeaturePointIndex] = useState(0);
  const [featureShapeSelected, setFeatureShapeSelected] = useState(false);
  const [orientationMode, setOrientationMode] = useState(false);
  const [orientationDraft, setOrientationDraft] = useState<GeometryDraft>({ ...emptyFeature, id: "campus-orientation", name: t.campusOrientation, kind: "road", points: [] });
  const [orientationDegrees, setOrientationDegrees] = useState(project.foundation.orientationDegrees ?? campus.orientationDegrees);
  const initialOrientationConfirmed = Number.isFinite(project.foundation.orientationDegrees ?? campus.orientationDegrees);
  const [orientationConfirmed, setOrientationConfirmed] = useState(initialOrientationConfirmed);
  const [activeFeatureStep, setActiveFeatureStep] = useState<FeatureReviewStep>(initialOrientationConfirmed ? "building" : "orientation");
  const [completedFeatureSteps, setCompletedFeatureSteps] = useState<Set<FeatureReviewStep>>(() => initialOrientationConfirmed ? new Set(["orientation" as FeatureReviewStep]) : new Set());
  const [activeMapEditor, setActiveMapEditor] = useState<"boundary" | "feature">("boundary");
  const [mapInteractionMode, setMapInteractionMode] = useState<"review" | "orientation" | "manual" | "visual">("review");
  const [selectedCandidateIds, setSelectedCandidateIds] = useState<string[]>([]);
  const [selectedCandidateIndex, setSelectedCandidateIndex] = useState(0);
  const [candidatePopupName, setCandidatePopupName] = useState("");
  const [buildingDirectory, setBuildingDirectory] = useState<CampusBuildingNameRecord[]>(() => loadCampusBuildingDirectory(campus));
  const [namingStates, setNamingStates] = useState<Record<string, "pending" | "matched" | "unmatched" | "failed">>({});
  const [foundationNamingMessage, setFoundationNamingMessage] = useState<string | null>(null);
  const [visibleKinds, setVisibleKinds] = useState<MapFeatureKind[]>(["building"]);
  const [confidenceFilter, setConfidenceFilter] = useState<"high" | "medium" | "low" | "confirmed">("high");
  const [batchReviewHistory, setBatchReviewHistory] = useState<Array<Record<string, CandidateReviewStatus>>>(project.foundation.reviewUndoStack ?? []);
  const [candidatePage, setCandidatePage] = useState(1);
  const [foundationStyle, setFoundationStyle] = useState<FoundationStyleSettings>(project.foundation.foundationStyle ?? DEFAULT_FOUNDATION_STYLE);
  const [foundationStylePack, setFoundationStylePack] = useState<FoundationStylePack>(project.foundation.foundationStylePack ?? DEFAULT_ARNIS_FOUNDATION_STYLE_PACK);
  const [stylePackMessage, setStylePackMessage] = useState<string | null>(null);
  const [foundationExportSummary, setFoundationExportSummary] = useState<string | null>(null);
  const [foundationModel, setFoundationModel] = useState<SchematicModel | null>(null);
  const [foundationPreview, setFoundationPreview] = useState<FoundationSchematicPreview | null>(null);
  const [visualEndpoint, setVisualEndpoint] = useState(import.meta.env.VITE_VISUAL_FEATURE_ENDPOINT ?? "");
  const [visualScreenshot, setVisualScreenshot] = useState("");
  const [visualCaptureMode, setVisualCaptureMode] = useState(false);
  const [visualSelectionMode, setVisualSelectionMode] = useState(false);
  const [visualSelectionPoints, setVisualSelectionPoints] = useState<Array<{ lng: number; lat: number }>>([]);
  const [visualCaptureRequest, setVisualCaptureRequest] = useState<GaodeMapCaptureRequest | null>(null);
  const [visualCaptureFitBounds, setVisualCaptureFitBounds] = useState<{ southWest: { lng: number; lat: number }; northEast: { lng: number; lat: number } } | null>(null);
  const [visualCaptureBoundary, setVisualCaptureBoundary] = useState<Array<{ lng: number; lat: number }> | null>(null);
  const [visualState, setVisualState] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [visualMessage, setVisualMessage] = useState<string | null>(null);
  const campusBoundaryFeature = manualFeatures.find((feature) => feature.kind === "campus") ?? null;

  useEffect(() => {
    let cancelled = false;
    void Promise.all([loadSharedCampusAnnotations(campus), loadLocalCampusAnnotationFile(campus)]).then(([shared, disk]) => {
      if (cancelled) return;
      setBuildingDirectory(mergeCampusBuildingDirectories(shared, mergeCampusBuildingDirectories(disk, loadCampusBuildingDirectory(campus))));
    });
    return () => { cancelled = true; };
  }, [campus.id]);

  useEffect(() => {
    const buildingIds = candidates.filter((candidate) => candidate.kind === "building").map((candidate) => candidate.id).sort().join("|");
    const key = `${project.id}:${buildingIds}`;
    if (!buildingIds || namingRunKeyRef.current === key) return;
    namingRunKeyRef.current = key;
    void nameDiscoveredBuildings(candidates);
  }, [project.id, candidates.length]);

  function nameRecordForCandidate(candidate: MapCandidate) {
    if (candidate.kind !== "building") return undefined;
    return findCampusBuildingRecordForGeometry(
      buildingDirectory,
      candidate.provenance.rawId,
      candidate.geometry.type === "polygon" ? [candidate.geometry.points] : [],
      candidateCenter(candidate)
    );
  }

  useEffect(() => {
    setReviews((current) => {
      let changed = false;
      const next = { ...current };
      for (const candidate of candidates) {
        if (candidate.kind === "building" && next[candidate.id] === "accepted" && !nameRecordForCandidate(candidate)) {
          next[candidate.id] = "pending";
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, [buildingDirectory, candidates]);

  useEffect(() => {
    setCandidates((current) => {
      let changed = false;
      const renamed = current.map((candidate) => {
        const record = candidate.kind === "building" ? nameRecordForCandidate(candidate) : undefined;
        if (!record || candidate.name === record.name) return candidate;
        changed = true;
        return { ...candidate, name: record.name };
      });
      return changed ? renamed : current;
    });
  }, [buildingDirectory]);

  function saveFoundationCandidateName(candidate: MapCandidate, name: string, source: "manual" | "gaode_reverse_geocode" = "manual") {
    const trimmed = name.trim();
    if (!trimmed) return;
    const wgs84 = candidateCenter(candidate), gcj02 = wgs84ToGcj02(wgs84);
    const nextDirectory = saveCampusBuildingName(campus, candidate.provenance.rawId, trimmed, { nameSource: source, wgs84, gcj02 });
    setBuildingDirectory((current) => mergeCampusBuildingDirectories(current.filter((record) => record.nameSource === "shared_annotation"), nextDirectory));
    setCandidates((current) => current.map((item) => item.id === candidate.id ? { ...item, name: trimmed } : item));
    void persistLocalCampusAnnotationFile(campus, nextDirectory);
  }

  async function nameDiscoveredBuildings(discovered: MapCandidate[]) {
    const rank = { high: 3, medium: 2, manual: 2, low: 1 } as const;
    const buildings = discovered.filter((candidate) => candidate.kind === "building").sort((left, right) => rank[right.confidence] - rank[left.confidence]);
    if (!buildings.length) return;
    setNamingStates(Object.fromEntries(buildings.map((candidate) => [candidate.id, nameRecordForCandidate(candidate) ? "matched" : "pending"])));
    const outcomes = await mapWithConcurrency(buildings, 4, async (candidate) => {
      const existingRecord = nameRecordForCandidate(candidate);
      if (existingRecord) return { candidate, state: "matched" as const, record: existingRecord };
      try {
        const { record } = await reverseGeocodeBuildingCandidate(candidate, campus);
        return { candidate, state: record ? "matched" as const : "unmatched" as const, record };
      } catch { return { candidate, state: "failed" as const, record: null }; }
    });
    if (!aliveRef.current) return;
    let directory = loadCampusBuildingDirectory(campus), matched = 0, unmatched = 0, failed = 0;
    const renamed = new globalThis.Map<string, string>();
    const nextStates: Record<string, "pending" | "matched" | "unmatched" | "failed"> = {};
    for (const outcome of outcomes) {
      nextStates[outcome.candidate.id] = outcome.state;
      if (outcome.state === "matched" && outcome.record) {
        directory = saveCampusBuildingName(campus, outcome.candidate.provenance.rawId, outcome.record.name, {
          nameSource: "gaode_reverse_geocode", gcj02: outcome.record.gcj02, wgs84: outcome.record.wgs84
        });
        renamed.set(outcome.candidate.id, outcome.record.name); matched += 1;
      } else if (outcome.state === "unmatched") unmatched += 1;
      else if (outcome.state === "failed") failed += 1;
    }
    setBuildingDirectory((current) => mergeCampusBuildingDirectories(current.filter((record) => record.nameSource === "shared_annotation"), directory));
    setCandidates((current) => current.map((candidate) => renamed.has(candidate.id) ? { ...candidate, name: renamed.get(candidate.id)! } : candidate));
    setNamingStates(nextStates);
    setFoundationNamingMessage(`自动命名完成：匹配 ${matched}，无匹配 ${unmatched}，失败 ${failed}`);
    void persistLocalCampusAnnotationFile(campus, directory);
  }

  function retryNameCurrentBuildingPage() {
    const pageBuildings = pagedCandidates.filter((candidate) => candidate.kind === "building").slice(0, 10);
    if (!pageBuildings.length) return;
    for (const candidate of pageBuildings) clearReverseGeocodeBuildingCandidateCache(candidate, campus);
    setFoundationNamingMessage("正在重新尝试命名当前页建筑候选…");
    void nameDiscoveredBuildings(pageBuildings);
  }

  function currentProjectSnapshot(base = project): CampusReconstructionProject {
    return {
      ...base,
      name: projectName.trim() || base.name,
      campus: { ...base.campus, blocksPerMeter: projectBlocksPerMeter, orientationDegrees },
      foundation: {
        boundaryDraft, candidates, reviews, manualFeatures, foundationStyle, foundationStylePack,
        orientationDegrees, providerSnapshot: queryResult,
        boundaryHistory: boundaryHistory.slice(-50), featureHistory: featureHistory.slice(-50), reviewUndoStack: batchReviewHistory.slice(-50)
      }
    };
  }

  function saveCurrentProjectNow() {
    const snapshot = saveCampusProject(currentProjectSnapshot());
    setProject(snapshot);
    setAvailableProjects(listCampusProjects(campus));
    setProjectMessage(`已保存：${snapshot.name}`);
    return snapshot.name;
  }

  useEffect(() => {
    onRegisterProjectSave(saveCurrentProjectNow);
    return () => onRegisterProjectSave(null);
  });

  useEffect(() => {
    const timer = window.setTimeout(() => {
      saveCampusProject(currentProjectSnapshot());
      setAvailableProjects(listCampusProjects(campus));
    }, 350);
    return () => window.clearTimeout(timer);
  }, [project.id, projectName, projectBlocksPerMeter, boundaryDraft, boundaryHistory, candidates, reviews, batchReviewHistory, manualFeatures, featureHistory, foundationStyle, foundationStylePack, orientationDegrees, queryResult]);

  function loadProjectIntoWorkspace(next: CampusReconstructionProject) {
    setProject(next); setProjectName(next.name);
    setProjectBlocksPerMeter(next.campus.blocksPerMeter ?? campus.blocksPerMeter);
    setBoundaryDraft(limitCampusBoundaryDraft(next.foundation.boundaryDraft ?? emptyBoundary));
    setBoundaryHistory((next.foundation.boundaryHistory ?? []).map(limitCampusBoundaryDraft));
    setCandidates(next.foundation.candidates); setReviews(next.foundation.reviews);
    setManualFeatures(next.foundation.manualFeatures);
    setFeatureHistory(next.foundation.featureHistory ?? []);
    setBatchReviewHistory(next.foundation.reviewUndoStack ?? []);
    setFoundationStyle(next.foundation.foundationStyle ?? DEFAULT_FOUNDATION_STYLE);
    setFoundationStylePack(next.foundation.foundationStylePack ?? DEFAULT_ARNIS_FOUNDATION_STYLE_PACK);
    const nextOrientationKnown = Number.isFinite(next.foundation.orientationDegrees ?? campus.orientationDegrees);
    setOrientationDegrees(next.foundation.orientationDegrees ?? campus.orientationDegrees);
    setOrientationConfirmed(nextOrientationKnown);
    setActiveFeatureStep(nextOrientationKnown ? "building" : "orientation");
    setCompletedFeatureSteps(nextOrientationKnown ? new Set(["orientation" as FeatureReviewStep]) : new Set());
    setVisibleKinds(["building"]);
    setQueryResult((next.foundation.providerSnapshot as OnlineMapQueryResult | undefined) ?? null);
    setBoundaryState(next.foundation.manualFeatures.some((feature) => feature.kind === "campus") ? "ready" : "idle");
    setProjectMessage(`已载入 ${next.name}`);
  }

  function switchProject(nextId: string) {
    if (nextId === project.id) return;
    if (window.confirm(`切换方案前是否保存 ${projectName}？`)) saveCurrentProjectNow();
    const next = loadCampusProject(nextId);
    if (next) loadProjectIntoWorkspace(next);
  }

  function saveProjectAs() {
    const name = window.prompt("新方案名称", `${projectName} 副本`)?.trim();
    if (!name) return;
    const duplicate = duplicateCampusProject(currentProjectSnapshot(), name);
    loadProjectIntoWorkspace(duplicate);
    setAvailableProjects(listCampusProjects(campus));
  }

  async function exportProject() {
    try {
      const snapshot = saveCampusProject(currentProjectSnapshot());
      const saved = await saveExportBundleToChosenFolder([{ fileName: `${snapshot.name.replace(/[^a-zA-Z0-9\u4e00-\u9fff_-]+/g, "_")}.campus-project.json`, bytes: utf8Bytes(exportCampusProjectJson(snapshot)) }]);
      if (saved) setProjectMessage(`项目已导出：${saved.directory}`);
    } catch (reason) { setProjectMessage(reason instanceof Error ? reason.message : String(reason)); }
  }

  async function importProject(file: File) {
    if (!window.confirm("导入会为该校区创建一个新的本地复刻方案，不会覆盖当前方案。继续吗？")) return;
    try {
      const imported = importCampusProjectJson(await file.text(), campus);
      loadProjectIntoWorkspace(imported);
      setAvailableProjects(listCampusProjects(campus));
    } catch (reason) { setProjectMessage(reason instanceof Error ? reason.message : String(reason)); }
  }

  function reviewedFrom(nextCandidates = candidates, nextReviews = reviews) {
    return nextCandidates.flatMap((candidate): ReviewedCandidate[] => {
      const status = nextReviews[candidate.id] ?? "pending";
      return status === "accepted" ? [acceptCandidate(candidate)] : status === "rejected" ? [rejectCandidate(candidate)] : [];
    });
  }

  function buildStyledManifest(
    nextCandidates = candidates,
    nextReviews = reviews,
    nextManualFeatures = manualFeatures,
    nextStyle = foundationStyle
  ) {
    const styled = applyFoundationStyle(
      buildFoundationManifestFromReviews(baseManifest, reviewedFrom(nextCandidates, nextReviews), nextManualFeatures),
      nextStyle
    );
    return { ...styled, target: { ...styled.target, blocksPerMeter: projectBlocksPerMeter, orientationDegrees } };
  }

  useEffect(() => {
    onManifestChange(buildStyledManifest());
  }, [project.id, projectBlocksPerMeter, candidates, reviews, manualFeatures, foundationStyle, orientationDegrees]);

  const reviewedManifest = buildStyledManifest();
  useEffect(() => {
    setFoundationModel(null);
    setFoundationPreview(null);
  }, [project.id, projectBlocksPerMeter, candidates, reviews, manualFeatures, foundationStyle, foundationStylePack, orientationDegrees]);
  const counts = featureCounts(
    candidates.filter((candidate) => (candidate.confidence !== "low" || reviews[candidate.id] === "accepted") && reviews[candidate.id] !== "rejected"),
    manualFeatures.filter((feature) => feature.kind !== "campus")
  );
  const candidateConfidenceCounts = {
    high: candidates.filter((candidate) => candidate.confidence === "high").length,
    medium: candidates.filter((candidate) => candidate.confidence === "medium" || candidate.confidence === "manual").length,
    low: candidates.filter((candidate) => candidate.confidence === "low").length
  };
  const candidateDiscoveryText = queryState === "loading"
    ? "正在查询建筑与基地要素候选，请稍等…"
    : queryState === "ready"
      ? `查询完成：共 ${candidates.length} 个候选，高 ${candidateConfidenceCounts.high} / 中 ${candidateConfidenceCounts.medium} / 低 ${candidateConfidenceCounts.low}`
      : "确认校区边界后开始查询候选。";
  const confidenceRank = { high: 3, medium: 2, manual: 2, low: 1 } as const;
  const filteredCandidates = useMemo(() => candidates
    .filter((candidate) => visibleKinds.includes(candidate.kind))
    .filter((candidate) => confidenceFilter === "confirmed"
      ? reviews[candidate.id] === "accepted"
      : (reviews[candidate.id] ?? "pending") === "pending" && (candidate.confidence === confidenceFilter || (confidenceFilter === "medium" && candidate.confidence === "manual")))
    .sort((left, right) => confidenceRank[right.confidence] - confidenceRank[left.confidence]),
  [candidates, confidenceFilter, reviews, visibleKinds]);
  const pagedCandidates = paginate(filteredCandidates, candidatePage, 10);
  const activeReviewKind: MapFeatureKind | null = activeFeatureStep === "orientation" ? null : activeFeatureStep;
  const candidateReviewUnlocked = Boolean(activeReviewKind && canOpenFeatureStep(activeReviewKind));
  const boundaryEditorPoints = useMemo(() => boundaryDraft.points.map(wgs84ToGcj02), [boundaryDraft.points]);
  const featureEditorPoints = useMemo(
    () => visualSelectionPoints.length
      ? visualSelectionPoints
      : (orientationMode ? orientationDraft.points : featureDraft.points).map(wgs84ToGcj02),
    [featureDraft.points, orientationDraft.points, orientationMode, visualSelectionPoints]
  );
  const featureVisibleKinds = useMemo<MapFeatureKind[]>(() => ["campus", ...visibleKinds], [visibleKinds]);
  const overlayCandidates = useMemo(() => {
    const visibleAccepted = candidates.filter((candidate) => visibleKinds.includes(candidate.kind) && reviews[candidate.id] === "accepted");
    const byId = new globalThis.Map(filteredCandidates.map((candidate) => [candidate.id, candidate]));
    for (const candidate of visibleAccepted) byId.set(candidate.id, candidate);
    return Array.from(byId.values());
  }, [candidates, filteredCandidates, reviews, visibleKinds]);
  const overlays = useMemo<ReviewMapOverlay[]>(() => [
    ...(campusBoundaryFeature ? [{
      id: campusBoundaryFeature.id,
      kind: campusBoundaryFeature.kind,
      status: "manual" as const,
      geometry: { ...campusBoundaryFeature.geometry, points: campusBoundaryFeature.geometry.points.map(wgs84ToGcj02) }
    }] : []),
    ...overlayCandidates.map((candidate) => ({
      id: candidate.id,
      kind: candidate.kind,
      status: reviews[candidate.id] === "accepted" ? "accepted" as const : "pending" as const,
      confidence: candidate.confidence,
      geometry: { ...candidate.geometry, points: candidate.geometry.points.map(wgs84ToGcj02) }
    })),
    ...manualFeatures.filter((feature) => feature.kind !== "campus").map((feature) => ({
      id: feature.id,
      kind: feature.kind,
      status: "manual" as const,
      geometry: { ...feature.geometry, points: feature.geometry.points.map(wgs84ToGcj02) }
    }))
  ], [campusBoundaryFeature, manualFeatures, overlayCandidates, reviews]);

  async function findBoundary() {
    setBoundaryState("loading");
    setBoundaryMessage(null);
    setQueryError(null);
    try {
      const found = await queryCampusBoundaryCandidates(campus);
      if (!aliveRef.current) return;
      setBoundaryCandidates(found);
      if (found[0]) {
        setBoundaryDraft(boundaryDraftFromCandidate(found[0]));
        setBoundaryHistory([]);
        setBoundaryPointIndex(0);
        setBoundaryState("ready");
        setBoundaryMessage(t.autoBoundaryFound);
      } else {
        setBoundaryDraft(emptyBoundary);
        setBoundaryState("manual");
        setBoundaryMessage(t.manualBoundaryRequired);
      }
    } catch (reason) {
      if (!aliveRef.current) return;
      setBoundaryDraft(emptyBoundary);
      setBoundaryState("manual");
      setBoundaryMessage(`${t.autoBoundaryUnavailable}: ${reason instanceof Error ? reason.message : String(reason)}`);
    }
  }

  async function discoverFeatures(boundary: MapFeature) {
    setQueryState("loading");
    setQueryError(null);
    try {
      const scopedTarget = targetFromCampusBoundary(target, boundary.geometry.points);
      const result = await defaultOnlineMapQueryService.queryCampus(scopedTarget);
      if (!aliveRef.current) return;
      const scoped = scopeCandidatesToBoundary(result.candidates, boundary.geometry.points);
      const nextCandidates = scoped.map((item) => item.candidate);
      const nextReviews = Object.fromEntries(scoped.map((item) => [item.candidate.id, "pending"])) as Record<string, CandidateReviewStatus>;
      setQueryResult(result);
      setCandidates(nextCandidates);
      setReviews(nextReviews);
      setQueryState("ready");
      const nextManual = [...manualFeatures.filter((feature) => feature.kind !== "campus"), boundary];
      onManifestChange(buildStyledManifest(nextCandidates, nextReviews, nextManual));
      if (!nextCandidates.length) setQueryError(t.noFeatureGeometry);
    } catch (reason) {
      if (!aliveRef.current) return;
      setQueryState("error");
      setQueryError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  function changeBoundaryPoints(gcjPoints: Array<{ lng: number; lat: number }>) {
    setBoundaryHistory((history) => [...history, boundaryDraft]);
    setBoundaryDraft((draft) => limitCampusBoundaryDraft(replaceDraftPoints(draft, gcjPoints.map(gcj02ToWgs84))));
  }

  function addBoundaryPoint(point: { lng: number; lat: number }) {
    if (boundaryDraft.points.length >= MAX_CAMPUS_BOUNDARY_POINTS) {
      setBoundaryMessage(`校区边界最多支持 ${MAX_CAMPUS_BOUNDARY_POINTS} 个顶点；请拖动或删除现有顶点。`);
      return;
    }
    setBoundaryHistory((history) => [...history, boundaryDraft]);
    setBoundaryDraft((draft) => addPointToDraft(draft, gcj02ToWgs84(point)));
    setBoundaryPointIndex(boundaryDraft.points.length);
  }

  function deleteBoundaryPoint() {
    if (!boundaryDraft.points[boundaryPointIndex]) return;
    setBoundaryHistory((history) => [...history, boundaryDraft]);
    setBoundaryDraft((draft) => removeDraftPoint(draft, boundaryPointIndex));
    setBoundaryPointIndex((index) => Math.max(0, index - 1));
  }

  function undoBoundary() {
    const previous = boundaryHistory[boundaryHistory.length - 1];
    if (!previous) return;
    setBoundaryDraft(limitCampusBoundaryDraft(previous));
    setBoundaryHistory((history) => history.slice(0, -1));
  }

  async function confirmBoundary() {
    if (!draftCanClose(boundaryDraft)) return;
    const boundary = {
      ...draftToManualFeature({ ...boundaryDraft, kind: "campus", name: `${campus.canonicalName} boundary` }, 1),
      id: `feature-campus-boundary-${campus.id}`
    };
    const nextManual = [...manualFeatures.filter((feature) => feature.kind !== "campus"), boundary];
    setManualFeatures(nextManual);
    setBoundaryShapeSelected(false);
    setBoundaryState("ready");
    setBoundaryMessage(t.campusBoundaryConfirmed);
    onManifestChange(buildStyledManifest(candidates, reviews, nextManual));
    await discoverFeatures(boundary);
  }

  function startManualFeature(kind: MapFeatureKind) {
    setMapInteractionMode("manual");
    setOrientationMode(false);
    setVisualSelectionMode(false);
    setVisualSelectionPoints([]);
    setFeatureShapeSelected(false);
    setFeatureDraft({ ...emptyFeature, id: `manual-${kind}-${Date.now()}`, name: `${t.manualFeature} · ${featureKindLabel(kind, t)}`, kind, points: [] });
    setFeatureHistory([]);
    setFeaturePointIndex(0);
  }

  function changeFeaturePoints(gcjPoints: Array<{ lng: number; lat: number }>) {
    setFeatureHistory((history) => [...history, featureDraft]);
    setFeatureDraft((draft) => replaceDraftPoints(draft, gcjPoints.map(gcj02ToWgs84)));
  }

  function addFeaturePoint(point: { lng: number; lat: number }) {
    if (visualSelectionMode) {
      if (!visualSelectionPoints.length) { setVisualSelectionPoints([point]); return; }
      const first = visualSelectionPoints[0];
      const west = Math.min(first.lng, point.lng), east = Math.max(first.lng, point.lng);
      const south = Math.min(first.lat, point.lat), north = Math.max(first.lat, point.lat);
      const rectangle = [{ lng: west, lat: south }, { lng: east, lat: south }, { lng: east, lat: north }, { lng: west, lat: north }, { lng: west, lat: south }];
      setVisualSelectionPoints(rectangle);
      setVisualCaptureRequest({ id: `visual-capture-${Date.now()}`, southWest: { lng: west, lat: south }, northEast: { lng: east, lat: north } });
      setVisualSelectionMode(false);
      setMapInteractionMode("review");
      setVisualState("loading"); setVisualMessage("正在生成无标签俯视截图…");
      return;
    }
    if (orientationMode) {
      const wgs = gcj02ToWgs84(point);
      setOrientationDraft((draft) => ({ ...draft, points: draft.points.length >= 2 ? [draft.points[0], wgs] : [...draft.points, wgs] }));
      return;
    }
    if (mapInteractionMode !== "manual") {
      if (mapInteractionMode === "review") setSelectedCandidateIds([]);
      return;
    }
    setFeatureHistory((history) => [...history, featureDraft]);
    setFeatureDraft((draft) => addPointToDraft(draft, gcj02ToWgs84(point)));
    setFeaturePointIndex(featureDraft.points.length);
  }

  function beginCampusOrientation() {
    setMapInteractionMode("orientation");
    setOrientationMode(true);
    setOrientationDraft((draft) => ({ ...draft, points: [] }));
    setVisualSelectionMode(false);
    setVisualSelectionPoints([]);
    setVisualCaptureRequest(null);
    setFeatureShapeSelected(false);
    setFeaturePointIndex(0);
    setActiveMapEditor("feature");
  }

  function confirmCampusOrientation() {
    if (orientationDraft.points.length !== 2) return;
    const [start, end] = orientationDraft.points;
    const east = (end.lng - start.lng) * 111_320 * Math.cos(((start.lat + end.lat) / 2) * Math.PI / 180);
    const north = (end.lat - start.lat) * 111_320;
    const sourceAngle = Math.atan2(north, east) * 180 / Math.PI;
    const nearestAxis = Math.round(sourceAngle / 90) * 90;
    setOrientationDegrees(nearestAxis - sourceAngle);
    setOrientationConfirmed(true);
    setCompletedFeatureSteps((steps) => new Set([...steps, "orientation"]));
    setActiveFeatureStep("building");
    setVisibleKinds(["building"]);
    setOrientationMode(false);
    setMapInteractionMode("review");
  }

  function deleteFeaturePoint() {
    if (!featureDraft.points[featurePointIndex]) return;
    setFeatureHistory((history) => [...history, featureDraft]);
    setFeatureDraft((draft) => removeDraftPoint(draft, featurePointIndex));
    setFeaturePointIndex((index) => Math.max(0, index - 1));
  }

  function undoFeature() {
    const previous = featureHistory[featureHistory.length - 1];
    if (!previous) return;
    setFeatureDraft(previous);
    setFeatureHistory((history) => history.slice(0, -1));
  }

  function commitFeatureDraft() {
    if (!draftCanCommit(featureDraft)) return;
    const feature = {
      ...draftToManualFeature(featureDraft, manualFeatures.length + 1),
      id: `feature-manual-${featureDraft.kind}-${Date.now()}`
    };
    const nextManual = [...manualFeatures, feature];
    const nextReviews = reviews;
    setManualFeatures(nextManual);
    setReviews(nextReviews);
    setFeatureDraft(emptyFeature);
    setMapInteractionMode("review");
    onManifestChange(buildStyledManifest(candidates, nextReviews, nextManual));
  }

  function updateReview(candidate: MapCandidate, status: CandidateReviewStatus) {
    if (status === "pending" && reviews[candidate.id] === "accepted" && refinedSlotIds.includes(`slot-feature-${candidate.id}`) && !window.confirm("该建筑已有精细建筑版本。撤销确认会暂时从精细模式移除，但不会删除历史版本。继续吗？")) return;
    if (status === "accepted" && candidate.kind === "building" && !nameRecordForCandidate(candidate)) {
      setSelectedCandidateIds([candidate.id]);
      setSelectedCandidateIndex(0);
      setCandidatePopupName("");
      setFoundationNamingMessage("该建筑缺少名称，请先在地图弹窗中填写名称。");
      return;
    }
    const next = { ...reviews, [candidate.id]: status };
    setReviews(next);
    onManifestChange(buildStyledManifest(candidates, next, manualFeatures));
  }

  function openCandidatePopup(candidateId: string, gcjPoint: { lng: number; lat: number } | null) {
    const selected = candidates.find((candidate) => candidate.id === candidateId);
    if (!selected || mapInteractionMode !== "review") return;
    const point = gcjPoint ? gcj02ToWgs84(gcjPoint) : candidateCenter(selected);
    const group = candidates.filter((candidate) =>
      candidate.kind === selected.kind && reviews[candidate.id] !== "rejected" && candidateGeometryHitsPoint(candidate, point)
    );
    const ordered = [selected, ...group.filter((candidate) => candidate.id !== selected.id)];
    setSelectedCandidateIds(ordered.map((candidate) => candidate.id));
    setSelectedCandidateIndex(0);
    setCandidatePopupName(selected.kind === "building" ? nameRecordForCandidate(selected)?.name ?? "" : selected.name);
  }

  function viewBuildingCandidateIn3d(candidate: MapCandidate) {
    setActiveFeatureStep(candidate.kind);
    setVisibleKinds([candidate.kind]);
    setConfidenceFilter(reviews[candidate.id] === "accepted" ? "confirmed" : candidate.confidence === "manual" ? "medium" : candidate.confidence);
    setCandidatePage(1);
    setSelectedCandidateIds([candidate.id]);
    setSelectedCandidateIndex(0);
    setCandidatePopupName(candidate.kind === "building" ? nameRecordForCandidate(candidate)?.name ?? "" : candidate.name);
    setMapInteractionMode("review");
    window.setTimeout(() => featureMapRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" }), 0);
  }

  function selectPopupCandidate(index: number) {
    const candidate = candidates.find((item) => item.id === selectedCandidateIds[index]);
    setSelectedCandidateIndex(index);
    if (candidate) setCandidatePopupName(candidate.kind === "building" ? nameRecordForCandidate(candidate)?.name ?? "" : candidate.name);
  }

  function confirmCandidateFromPopup(candidate: MapCandidate) {
    if (candidate.kind === "building") {
      if (!candidatePopupName.trim()) { setFoundationNamingMessage("建筑名称不能为空。"); return; }
      saveFoundationCandidateName(candidate, candidatePopupName, nameRecordForCandidate(candidate)?.nameSource === "gaode_reverse_geocode" ? "gaode_reverse_geocode" : "manual");
    }
    const next = { ...reviews, [candidate.id]: "accepted" as const };
    for (const id of selectedCandidateIds) if (id !== candidate.id) next[id] = "merged";
    setReviews(next);
    setSelectedCandidateIds([]);
    onManifestChange(buildStyledManifest(candidates.map((item) => item.id === candidate.id ? { ...item, name: candidatePopupName.trim() || item.name } : item), next, manualFeatures));
  }

  function rejectCandidateFromPopup(candidate: MapCandidate) {
    updateReview(candidate, "rejected");
    setSelectedCandidateIds([]);
  }

  function revokeCandidateConfirmation(candidate: MapCandidate) {
    const slotId = `slot-feature-${candidate.id}`;
    if (refinedSlotIds.includes(slotId) && !window.confirm("该建筑已有精细建筑版本。撤销确认会暂时从精细模式移除，但不会删除历史版本。继续吗？")) return;
    const next = { ...reviews, [candidate.id]: "pending" as const };
    for (const id of selectedCandidateIds) if (next[id] === "merged") next[id] = "pending";
    setReviews(next);
    onManifestChange(buildStyledManifest(candidates, next, manualFeatures));
    setSelectedCandidateIds([]);
  }

  function updateStyle(nextStyle: FoundationStyleSettings) {
    setFoundationStyle(nextStyle);
    onManifestChange(buildStyledManifest(candidates, reviews, manualFeatures, nextStyle));
  }

  function selectFoundationStylePack(pack: FoundationStylePack) {
    setFoundationStylePack(pack);
    setFoundationStyle((current) => ({
      roadWidthBlocks: pack.features.road?.width ?? current.roadWidthBlocks,
      blocks: Object.fromEntries(FEATURE_KINDS.map((kind) => [
        kind,
        (pack.features[kind]?.blocks[0] ?? current.blocks[kind]).replace(/^minecraft:/, "")
      ])) as FoundationStyleSettings["blocks"]
    }));
  }

  function toggleLayer(kind: MapFeatureKind) {
    if (!canOpenFeatureStep(kind)) return;
    setActiveFeatureStep(kind);
    setVisibleKinds([kind]);
    setCandidatePage(1);
  }

  function showAllLayers() {
    const available = featureKinds.filter(canOpenFeatureStep);
    setVisibleKinds(available.length ? available : ["building"]);
    setCandidatePage(1);
  }

  function canOpenFeatureStep(step: FeatureReviewStep) {
    if (step === "orientation") return true;
    if (!orientationConfirmed) return false;
    const index = featureReviewSteps.indexOf(step);
    return featureReviewSteps.slice(0, index).every((previous) => completedFeatureSteps.has(previous));
  }

  function featureStepStatus(step: FeatureReviewStep) {
    if (completedFeatureSteps.has(step)) return t.stepDone;
    if (step === activeFeatureStep) return t.stepActive;
    return canOpenFeatureStep(step) ? t.notStarted : t.stepLocked;
  }

  function featureStepLabel(step: FeatureReviewStep) {
    if (step === "orientation") return t.drawCampusOrientation;
    if (step === "building") return t.reviewBuildings;
    if (step === "water") return t.reviewWater;
    if (step === "sports") return t.reviewSports;
    if (step === "vegetation") return t.reviewVegetation;
    return t.reviewRoads;
  }

  function pendingCountForStep(step: FeatureReviewStep) {
    if (step === "orientation") return orientationConfirmed ? 0 : 1;
    return candidates.filter((candidate) => candidate.kind === step && (reviews[candidate.id] ?? "pending") === "pending").length;
  }

  function activateFeatureStep(step: FeatureReviewStep) {
    if (!canOpenFeatureStep(step)) return;
    setActiveFeatureStep(step);
    setCandidatePage(1);
    setSelectedCandidateIds([]);
    if (step !== "orientation") setVisibleKinds([step]);
  }

  function completeActiveFeatureStep() {
    if (activeFeatureStep === "orientation") {
      if (!orientationConfirmed) {
        setFoundationNamingMessage(t.orientationRequired);
        return;
      }
      setCompletedFeatureSteps((steps) => new Set([...steps, "orientation"]));
      setActiveFeatureStep("building");
      setVisibleKinds(["building"]);
      return;
    }
    const pending = pendingCountForStep(activeFeatureStep);
    if (pending && !window.confirm(`${pending} ${t.pendingCandidates}。${t.completeStep}?`)) return;
    setCompletedFeatureSteps((steps) => new Set([...steps, activeFeatureStep]));
    const currentIndex = featureReviewSteps.indexOf(activeFeatureStep);
    const next = featureReviewSteps[currentIndex + 1];
    if (next) {
      setActiveFeatureStep(next);
      setCandidatePage(1);
      setSelectedCandidateIds([]);
      if (next !== "orientation") setVisibleKinds([next]);
    }
  }

  function applyBatchReview(status: CandidateReviewStatus) {
    if (!filteredCandidates.length) return;
    const eligible = status === "accepted"
      ? filteredCandidates.filter((candidate) => candidate.kind !== "building" || nameRecordForCandidate(candidate))
      : filteredCandidates;
    const skipped = filteredCandidates.length - eligible.length;
    if (!eligible.length) { setFoundationNamingMessage(`没有可确认候选；缺少名称 ${skipped}`); return; }
    if (!window.confirm(`${status === "accepted" ? "确认" : "拒绝"}当前筛选的 ${eligible.length} 个候选${skipped ? `（缺少名称跳过 ${skipped}）` : ""}？`)) return;
    setBatchReviewHistory((history) => [...history, reviews].slice(-50));
    const next = { ...reviews };
    for (const candidate of eligible) next[candidate.id] = status;
    setReviews(next);
    if (status === "accepted") setFoundationNamingMessage(`批量确认 ${eligible.length}，缺少名称跳过 ${skipped}`);
    onManifestChange(buildStyledManifest(candidates, next, manualFeatures));
  }

  function undoBatchReview() {
    const previous = batchReviewHistory.at(-1);
    if (!previous) return;
    setReviews(previous);
    setBatchReviewHistory((history) => history.slice(0, -1));
    onManifestChange(buildStyledManifest(candidates, previous, manualFeatures));
  }

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const targetElement = event.target as HTMLElement | null;
      if (targetElement?.matches("input, textarea, select, [contenteditable='true']")) return;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z") {
        event.preventDefault();
        if (activeMapEditor === "feature") undoFeature(); else undoBoundary();
      }
      if (event.key === "Delete" || event.key === "Backspace") {
        event.preventDefault();
        if (activeMapEditor === "feature") deleteFeaturePoint(); else deleteBoundaryPoint();
      }
      if (event.key === "Escape") {
        setBoundaryShapeSelected(false);
        setFeatureShapeSelected(false);
        setOrientationMode(false);
        setVisualSelectionMode(false);
        setMapInteractionMode("review");
        setSelectedCandidateIds([]);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });

  async function retryFeatureDiscovery() {
    if (!campusBoundaryFeature) return;
    await discoverFeatures(campusBoundaryFeature);
  }

  function beginVisualSelection() {
    if (!campusBoundaryFeature) return;
    const gcjBoundary = campusBoundaryFeature.geometry.points.map(wgs84ToGcj02);
    const bounds = {
      minLng: Math.min(...gcjBoundary.map((point) => point.lng)),
      minLat: Math.min(...gcjBoundary.map((point) => point.lat)),
      maxLng: Math.max(...gcjBoundary.map((point) => point.lng)),
      maxLat: Math.max(...gcjBoundary.map((point) => point.lat))
    };
    const padLng = Math.max((bounds.maxLng - bounds.minLng) * 0.06, 0.0003);
    const padLat = Math.max((bounds.maxLat - bounds.minLat) * 0.06, 0.0003);
    setMapInteractionMode("visual");
    setOrientationMode(false);
    setFeatureShapeSelected(false);
    setSelectedCandidateIds([]);
    setVisualSelectionMode(false);
    setVisualSelectionPoints([]);
    setVisualScreenshot("");
    setVisualCaptureBoundary(null);
    setVisualCaptureRequest(null);
    setVisualCaptureFitBounds({
      southWest: { lng: bounds.minLng - padLng, lat: bounds.minLat - padLat },
      northEast: { lng: bounds.maxLng + padLng, lat: bounds.maxLat + padLat }
    });
    setVisualCaptureMode(true);
    setVisualState("idle");
    setVisualMessage("已进入无标签俯视取景。请在地图中缩放、平移，细节合适后点击“截取当前视野”。");
    window.setTimeout(() => featureMapRef.current?.scrollIntoView({ block: "center", behavior: "smooth" }), 0);
  }

  function captureCurrentVisualView() {
    if (!visualCaptureMode) return;
    setVisualState("loading");
    setVisualMessage("正在截取当前无标签视野…");
    setVisualCaptureRequest({ id: `visual-current-view-${Date.now()}` });
  }

  function beginManualVisualSelection() {
    setMapInteractionMode("visual");
    setOrientationMode(false);
    setFeatureShapeSelected(false);
    setSelectedCandidateIds([]);
    setVisualSelectionMode(true);
    setVisualSelectionPoints([]);
    setVisualScreenshot("");
    setVisualCaptureBoundary(null);
    setVisualCaptureRequest(null);
    setVisualState("idle");
    setVisualMessage("请在高德 3D 校区底图上点击两点框选补缺区域；系统会再次隐藏标签并截图。");
    window.setTimeout(() => featureMapRef.current?.scrollIntoView({ block: "center", behavior: "smooth" }), 0);
  }

  async function runDeterministicVisualRecovery() {
    if (!campusBoundaryFeature || !visualScreenshot) return;
    setVisualState("loading"); setVisualMessage(null);
    try {
      const visual = await queryDeterministicVisualFeatures({ imageDataUrl: visualScreenshot, boundary: visualCaptureBoundary ?? campusBoundaryFeature.geometry.points, campus: campus.canonicalName });
      const activeRecoveryKind = activeFeatureStep === "orientation" ? null : activeFeatureStep;
      const scoped = scopeCandidatesToBoundary(visual, campusBoundaryFeature.geometry.points)
        .map((item) => item.candidate)
        .filter((candidate) => !activeRecoveryKind || candidate.kind === activeRecoveryKind);
      const existing = new Set(candidates.map((candidate) => candidate.provenance.rawId));
      const additions = scoped.filter((candidate) => !existing.has(candidate.provenance.rawId));
      const nextCandidates = [...candidates, ...additions];
      const nextReviews = { ...reviews, ...Object.fromEntries(additions.map((candidate) => [candidate.id, "pending"])) } as Record<string, CandidateReviewStatus>;
      setCandidates(nextCandidates); setReviews(nextReviews); setVisualState("ready"); setConfidenceFilter("medium");
      setVisualMessage(`规则识别加入 ${additions.length} 个低置信度候选，请逐一确认。`);
    } catch (reason) { setVisualState("error"); setVisualMessage(reason instanceof Error ? reason.message : String(reason)); }
  }

  async function runVisualFeatureProvider() {
    if (!campusBoundaryFeature || !visualScreenshot) return;
    setVisualState("loading"); setVisualMessage(null);
    try {
      const visual = await queryVisualFeatureProvider({ endpoint: visualEndpoint, imageDataUrl: visualScreenshot, boundary: visualCaptureBoundary ?? campusBoundaryFeature.geometry.points, campus: campus.canonicalName });
      if (!aliveRef.current) return;
      const activeRecoveryKind = activeFeatureStep === "orientation" ? null : activeFeatureStep;
      const scoped = scopeCandidatesToBoundary(visual, campusBoundaryFeature.geometry.points).map((item) => item.candidate).filter((candidate) => !activeRecoveryKind || candidate.kind === activeRecoveryKind);
      const existing = new Set(candidates.map((candidate) => candidate.provenance.rawId));
      const additions = scoped.filter((candidate) => !existing.has(candidate.provenance.rawId));
      const nextCandidates = [...candidates, ...additions];
      const nextReviews = { ...reviews, ...Object.fromEntries(additions.map((candidate) => [candidate.id, "pending"])) } as Record<string, CandidateReviewStatus>;
      setCandidates(nextCandidates); setReviews(nextReviews); setVisualState("ready");
      setVisualMessage(`模型识别加入 ${additions.length} 个候选，请逐一确认。`);
      onManifestChange(buildStyledManifest(nextCandidates, nextReviews, manualFeatures));
    } catch (reason) {
      if (!aliveRef.current) return;
      setVisualState("error"); setVisualMessage(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function exportFoundationSchematic() {
    try {
      const exportResult = generateFoundationSchematicFromManifest(reviewedManifest, { roadWidthBlocks: foundationStyle.roadWidthBlocks, stylePack: foundationStylePack });
      const bytes = gzipBytes(writeSpongeV2Schematic(exportResult.model));
      const manifestExport = exportFoundationManifestJson(reviewedManifest, `${exportResult.model.name}.foundation_manifest.json`);
      const saved = await saveExportBundleToChosenFolder([
        { fileName: `${exportResult.model.name}.schem`, bytes },
        { fileName: manifestExport.fileName, bytes: utf8Bytes(manifestExport.json) }
      ]);
      if (!saved) return;
      setFoundationExportSummary(`${t.foundationExported}: ${saved.directory} · ${exportResult.model.width} x ${exportResult.model.height} x ${exportResult.model.length}`);
    } catch (error) { setFoundationExportSummary(error instanceof Error ? error.message : String(error)); }
  }

  function generateFoundation3dPreview() {
    if (!reviewedManifest.mapFeatures.length) return;
    const generated = generateFoundationSchematicFromManifest(reviewedManifest, {
      roadWidthBlocks: foundationStyle.roadWidthBlocks,
      stylePack: foundationStylePack
    });
    setFoundationModel(generated.model);
    setFoundationPreview(previewGeneratedFoundationSchematic(generated));
  }

  const popupCandidates = selectedCandidateIds.flatMap((id) => {
    const candidate = candidates.find((item) => item.id === id);
    return candidate ? [candidate] : [];
  });
  const popupCandidate = popupCandidates[selectedCandidateIndex] ?? null;
  const candidatePopup = popupCandidate ? <CandidateMapPopup
    candidate={popupCandidate}
    candidates={popupCandidates}
    activeIndex={selectedCandidateIndex}
    name={candidatePopupName}
    namingState={namingStates[popupCandidate.id]}
    review={reviews[popupCandidate.id] ?? "pending"}
    onNameChange={setCandidatePopupName}
    onSaveName={() => saveFoundationCandidateName(popupCandidate, candidatePopupName)}
    onSelect={selectPopupCandidate}
    onConfirm={() => confirmCandidateFromPopup(popupCandidate)}
    onReject={() => rejectCandidateFromPopup(popupCandidate)}
    onRevoke={() => revokeCandidateConfirmation(popupCandidate)}
    onClose={() => setSelectedCandidateIds([])}
    t={t}
  /> : null;

  return <div className="mode-panel">
    <div className="panel-icon"><Route aria-hidden="true" /></div>
    <p className="eyebrow">{t.mode01}</p><h2>{t.foundationMode}</h2><p>{t.foundationBody}</p>
    <section className="project-toolbar" aria-label="校园复刻项目">
      <div className="candidate-heading"><p className="mini-label">{t.projectGate}</p><strong>{projectName}</strong></div>
      <p>{t.projectGateHelp}</p>
      <label><span>{t.currentProject}</span><select value={project.id} onChange={(event) => switchProject(event.target.value)}><option value={project.id}>{projectName}</option>{availableProjects.filter((item) => item.id !== project.id).map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}</select></label>
      <label><span>{t.projectName}</span><input value={projectName} onChange={(event) => setProjectName(event.target.value)} /></label>
      <div className="candidate-actions"><button className="secondary-action" onClick={saveProjectAs}>{t.saveProjectAs}</button><button className="secondary-action" onClick={exportProject}>{t.exportProject}</button><label className="secondary-action">{t.importProject}<input type="file" accept="application/json,.json,.campus-project.json" hidden onChange={(event) => { const file = event.target.files?.[0]; if (file) void importProject(file); event.currentTarget.value = ""; }} /></label></div>
      <small>{t.autosaveProjectMeta.replace("{version}", project.minecraftTargetVersion).replace("{schema}", project.schemaVersion)}</small>
      {projectMessage ? <p className="naming-message">{projectMessage}</p> : null}
    </section>
    <section className="foundation-parameter-stage" aria-label={t.foundationParameters}>
      <div className="candidate-heading"><p className="mini-label">01 · {t.foundationParameters}</p><strong>{projectName}</strong></div>
      <p>{t.foundationParametersHelp}</p>
      <label className="campus-scale-control"><span>{t.campusScale}</span><input type="number" min="0.1" max="8" step="0.1" value={projectBlocksPerMeter} onChange={(event) => setProjectBlocksPerMeter(Math.max(0.1, Number(event.target.value) || 1))} /><small>{t.campusScaleHelp}</small></label>
      <FoundationStylePresetPanel value={foundationStylePack} message={stylePackMessage} onChange={selectFoundationStylePack} onMessage={setStylePackMessage} t={t} />
      <FoundationStylePanel style={foundationStyle} t={t} onStyleChange={updateStyle} />
    </section>
    <section className="foundation-boundary-stage" aria-label={t.campusBoundaryEditor}>
      <div className="candidate-heading"><p className="mini-label">02 · {t.campusBoundaryEditor}</p><strong>{boundaryState === "ready" ? t.boundaryReady : t.boundaryNeedsReview}</strong></div>
      <GaodeCampusBoundaryEditor center={campus.center} points={boundaryEditorPoints} maxPoints={MAX_CAMPUS_BOUNDARY_POINTS} selectedPointIndex={boundaryPointIndex} shapeSelected={boundaryShapeSelected} onSelectShape={() => setBoundaryShapeSelected(true)} onDeselect={() => setBoundaryShapeSelected(false)} onActivate={() => setActiveMapEditor("boundary")} onSelectPoint={(index) => { setBoundaryPointIndex(index); setBoundaryShapeSelected(true); }} onAddPoint={addBoundaryPoint} onChangePoints={changeBoundaryPoints} viewStorageKey={`boundary:${campus.id}`} t={t} />
      <div className="actions"><button className="primary-action" onClick={findBoundary} disabled={boundaryState === "loading"}><Search aria-hidden="true" />{boundaryState === "loading" ? t.querying : t.findCampusBoundary}</button></div>
      {boundaryMessage ? <p className="naming-message">{boundaryMessage}</p> : null}
      {boundaryCandidates.length ? <div className="boundary-candidate-list">{boundaryCandidates.slice(0, 5).map((candidate) => <button className="review-button" key={candidate.id} onClick={() => { setBoundaryHistory((history) => [...history, boundaryDraft]); setBoundaryDraft(boundaryDraftFromCandidate(candidate)); }}>{candidate.name} · {candidate.confidence}</button>)}</div> : null}
      <div className="candidate-actions">
        <button className="secondary-action" onClick={undoBoundary} disabled={!boundaryHistory.length}>{t.undoPoint}</button>
        <button className="secondary-action" onClick={deleteBoundaryPoint} disabled={!boundaryDraft.points.length}>{t.deleteVertex}</button>
        {boundaryShapeSelected ? <button className="secondary-action" onClick={() => setBoundaryShapeSelected(false)}>完成编辑</button> : null}
        <button className="secondary-action" onClick={() => { setBoundaryHistory((history) => [...history, boundaryDraft]); setBoundaryDraft(emptyBoundary); setBoundaryShapeSelected(false); setBoundaryState("manual"); }}>{t.clearBoundary}</button>
        <button className="primary-action" onClick={confirmBoundary} disabled={!draftCanClose(boundaryDraft) || queryState === "loading"}>{t.confirmCampusBoundary}</button>
      </div>
    </section>
    {campusBoundaryFeature ? <section className="feature-review-stage" aria-label={t.foundationFeatureReviewMap}>
      <div className="candidate-heading"><p className="mini-label">03 · {t.foundationFeatureReviewMap}</p><strong>{queryState === "loading" ? t.querying : `${counts.building + counts.road + counts.water + counts.vegetation + counts.sports} ${t.features}`}</strong></div>
      <div className={queryState === "loading" ? "candidate-discovery-summary loading" : "candidate-discovery-summary"}><strong>{candidateDiscoveryText}</strong></div>
      <div className="feature-stepper" aria-label={t.featureReviewStep}>{featureReviewSteps.map((step, index) => <button className={step === activeFeatureStep ? "feature-step active" : completedFeatureSteps.has(step) ? "feature-step done" : canOpenFeatureStep(step) ? "feature-step" : "feature-step locked"} disabled={!canOpenFeatureStep(step)} onClick={() => activateFeatureStep(step)} key={step}><span>{index + 1}</span><strong>{featureStepLabel(step)}</strong><small>{featureStepStatus(step)} · {pendingCountForStep(step)}</small></button>)}</div>
      <div className="feature-coverage-grid"><button className={visibleKinds.length === featureKinds.filter(canOpenFeatureStep).length ? "layer-toggle active" : "layer-toggle"} onClick={showAllLayers}><SquareStack aria-hidden="true" />{t.showAll} <strong>{candidates.length}</strong></button>{featureKinds.map((kind) => <button className={visibleKinds.length === 1 && visibleKinds[0] === kind ? `layer-toggle ${kind} active` : `layer-toggle ${kind}`} onClick={() => toggleLayer(kind)} disabled={!canOpenFeatureStep(kind)} key={kind}><span />{featureKindLabel(kind, t)} <strong>{counts[kind as keyof typeof counts]}</strong></button>)}</div>
      {queryError ? <div className="schematic-error">{queryError}</div> : null}
      <div ref={featureMapRef} className="foundation-gaode-3d-focus"><GaodeCampusBoundaryEditor center={campus.center} points={featureEditorPoints} geometryType={visualSelectionPoints.length ? "polygon" : orientationMode || featureDraft.kind === "road" ? "polyline" : "polygon"} overlays={overlays} visibleKinds={featureVisibleKinds} selectedOverlayIds={selectedCandidateIds} overlayInteractive={mapInteractionMode === "review"} onOverlayClick={openCandidatePopup} popup={candidatePopup} selectedPointIndex={featurePointIndex} shapeSelected={featureShapeSelected} onSelectShape={() => setFeatureShapeSelected(true)} onDeselect={() => setFeatureShapeSelected(false)} onActivate={() => setActiveMapEditor("feature")} onSelectPoint={(index) => { setFeaturePointIndex(index); setFeatureShapeSelected(true); }} onAddPoint={addFeaturePoint} onChangePoints={mapInteractionMode === "manual" ? changeFeaturePoints : mapInteractionMode === "orientation" ? (points) => setOrientationDraft((draft) => replaceDraftPoints(draft, points.map(gcj02ToWgs84))) : undefined} viewStorageKey={`feature-review:${campus.id}`} captureMode={visualCaptureMode} captureFitBounds={visualCaptureFitBounds} captureRequest={visualCaptureRequest} onCapture={({ imageDataUrl, request }) => { if (!request.southWest || !request.northEast) return; setVisualScreenshot(imageDataUrl); const west=request.southWest.lng,east=request.northEast.lng,south=request.southWest.lat,north=request.northEast.lat; setVisualCaptureBoundary([{lng:west,lat:south},{lng:east,lat:south},{lng:east,lat:north},{lng:west,lat:north},{lng:west,lat:south}].map(gcj02ToWgs84)); setVisualSelectionPoints([]); setVisualCaptureRequest(null); setVisualCaptureFitBounds(null); setVisualCaptureMode(false); setMapInteractionMode("review"); setVisualState("ready"); setVisualMessage("截图已准备好。采用后会识别当前审核类型；也可重新取景。"); }} t={t} /></div>
      <div className="feature-editor-toolbar">
        <div className="orientation-toolbar"><button className={orientationMode ? "review-button accepted" : "review-button"} onClick={beginCampusOrientation}>{t.drawCampusOrientation}</button><button className="primary-action compact-action" onClick={confirmCampusOrientation} disabled={!orientationMode || orientationDraft.points.length !== 2}>{t.confirmOrientation}</button><span>{t.rotation}: {orientationDegrees.toFixed(1)}°</span>{orientationMode ? <small>{orientationDraft.points.length === 0 ? t.drawCampusOrientationHelp : orientationDraft.points.length === 1 ? t.orientationStartSelected : t.orientationLineReady}</small> : null}</div>
        {!candidateReviewUnlocked ? <p className="naming-message">{t.orientationRequired}</p> : null}
        <div className="candidate-actions"><button className="secondary-action" onClick={undoFeature} disabled={!featureHistory.length}>{t.undoPoint}</button><button className="secondary-action" onClick={deleteFeaturePoint} disabled={!featureDraft.points.length}>{t.deleteVertex}</button><button className="primary-action" onClick={completeActiveFeatureStep}>{t.completeStep}</button></div>
      </div>
      <ReviewSummary t={t} acceptedCount={Object.values(reviews).filter((status) => status === "accepted").length} rejectedCount={Object.values(reviews).filter((status) => status === "rejected").length} featureCount={reviewedManifest.mapFeatures.length} slotCount={reviewedManifest.buildingSlots.length} />
      {candidateReviewUnlocked && activeReviewKind && activeReviewKind !== "building" ? <section className="visual-provider-panel primary-recovery-panel"><div className="candidate-heading"><p className="mini-label">校区视觉补缺 · 主流程</p><strong>{featureKindLabel(activeReviewKind, t)}</strong></div><p className="visual-provider-help">结构化候选审核后，用无标签高德底图补齐当前类型。先进入取景，手动调整缩放和平移；再截取当前视野并识别。</p><div className="candidate-actions visual-capture-actions"><button className={visualCaptureMode ? "review-button accepted" : "primary-action compact-action"} onClick={beginVisualSelection} disabled={visualState === "loading"}>{visualCaptureMode ? "重新适配完整校区" : "进入补缺取景"}</button><button className="primary-action compact-action" onClick={captureCurrentVisualView} disabled={!visualCaptureMode || visualState === "loading"}>{visualState === "loading" ? t.querying : "截取当前视野"}</button><button className="primary-action compact-action" onClick={runDeterministicVisualRecovery} disabled={visualState === "loading" || !visualScreenshot}>采用此图识别</button></div>{visualScreenshot ? <div className="visual-capture-preview"><img src={visualScreenshot} alt="视觉识别截图预览" /></div> : null}{visualMessage ? <p className={visualState === "error" ? "schematic-error" : "naming-message"}>{visualMessage}</p> : null}</section> : null}
      <div className="candidate-list-toolbar">
        <div className="confidence-filter" aria-label={t.confidence}>{(["high", "medium", "low", "confirmed"] as const).map((value) => <button className={confidenceFilter === value ? "review-button accepted" : "review-button"} onClick={() => { setConfidenceFilter(value); setCandidatePage(1); setSelectedCandidateIds([]); }} key={value}>{value === "confirmed" ? t.confirmed : confidenceLabel(value, t)} <strong>{candidates.filter((candidate) => visibleKinds.includes(candidate.kind) && (value === "confirmed" ? reviews[candidate.id] === "accepted" : (reviews[candidate.id] ?? "pending") === "pending" && (candidate.confidence === value || (value === "medium" && candidate.confidence === "manual")))).length}</strong></button>)}</div>
        <div className="candidate-actions">{activeReviewKind === "building" ? <button className="secondary-action compact-action" onClick={retryNameCurrentBuildingPage} disabled={!candidateReviewUnlocked || !pagedCandidates.some((candidate) => candidate.kind === "building")}>{t.retryNameCurrentPage}</button> : null}<button className="primary-action compact-action" onClick={() => applyBatchReview("accepted")} disabled={!candidateReviewUnlocked || !filteredCandidates.length}>{t.accept} {filteredCandidates.length}</button><button className="secondary-action compact-action" onClick={() => applyBatchReview("rejected")} disabled={!candidateReviewUnlocked || !filteredCandidates.length}>{t.reject} {filteredCandidates.length}</button><button className="secondary-action compact-action" onClick={undoBatchReview} disabled={!batchReviewHistory.length}>{t.undoPoint}</button></div>
        <Pagination current={candidatePage} total={filteredCandidates.length} pageSize={10} onChange={setCandidatePage} t={t} />
      </div>
      {foundationNamingMessage ? <p className="naming-message">{foundationNamingMessage}</p> : null}
      {!candidateReviewUnlocked ? <p className="naming-message">{t.orientationRequired}</p> : null}
      <CandidateList candidates={candidateReviewUnlocked ? pagedCandidates : []} reviews={reviews} onReview={updateReview} onViewIn3d={viewBuildingCandidateIn3d} t={t} />
      <details className="workflow-stage-disclosure"><summary>{t.advancedData}</summary><section className="visual-provider-panel"><p className="mini-label">训练模型 Provider（可选）</p><label><span>Endpoint</span><input value={visualEndpoint} onChange={(event) => setVisualEndpoint(event.target.value)} placeholder="http://127.0.0.1:8000/features" /></label><button className="secondary-action" onClick={runVisualFeatureProvider} disabled={!visualEndpoint.trim() || !visualScreenshot || visualState === "loading"}>使用当前截图运行模型识别</button>{queryResult?.providerDebug.some((entry) => entry.error) ? <button className="secondary-action" onClick={retryFeatureDiscovery}>{t.retryFailedLayers}</button> : null}</section></details>
    </section> : null}
    {campusBoundaryFeature ? <section className="foundation-preview-stage"><div className="candidate-heading"><p className="mini-label">04 · {t.foundationPreviewAndExport}</p><strong>{reviewedManifest.mapFeatures.length} {t.reviewedFeatures}</strong></div><button className="primary-action" onClick={generateFoundation3dPreview} disabled={!reviewedManifest.mapFeatures.length}>{t.generateFoundation3dPreview}</button>{foundationPreview ? <FoundationExportPreviewPanel preview={foundationPreview} t={t} /> : null}{foundationModel ? <FoundationMinecraftPreview model={foundationModel} t={t} /> : null}<button className="secondary-action" onClick={exportFoundationSchematic} disabled={!foundationModel}><FileJson2 aria-hidden="true" /> {t.exportFoundationSchem}</button></section> : null}
    {foundationExportSummary ? <p className="foundation-export-summary">{foundationExportSummary}</p> : null}
  </div>;
}

function LegacyFoundationModePanel({
  campus,
  onManifestChange,
  t
}: {
  campus: CampusTarget;
  onManifestChange: (manifest: typeof foundationManifestPlaceholder) => void;
  t: Translation;
}) {
  const [queryState, setQueryState] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [queryError, setQueryError] = useState<string | null>(null);
  const [queryResult, setQueryResult] = useState<OnlineMapQueryResult | null>(null);
  const [reviews, setReviews] = useState<Record<string, CandidateReviewStatus>>({});
  const [manualFeatures, setManualFeatures] = useState<MapFeature[]>([]);
  const [foundationStyle, setFoundationStyle] = useState<FoundationStyleSettings>(
    DEFAULT_FOUNDATION_STYLE
  );
  const [geometryDraft, setGeometryDraft] = useState<GeometryDraft>(
    createEmptyGeometryDraft(campusOnlineQueryTarget(campus))
  );
  const [selectedDraftPointIndex, setSelectedDraftPointIndex] = useState(0);
  const [foundationExportSummary, setFoundationExportSummary] = useState<string | null>(null);
  const [foundationModel, setFoundationModel] = useState<SchematicModel | null>(null);
  const [foundationPreview, setFoundationPreview] = useState<FoundationSchematicPreview | null>(null);
  const [boundaryCandidates, setBoundaryCandidates] = useState<MapCandidate[]>([]);
  const [boundaryState, setBoundaryState] = useState<"idle" | "loading" | "ready" | "manual" | "error">("idle");
  const [boundaryMessage, setBoundaryMessage] = useState<string | null>(null);
  const baseManifest = createEmptyFoundationManifest(campus.canonicalName);
  const sourceCandidates = queryResult?.candidates ?? [];
  const campusBoundaryFeature = manualFeatures.find((feature) => feature.kind === "campus") ?? null;
  const candidates = campusBoundaryFeature
    ? sourceCandidates.filter((candidate) => candidate.kind !== "campus" && candidateWithinBoundary(candidate, campusBoundaryFeature.geometry.points))
    : [];
  const reviewedCandidates = candidates
    .map((candidate): ReviewedCandidate | null => {
      const status = reviews[candidate.id];
      if (!status || status === "pending") return null;
      return status === "accepted" ? acceptCandidate(candidate) : rejectCandidate(candidate);
    })
    .filter((review): review is ReviewedCandidate => Boolean(review));

  const reviewedManifest = applyFoundationStyle(
    buildFoundationManifestFromReviews(
      baseManifest,
      reviewedCandidates,
      manualFeatures
    ),
    foundationStyle
  );
  useEffect(() => {
    setFoundationModel(null);
    setFoundationPreview(null);
  }, [queryResult, reviews, manualFeatures, foundationStyle]);


  function buildStyledManifest(
    nextReviewedCandidates = reviewedCandidates,
    nextManualFeatures = manualFeatures,
    nextStyle = foundationStyle
  ) {
    return applyFoundationStyle(
      buildFoundationManifestFromReviews(
        baseManifest,
        nextReviewedCandidates,
        nextManualFeatures
      ),
      nextStyle
    );
  }

  async function runPutuoQuery() {
    try {
      setQueryState("loading");
      setBoundaryState("loading");
      setQueryError(null);
      setBoundaryMessage(null);
      const [featureResult, boundaryResult] = await Promise.allSettled([
        defaultOnlineMapQueryService.queryCampus(campusOnlineQueryTarget(campus)),
        queryCampusBoundaryCandidates(campus)
      ]);
      if (featureResult.status === "rejected") throw featureResult.reason;
      const result = featureResult.value;
      setQueryResult(result);
      setReviews(Object.fromEntries(result.candidates.map((candidate) => [candidate.id, "pending"])));
      const boundaries = boundaryResult.status === "fulfilled" ? boundaryResult.value : [];
      setBoundaryCandidates(boundaries);
      const recommended = boundaries.find((candidate) => candidate.confidence === "high") ?? null;
      if (recommended) {
        setGeometryDraft(geometryDraftFromCandidate(recommended));
        setBoundaryState("ready");
        setBoundaryMessage(t.autoBoundaryFound);
      } else {
        setGeometryDraft({ ...createEmptyGeometryDraft(result.target), points: [] });
        setBoundaryState("manual");
        setBoundaryMessage(boundaryResult.status === "rejected" ? `${t.autoBoundaryUnavailable}: ${String(boundaryResult.reason)}` : t.manualBoundaryRequired);
      }
      setSelectedDraftPointIndex(0);
      setQueryState("ready");
      onManifestChange(baseManifest);
    } catch (reason) {
      setQueryState("error");
      setBoundaryState("error");
      setQueryError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  function updateReview(candidate: MapCandidate, status: CandidateReviewStatus) {
    const nextReviews = { ...reviews, [candidate.id]: status };
    setReviews(nextReviews);
    const nextReviewedCandidates = candidates
      .map((item): ReviewedCandidate | null => {
        const nextStatus = nextReviews[item.id];
        if (!nextStatus || nextStatus === "pending") return null;
        return nextStatus === "accepted" ? acceptCandidate(item) : rejectCandidate(item);
      })
      .filter((review): review is ReviewedCandidate => Boolean(review));
    onManifestChange(buildStyledManifest(nextReviewedCandidates, manualFeatures));
  }

  function addManualBoundary() {
    const fixedBoundary = makeManualPutuoBoundaryFeature();
    const nextManualFeatures = manualFeatures.some((feature) => feature.id === fixedBoundary.id)
      ? manualFeatures
      : [...manualFeatures, fixedBoundary];
    setManualFeatures(nextManualFeatures);
    onManifestChange(buildStyledManifest(reviewedCandidates, nextManualFeatures));
  }

  function loadCandidateGeometry(candidate: MapCandidate) {
    setGeometryDraft(geometryDraftFromCandidate(candidate));
    setSelectedDraftPointIndex(0);
  }

  function addDraftPoint(point: { lng: number; lat: number }) {
    const nextIndex = geometryDraft.points.length;
    setGeometryDraft((draft) => addPointToDraft(draft, point));
    setSelectedDraftPointIndex(nextIndex);
  }

  function moveSelectedDraftPoint(delta: { lng: number; lat: number }) {
    setGeometryDraft((draft) => moveDraftPoint(draft, selectedDraftPointIndex, delta));
  }

  function undoDraftPoint() {
    setGeometryDraft((draft) => removeLastDraftPoint(draft));
    setSelectedDraftPointIndex((index) => Math.max(0, index - 1));
  }

  function commitDraftAsManualFeature() {
    if (!draftCanClose(geometryDraft)) return;
    const boundary = {
      ...draftToManualFeature({ ...geometryDraft, kind: "campus", name: `${campus.canonicalName} boundary` }, manualFeatures.length + 1),
      id: `feature-campus-boundary-${campus.id}`
    };
    const nextManualFeatures = [...manualFeatures.filter((feature) => feature.kind !== "campus"), boundary];
    setManualFeatures(nextManualFeatures);
    setBoundaryState("ready");
    setBoundaryMessage(t.campusBoundaryConfirmed);
    onManifestChange(buildStyledManifest(reviewedCandidates, nextManualFeatures));
  }

  function addBoundaryPointFromGaode(point: { lng: number; lat: number }) {
    addDraftPoint(gcj02ToWgs84(point));
  }

  function clearBoundaryDraft() {
    setGeometryDraft((draft) => ({ ...draft, kind: "campus", points: [] }));
    setSelectedDraftPointIndex(0);
    setBoundaryState("manual");
  }

  function updateStyle(nextStyle: FoundationStyleSettings) {
    setFoundationStyle(nextStyle);
    onManifestChange(buildStyledManifest(reviewedCandidates, manualFeatures, nextStyle));
  }

  async function exportFoundationSchematic() {
    try {
      const exportResult = generateFoundationSchematicFromManifest(reviewedManifest, {
      roadWidthBlocks: foundationStyle.roadWidthBlocks
      });
      const bytes = gzipBytes(writeSpongeV2Schematic(exportResult.model));
      const manifestExport = exportFoundationManifestJson(reviewedManifest, `${exportResult.model.name}.foundation_manifest.json`);
      const saved = await saveExportBundleToChosenFolder([
        { fileName: `${exportResult.model.name}.schem`, bytes },
        { fileName: manifestExport.fileName, bytes: utf8Bytes(manifestExport.json) }
      ]);
      if (!saved) return;
      setFoundationExportSummary(`${t.foundationExported}: ${saved.directory} · ${exportResult.model.width} x ${exportResult.model.height} x ${exportResult.model.length}, ${t.exportedFoundationBytes} ${bytes.length}, ${t.exportedManifestFeatures} ${manifestExport.featureCount}, ${t.exportedManifestSlots} ${manifestExport.slotCount}`);
    } catch (error) {
      setFoundationExportSummary(error instanceof Error ? error.message : String(error));
    }
  }

  function generateFoundation3dPreview() {
    if (!reviewedManifest.mapFeatures.length) return;
    const generated = generateFoundationSchematicFromManifest(reviewedManifest, {
      roadWidthBlocks: foundationStyle.roadWidthBlocks
    });
    setFoundationModel(generated.model);
    setFoundationPreview(previewGeneratedFoundationSchematic(generated));
  }

  return (
    <div className="mode-panel">
      <div className="panel-icon">
        <Route aria-hidden="true" />
      </div>
      <p className="eyebrow">{t.mode01}</p>
      <h2>{t.foundationMode}</h2>
      <p>{t.foundationBody}</p>
      <div className="actions">
        <button className="primary-action" onClick={runPutuoQuery} disabled={queryState === "loading"}>
          <Search aria-hidden="true" />
          {queryState === "loading" ? t.querying : `${t.queryCampusBuildings} · ${campus.canonicalName}`}
        </button>

      </div>
      {queryError ? <div className="schematic-error">{queryError}</div> : null}
      {foundationExportSummary ? (
        <p className="foundation-export-summary">{foundationExportSummary}</p>
      ) : null}
      <section className="foundation-boundary-stage" aria-label={t.campusBoundaryEditor}>
        <div className="candidate-heading"><p className="mini-label">01 · {t.campusBoundaryEditor}</p><strong>{boundaryState === "ready" ? t.boundaryReady : t.boundaryNeedsReview}</strong></div>
        <GaodeCampusBoundaryEditor center={campus.center} points={geometryDraft.points.map(wgs84ToGcj02)} onAddPoint={addBoundaryPointFromGaode} t={t} />
        {boundaryMessage ? <p className="naming-message">{boundaryMessage}</p> : null}
        {boundaryCandidates.length ? <div className="boundary-candidate-list">{boundaryCandidates.slice(0, 5).map((candidate) => <button className="review-button" key={candidate.id} onClick={() => loadCandidateGeometry(candidate)}>{candidate.name} · {candidate.confidence}</button>)}</div> : null}
        <div className="candidate-actions">
          <button className="secondary-action" onClick={undoDraftPoint} disabled={!geometryDraft.points.length}>{t.undoPoint}</button>
          <button className="secondary-action" onClick={clearBoundaryDraft}>{t.clearBoundary}</button>
          <button className="primary-action" onClick={commitDraftAsManualFeature} disabled={!draftCanClose(geometryDraft)}>{t.confirmCampusBoundary}</button>
        </div>
      </section>
      {campusBoundaryFeature ? <>
        <FoundationStylePanel style={foundationStyle} t={t} onStyleChange={updateStyle} />
        <button className="primary-action" onClick={generateFoundation3dPreview} disabled={!reviewedManifest.mapFeatures.length}>{t.generateFoundation3dPreview}</button>
        {foundationPreview ? <FoundationExportPreviewPanel preview={foundationPreview} t={t} /> : null}
        {foundationModel ? <FoundationMinecraftPreview model={foundationModel} t={t} /> : null}
        <button className="secondary-action" onClick={exportFoundationSchematic} disabled={!foundationModel}>
          <FileJson2 aria-hidden="true" /> {t.exportFoundationSchem}
        </button>
      </> : null}
      <CandidateSourceStrip sourceOrder={queryResult?.sourceOrder ?? []} t={t} />
      <ProviderDebugPanel result={queryResult} t={t} />
      <ReviewSummary
        t={t}
        acceptedCount={Object.values(reviews).filter((status) => status === "accepted").length}
        rejectedCount={Object.values(reviews).filter((status) => status === "rejected").length}
        featureCount={reviewedManifest.mapFeatures.length}
        slotCount={reviewedManifest.buildingSlots.length}
      />
      <CandidateList
        candidates={candidates}
        reviews={reviews}
        onReview={updateReview}
        t={t}
      />
    </div>
  );
}

function CandidateSourceStrip({
  sourceOrder,
  t
}: {
  sourceOrder: CandidateSource[];
  t: Translation;
}) {
  const visibleOrder: CandidateSource[] = sourceOrder.length
    ? sourceOrder
    : ["arnis_open_geodata", "overture", "osm_overpass", "gaode_poi", "gaode_aoi"];

  return (
    <div className="source-strip" aria-label="Source priority">
      <p className="mini-label">{t.sourcePriority}</p>
      <div className="source-list">
        {visibleOrder.map((source, index) => (
          <span className={source === "gaode_aoi" ? "source-pill aoi" : "source-pill"} key={source}>
            {index + 1}. {sourceLabel(source, t)}
          </span>
        ))}
      </div>
    </div>
  );
}

function FoundationExportPreviewPanel({
  preview,
  t
}: {
  preview: FoundationSchematicPreview | null;
  t: Translation;
}) {
  if (!preview) {
    return (
      <section className="foundation-export-preview empty" aria-label={t.exportSizePreview}>
        <p className="mini-label">{t.exportSizePreview}</p>
        <strong>{t.exportPreviewNeedsReviewedGeometry}</strong>
      </section>
    );
  }

  return (
    <section
      className={`foundation-export-preview ${preview.risk}`}
      aria-label={t.exportSizePreview}
    >
      <div className="candidate-heading">
        <p className="mini-label">{t.exportSizePreview}</p>
        <strong>{exportRiskLabel(preview.risk, t)}</strong>
      </div>
      <div className="export-preview-grid">
        <span>{t.dimensions} {preview.width} x {preview.height} x {preview.length}</span>
        <span>{t.totalBlocks} {formatNumber(preview.totalBlocks)}</span>
        <span>{t.estimatedNonAirBlocks} {formatNumber(preview.estimatedNonAirBlocks)}</span>
        <span>{t.palette} {preview.paletteSize}</span>
        <span>{t.reviewedFeatures} {preview.reviewedFeatureCount}</span>
      </div>
    </section>
  );
}

function FoundationMinecraftPreview({ model, t }: { model: SchematicModel; t: Translation }) {
  const [selected, setSelected] = useState<BlockInspection | null>(null);
  const [cameraView, setCameraView] = useState<PreviewCameraView>("perspective");
  return <section className="foundation-minecraft-preview" aria-label={t.minecraftFoundationPreview}>
    <div className="candidate-heading"><p className="mini-label">03 · {t.minecraftFoundationPreview}</p><strong>{model.width} × {model.height} × {model.length}</strong></div>
    <div className="camera-view-buttons">{PREVIEW_CAMERA_VIEWS.map((view) => <button className={view === cameraView ? "review-button accepted" : "review-button"} key={view} onClick={() => setCameraView(view)}>{previewCameraViewLabel(view, t)}</button>)}</div>
    <SchematicPreviewer model={model} selectedBlock={selected} previewHint={t.foundationMinecraftPreviewHelp} onInspectBlock={setSelected} cameraView={cameraView} showFootprintOverlay={false} />
  </section>;
}
function ProviderDebugPanel({
  result,
  t
}: {
  result: OnlineMapQueryResult | null;
  t: Translation;
}) {
  if (!result) return null;

  return (
    <section className="provider-debug-panel" aria-label={t.providerDebug}>
      <div className="candidate-heading">
        <p className="mini-label">{t.providerDebug}</p>
        <strong>{result.providerDebug.length} {t.providersChecked}</strong>
      </div>
      <div className="provider-debug-grid">
        {result.providerDebug.map((entry) => (
          <article className="provider-debug-card" key={entry.source}>
            <div className="provider-debug-head">
              <strong>{sourceLabel(entry.source, t)}</strong>
              <span className={entry.cacheStatus === "hit" ? "cache-pill hit" : "cache-pill"}>
                {entry.cacheStatus === "hit" ? t.cacheHit : t.cacheMiss}
              </span>
            </div>
            <dl className="provider-debug-list">
              {entry.error ? <div className="provider-error"><dt>{t.providerFailure}</dt><dd>{entry.error}</dd></div> : null}
              <div>
                <dt>{t.providerRole}</dt>
                <dd>{providerRoleLabel(entry.role, t)}</dd>
              </div>
              <div>
                <dt>{t.candidateCount}</dt>
                <dd>{entry.count}</dd>
              </div>
              <div>
                <dt>{t.rawIds}</dt>
                <dd>{entry.rawIds.length ? entry.rawIds.join(", ") : t.none}</dd>
              </div>
              <div>
                <dt>{t.candidateIds}</dt>
                <dd>{entry.candidateIds.length ? entry.candidateIds.join(", ") : t.none}</dd>
              </div>
              <div>
                <dt>{t.provenanceNotes}</dt>
                <dd>{entry.notesPreview.length ? entry.notesPreview.join(" / ") : t.none}</dd>
              </div>
            </dl>
          </article>
        ))}
      </div>
    </section>
  );
}

function FoundationStylePanel({
  style,
  t,
  onStyleChange
}: {
  style: FoundationStyleSettings;
  t: Translation;
  onStyleChange: (style: FoundationStyleSettings) => void;
}) {
  return (
    <section className="foundation-style-panel" aria-label={t.featureStyleControls}>
      <div className="candidate-heading">
        <p className="mini-label">{t.featureStyleControls}</p>
        <strong>{t.roadWidth}: {style.roadWidthBlocks}</strong>
      </div>
      <label className="road-width-control">
        <span>{t.roadWidth}</span>
        <input
          type="range"
          min="1"
          max="16"
          value={style.roadWidthBlocks}
          onChange={(event) =>
            onStyleChange(updateRoadWidthStyle(style, Number(event.target.value)))
          }
        />
      </label>
      <div className="feature-style-grid">
        {FEATURE_KINDS.map((kind) => (
          <MinecraftBlockPicker
            key={kind}
            value={style.blocks[kind]}
            label={featureKindLabel(kind, t)}
            searchLabel={t.searchBlocks}
            onChange={(block) => onStyleChange(updateFeatureBlockStyle(style, kind, block))}
          />
        ))}
      </div>
    </section>
  );
}

function FoundationStylePresetPanel({ value, message, onChange, onMessage, t }: {
  value: FoundationStylePack;
  message: string | null;
  onChange: (pack: FoundationStylePack) => void;
  onMessage: (message: string | null) => void;
  t: Translation;
}) {
  return <section className="style-pack-panel">
    <div><p className="mini-label">{t.foundationStylePack}</p><strong>{value.name}</strong></div>
    <div className="candidate-grid">{FOUNDATION_STYLE_PRESETS.map((pack) => <button className={value.id === pack.id ? "review-button accepted" : "review-button"} onClick={() => { onChange(structuredClone(pack)); onMessage(`${t.loaded}: ${pack.name}`); }} key={pack.id}>{pack.name}</button>)}</div>
    <details><summary>{t.advancedData}</summary><label className="secondary-action compact-action">{t.importStylePack}<input type="file" accept="application/json,.json" hidden onChange={(event) => { const file = event.target.files?.[0]; if (!file) return; void file.text().then((json) => { const pack = parseFoundationStylePack(json); onChange(pack); onMessage(`${t.loaded}: ${pack.name}`); }).catch((reason) => onMessage(reason instanceof Error ? reason.message : String(reason))); }} /></label></details>
    {message ? <small>{message}</small> : null}
  </section>;
}

function GeometryEditorPanel({
  candidates,
  draft,
  selectedPointIndex,
  manualFeatureCount,
  t,
  onAddPoint,
  onSelectPoint,
  onMoveSelectedPoint,
  onUndoPoint,
  onCommitDraft,
  onLoadCandidate
}: {
  candidates: MapCandidate[];
  draft: GeometryDraft;
  selectedPointIndex: number;
  manualFeatureCount: number;
  t: Translation;
  onAddPoint: (point: { lng: number; lat: number }) => void;
  onSelectPoint: (index: number) => void;
  onMoveSelectedPoint: (delta: { lng: number; lat: number }) => void;
  onUndoPoint: () => void;
  onCommitDraft: () => void;
  onLoadCandidate: (candidate: MapCandidate) => void;
}) {
  const allPoints = [
    ...candidates.flatMap((candidate) => candidate.geometry.points),
    ...draft.points
  ];
  const projector = makeSvgProjector(allPoints);
  const draftPath = draft.points.map(projector.toSvgPoint);
  const selectedPoint = draft.points[selectedPointIndex];

  function addPointFromSvg(event: React.MouseEvent<SVGSVGElement>) {
    if ((event.target as SVGElement).tagName === "circle") return;
    const rect = event.currentTarget.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width) * projector.width;
    const y = ((event.clientY - rect.top) / rect.height) * projector.height;
    onAddPoint(projector.toLngLat({ x, y }));
  }

  return (
    <section className="geometry-editor" aria-label={t.geometryEditor}>
      <div className="candidate-heading">
        <p className="mini-label">{t.geometryEditor}</p>
        <strong>{draft.points.length} {t.points} / {manualFeatureCount} {t.manualFeatures}</strong>
      </div>
      <svg
        className="geometry-editor-canvas"
        viewBox={`0 0 ${projector.width} ${projector.height}`}
        role="img"
        aria-label={t.geometryEditorCanvas}
        onClick={addPointFromSvg}
      >
        <rect x="0" y="0" width={projector.width} height={projector.height} />
        {candidates.map((candidate) => {
          const points = candidate.geometry.points.map(projector.toSvgPoint);
          const path = svgPath(points, candidate.geometry.type === "polygon");
          return (
            <path
              className={`candidate-geometry ${candidate.kind}`}
              d={path}
              key={candidate.id}
            />
          );
        })}
        <path className="draft-geometry" d={svgPath(draftPath, draftCanClose(draft))} />
        {draftPath.map((point, index) => (
          <circle
            className={index === selectedPointIndex ? "draft-point selected" : "draft-point"}
            cx={point.x}
            cy={point.y}
            r={index === selectedPointIndex ? 6 : 4}
            key={`${point.x}-${point.y}-${index}`}
            onClick={(event) => {
              event.stopPropagation();
              onSelectPoint(index);
            }}
          />
        ))}
      </svg>
      <div className="geometry-editor-tools">
        <div>
          <p className="mini-label">{t.currentDraft}</p>
          <strong>{draft.name}</strong>
          <p>
            {selectedPoint
              ? `${t.selectedPoint}: ${selectedPoint.lng.toFixed(6)}, ${selectedPoint.lat.toFixed(6)}`
              : t.noPointSelected}
          </p>
        </div>
        <div className="geometry-nudge-grid">
          <button className="review-button" onClick={() => onMoveSelectedPoint({ lng: 0, lat: 0.00005 })}>
            {t.nudgeNorth}
          </button>
          <button className="review-button" onClick={() => onMoveSelectedPoint({ lng: -0.00005, lat: 0 })}>
            {t.nudgeWest}
          </button>
          <button className="review-button" onClick={() => onMoveSelectedPoint({ lng: 0.00005, lat: 0 })}>
            {t.nudgeEast}
          </button>
          <button className="review-button" onClick={() => onMoveSelectedPoint({ lng: 0, lat: -0.00005 })}>
            {t.nudgeSouth}
          </button>
        </div>
        <div className="geometry-editor-actions">
          <button className="secondary-action" onClick={onUndoPoint}>
            {t.undoPoint}
          </button>
          <button
            className="primary-action"
            onClick={onCommitDraft}
            disabled={!draftCanClose(draft)}
          >
            {t.closeAsManualFeature}
          </button>
        </div>
        <div className="geometry-load-list">
          <p className="mini-label">{t.loadCandidateGeometry}</p>
          {candidates.slice(0, 4).map((candidate) => (
            <button
              className="review-button"
              onClick={() => onLoadCandidate(candidate)}
              key={candidate.id}
            >
              {candidate.name}
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}

function ReviewSummary({
  t,
  acceptedCount,
  rejectedCount,
  featureCount,
  slotCount
}: {
  t: Translation;
  acceptedCount: number;
  rejectedCount: number;
  featureCount: number;
  slotCount: number;
}) {
  return (
    <div className="review-summary" aria-label="Review summary">
      <span>{t.accepted} {acceptedCount}</span>
      <span>{t.rejected} {rejectedCount}</span>
      <span>{t.reviewedFeatures} {featureCount}</span>
      <span>{t.buildingSlots} {slotCount}</span>
    </div>
  );
}

function CandidateList({
  candidates,
  reviews,
  onReview,
  onViewIn3d,
  t
}: {
  candidates: MapCandidate[];
  reviews: Record<string, CandidateReviewStatus>;
  onReview: (candidate: MapCandidate, status: CandidateReviewStatus) => void;
  onViewIn3d?: (candidate: MapCandidate) => void;
  t: Translation;
}) {
  if (candidates.length === 0) {
    return (
      <div className="candidate-empty">
        <Tag aria-hidden="true" />
        <span>{t.candidateEmpty}</span>
      </div>
    );
  }

  return (
    <section className="candidate-section" aria-label="Map Candidates">
      <div className="candidate-heading">
        <p className="mini-label">{t.mapCandidates}</p>
        <strong>{candidates.length} {t.editableSuggestions}</strong>
      </div>
      <div className="candidate-grid">
        {candidates.map((candidate) => (
          <article className={`candidate-card ${candidate.kind} ${reviews[candidate.id] ?? "pending"}`} key={candidate.id}>
            <div className="candidate-card-head">
              <strong>{candidate.name}</strong>
              <span className={`confidence ${candidate.confidence}`}>
                {confidenceLabel(candidate.confidence, t)}
              </span>
            </div>
            <p>
              {featureKindLabel(candidate.kind, t)} / {geometryTypeLabel(candidate.geometry.type, t)} / {candidate.geometry.points.length} {t.points}
            </p>
            {candidate.featureSubtype ? <small>{candidate.featureSubtype}</small> : null}
            {candidate.confidenceReasons?.length ? <details><summary>{t.confidence}</summary><ul>{candidate.confidenceReasons.map((reason) => <li key={reason}>{reason}</li>)}</ul></details> : null}
            <div className="candidate-meta">
              <span>{candidate.provenance.sourceLabel}</span>
              {isAoiCandidate(candidate) ? <span className="aoi-flag">{t.aoiCandidate}</span> : null}
            </div>
            <div className="candidate-actions">
              {onViewIn3d ? (
                <button className="secondary-action compact-action" onClick={() => onViewIn3d(candidate)}>
                  {t.jumpToGaode3d}
                </button>
              ) : null}
              <button
                className={reviews[candidate.id] === "accepted" ? "review-button accepted" : "review-button"}
                onClick={() => onReview(candidate, reviews[candidate.id] === "accepted" ? "pending" : "accepted")}
              >
                {reviews[candidate.id] === "accepted" ? t.revokeConfirmation : t.accept}
              </button>
              <button
                className={reviews[candidate.id] === "rejected" ? "review-button rejected" : "review-button"}
                onClick={() => onReview(candidate, "rejected")}
              >
                {t.reject}
              </button>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function CandidateMapPopup({ candidate, candidates, activeIndex, name, namingState, review, onNameChange, onSaveName, onSelect, onConfirm, onReject, onRevoke, onClose, t }: {
  candidate: MapCandidate;
  candidates: MapCandidate[];
  activeIndex: number;
  name: string;
  namingState?: "pending" | "matched" | "unmatched" | "failed";
  review: CandidateReviewStatus;
  onNameChange: (name: string) => void;
  onSaveName: () => void;
  onSelect: (index: number) => void;
  onConfirm: () => void;
  onReject: () => void;
  onRevoke: () => void;
  onClose: () => void;
  t: Translation;
}) {
  return <article aria-label={t.candidateDetails}>
    <div className="candidate-card-head"><strong>{t.candidateDetails}</strong><button className="review-button compact-action" onClick={onClose}>×</button></div>
    {candidates.length > 1 ? <div className="confidence-filter">{candidates.map((item, index) => <button className={index === activeIndex ? "review-button accepted" : "review-button"} onClick={() => onSelect(index)} key={item.id}>{index + 1} · {item.provenance.sourceLabel}</button>)}</div> : null}
    {candidate.kind === "building" ? <label><span>{t.buildingName}</span><input value={name} onChange={(event) => onNameChange(event.target.value)} placeholder={t.buildingName} /></label> : <h3>{candidate.name}</h3>}
    <p>{featureKindLabel(candidate.kind, t)} · {candidate.geometry.points.length} {t.points}</p>
    <p>{candidate.provenance.sourceLabel}</p>
    {candidate.kind === "building" ? <small>{t.namingStatus}: {namingState ?? "matched"}</small> : null}
    <div className="candidate-actions">
      {candidate.kind === "building" ? <button className="review-button compact-action" onClick={onSaveName} disabled={!name.trim()}>{t.saveBuildingName}</button> : null}
      {review === "accepted" ? <button className="review-button compact-action" onClick={onRevoke}>{t.revokeConfirmation}</button> : <button className="primary-action compact-action" onClick={onConfirm} disabled={candidate.kind === "building" && !name.trim()}>{t.confirmed}</button>}
      <button className="review-button rejected compact-action" onClick={onReject}>{t.reject}</button>
    </div>
  </article>;
}

function DetailedModeWorkspace({ campus, slots, refinements, onConfirmRefinement, initialBuildingDirectory, t }: {
  campus: CampusTarget;
  slots: BuildingSlot[];
  refinements: Record<string, BuildingSlotRefinement[]>;
  onConfirmRefinement: (slot: BuildingSlot, model: SchematicModel) => void;
  initialBuildingDirectory: CampusBuildingNameRecord[];
  t: Translation;
}) {
  const ordered = [...slots].sort((left, right) => {
    const leftDone = refinements[left.id]?.some((item) => item.status === "confirmed") ? 1 : 0;
    const rightDone = refinements[right.id]?.some((item) => item.status === "confirmed") ? 1 : 0;
    return leftDone - rightDone || confidenceOrder(right.confidence) - confidenceOrder(left.confidence);
  });
  const [selectedSlotId, setSelectedSlotId] = useState<string | null>(ordered[0]?.id ?? null);
  const slot = ordered.find((item) => item.id === selectedSlotId) ?? ordered[0] ?? null;
  return <div className="detailed-mode-workspace">
    <aside className="slot-work-queue"><div className="candidate-heading"><p className="mini-label">{t.buildingSlotWorkQueue}</p><strong>{slots.length}</strong></div>{ordered.length ? ordered.map((item) => { const history = refinements[item.id] ?? []; const status = history.some((entry) => entry.status === "confirmed") ? t.refined : t.notStarted; return <button className={item.id === slot?.id ? "slot-work-item active" : "slot-work-item"} key={item.id} onClick={() => setSelectedSlotId(item.id)}><strong>{item.name}</strong><span>{status} · {confidenceLabel(item.confidence, t)}</span>{history.length ? <small>v{history.at(-1)?.version}</small> : null}</button>; }) : <p className="candidate-empty">{t.noBuildingSlot}</p>}</aside>
    <div className="slot-work-detail">{slot ? <DetailedModePanel key={slot.id} campus={campus} slot={slot} onConfirmRefinement={onConfirmRefinement} initialBuildingDirectory={initialBuildingDirectory} t={t} /> : <section className="mode-panel"><h2>{t.detailedMode}</h2><p>{t.noBuildingSlot}</p></section>}</div>
  </div>;
}

function confidenceOrder(value: BuildingSlot["confidence"]) { return value === "high" ? 3 : value === "medium" || value === "manual" ? 2 : 1; }

function DetailedModePanel({ campus, slot, onConfirmRefinement, initialBuildingDirectory, t }: { campus: CampusTarget; slot: BuildingSlot | null; onConfirmRefinement: (slot: BuildingSlot, model: SchematicModel) => void; initialBuildingDirectory: CampusBuildingNameRecord[]; t: Translation }) {
  const [generationState, setGenerationState] = useState<"idle" | "loading" | "ready" | "error">(
    "idle"
  );
  const [schematicModel, setSchematicModel] = useState<SchematicModel | null>(null);
  const [arnisCandidates, setArnisCandidates] = useState<ArnisBuildingCandidate[]>([]);
  const [selectedArnisCandidate, setSelectedArnisCandidate] = useState<ArnisBuildingCandidate | null>(null);
  const [candidateWarnings, setCandidateWarnings] = useState<string[]>([]);
  const [buildingSearchQuery, setBuildingSearchQuery] = useState("图书馆");
  const [gaodeCandidates, setGaodeCandidates] = useState<MapCandidate[]>([]);
  const [gaodeSearchState, setGaodeSearchState] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [gaodeSearchError, setGaodeSearchError] = useState<string | null>(null);
  const [gaodeAnchor, setGaodeAnchor] = useState<GaodeLocationAnchor | null>(null);
  const [mapPickedPoint, setMapPickedPoint] = useState<{ lng: number; lat: number } | null>(null);
  const [openGeodataAnchor, setOpenGeodataAnchor] = useState<OpenGeodataQueryAnchor | null>(null);
  const [confirmedTarget, setConfirmedTarget] = useState<BuildingTarget | null>(null);
  const [generationError, setGenerationError] = useState<string | null>(null);
  const [selectedBlock, setSelectedBlock] = useState<BlockInspection | null>(null);
  const [sourceBlock, setSourceBlock] = useState<MinecraftBlockName>("minecraft:stone_bricks");
  const [replacementBlock, setReplacementBlock] =
    useState<MinecraftBlockName>("minecraft:mossy_stone_bricks");
  const [replacementResult, setReplacementResult] = useState<string | null>(null);
  const [buildingGeometry, setBuildingGeometry] = useState<BuildingGeometry | null>(null);
  const [manualCorrection, setManualCorrection] = useState<ManualCorrectionDraft>(
    EMPTY_MANUAL_CORRECTION
  );
  const [manualCorrectionResult, setManualCorrectionResult] = useState<string | null>(null);
  const [detailedExportSummary, setDetailedExportSummary] = useState<string | null>(null);
  const [gaodePage, setGaodePage] = useState(1);
  const [relatedPage, setRelatedPage] = useState(1);
  const [nearbyPage, setNearbyPage] = useState(1);
  const [referenceCenter, setReferenceCenter] = useState(campus.center);
  const [buildingDirectory, setBuildingDirectory] = useState<CampusBuildingNameRecord[]>(
    () => initialBuildingDirectory
  );
  const [suppressedBuildings, setSuppressedBuildings] = useState<CampusBuildingSuppression[]>(
    () => loadCampusBuildingSuppressions(campus)
  );
  const [namingCandidates, setNamingCandidates] = useState<MapCandidate[]>([]);
  const [namingState, setNamingState] = useState<"loading" | "ready" | "error">("loading");
  const [namingMessage, setNamingMessage] = useState<string | null>(null);
  const [namingPage, setNamingPage] = useState(1);
  const [namingReferenceCenter, setNamingReferenceCenter] = useState(campus.center);
  const [reverseGeocodeCalls, setReverseGeocodeCalls] = useState(0);
  const [candidateQueryStartedAt, setCandidateQueryStartedAt] = useState<number | null>(null);
  const [candidateQueryElapsed, setCandidateQueryElapsed] = useState(0);

  useEffect(() => {
    setBuildingDirectory((current) => mergeCampusBuildingDirectories(initialBuildingDirectory, current));
    setSuppressedBuildings(loadCampusBuildingSuppressions(campus));
    const legacyOffCampus = initialBuildingDirectory.filter(
      (record) =>
        record.nameSource === "gaode_reverse_geocode" &&
        !isCampusAffiliatedName(record.name, campus)
    );
    if (legacyOffCampus.length) {
      let suppressions = loadCampusBuildingSuppressions(campus);
      for (const record of legacyOffCampus) {
        suppressions = suppressCampusBuilding(campus, record.sourceId, {
          wgs84: record.wgs84,
          reason: "Migrated reverse-geocode result without selected school/campus identity."
        });
      }
      setSuppressedBuildings(suppressions);
      commitLocalBuildingDirectory(loadCampusBuildingDirectory(campus));
    }
  }, [initialBuildingDirectory]);

  useEffect(() => {
    if (candidateQueryStartedAt === null) return;
    const update = () => setCandidateQueryElapsed(Math.floor((Date.now() - candidateQueryStartedAt) / 1000));
    update();
    const timer = window.setInterval(update, 1000);
    return () => window.clearInterval(timer);
  }, [candidateQueryStartedAt]);

  useEffect(() => {
    if (!slot) return;
    let cancelled = false;
    void (async () => {
      try {
        setGenerationState("loading");
        setCandidateQueryStartedAt(Date.now());
        const target = buildingSlotToBuildingTarget(slot);
        setConfirmedTarget(target);
        setReferenceCenter(wgs84ToGcj02(target.approximateCenter));
        setOpenGeodataAnchor({ derivedFromPoiId: slot.provenance.rawId, coordinateSystem: "WGS-84", position: target.approximateCenter, transformation: "gcj02-to-wgs84-iterative-v1" });
        const result = await queryArnisBuildingCandidates(target);
        if (cancelled) return;
        setArnisCandidates(result.candidates);
        setCandidateWarnings(result.warnings);
        setSelectedArnisCandidate(null);
        setSchematicModel(null);
        setBuildingGeometry(null);
        setGenerationError(null);
        setGenerationState("ready");
      } catch (error) {
        if (cancelled) return;
        setGenerationState("error");
        setGenerationError(error instanceof Error ? error.message : String(error));
      } finally { if (!cancelled) setCandidateQueryStartedAt(null); }
    })();
    return () => { cancelled = true; };
  }, [slot?.id]);

  async function loadCampusNamingCandidates(force = false) {
    try {
      setNamingState("loading");
      setNamingMessage(null);
      if (force) clearCampusBuildingProviderCache();
      const candidates = await createCampusBuildingProvider().query(campusOnlineQueryTarget(campus));
      const buildings = mergeCampusCandidateCorpus(
        candidates.filter((candidate) =>
          candidate.kind === "building" &&
          candidate.geometry.type === "polygon" &&
          candidateInsideCampus(candidate, campus) &&
          !suppressionForNamingCandidate(candidate)
        ),
        arnisCandidates.map((candidate) => arnisCandidateToNamingCandidate(candidate, campus.canonicalName))
      );
      setNamingCandidates(buildings);
      setNamingPage(1);
      setNamingState("ready");
    } catch (error) {
      setNamingState("error");
      setNamingMessage(error instanceof Error ? error.message : String(error));
    }
  }

  function commitLocalBuildingDirectory(local: CampusBuildingNameRecord[]) {
    setBuildingDirectory((current) =>
      mergeCampusBuildingDirectories(
        current.filter((record) => record.nameSource === "shared_annotation"),
        local
      )
    );
    void persistLocalCampusAnnotationFile(campus, local).catch((error) =>
      setNamingMessage(error instanceof Error ? error.message : String(error))
    );
  }

  function saveNamingCandidate(candidate: MapCandidate, name: string, nameSource: "manual" | "gaode_reverse_geocode" = "manual") {
    const wgs84 = candidateCenter(candidate);
    const gcj02 = wgs84ToGcj02(wgs84);
    commitLocalBuildingDirectory(
      saveCampusBuildingName(campus, candidate.provenance.rawId, name, {
        nameSource,
        gcj02,
        wgs84
      })
    );
  }

  function excludeNamingCandidate(
    candidate: MapCandidate,
    classificationSource: "automatic_name_filter" | "manual"
  ) {
    const next = suppressCampusBuilding(campus, candidate.provenance.rawId, {
      wgs84: candidateCenter(candidate),
      reason: classificationSource === "manual"
        ? "User deleted this source object from the selected campus."
        : "Gaode returned no school- or campus-prefixed POI for this source object."
    });
    setSuppressedBuildings(next);
    commitLocalBuildingDirectory(loadCampusBuildingDirectory(campus));
  }

  async function exportCampusAnnotations() {
    try {
      const fileName = `${campus.canonicalName.replace(/[^\p{L}\p{N}]+/gu, "-")}-buildings.json`;
      const saved = await saveExportBundleToChosenFolder([{ fileName, bytes: utf8Bytes(campusAnnotationExportJson(campus, buildingDirectory)) }]);
      if (saved) setNamingMessage(`${t.annotationFileSaved}: ${saved.paths[0]}`);
    } catch (error) {
      setNamingMessage(error instanceof Error ? error.message : String(error));
    }
  }

  async function autoNameCurrentPage() {
    const unnamed = paginate(orderedNamingCandidates, namingPage, 10).filter(
      (candidate) => !namingRecordForCandidate(candidate)
    );
    if (!unnamed.length) {
      setNamingMessage(t.namingPageComplete);
      return;
    }
    setNamingState("loading");
    setNamingMessage(t.reverseGeocodingInProgress);
    const results = await mapWithConcurrency(unnamed.slice(0, 10), 4, async (candidate) => {
      try {
        const result = await reverseGeocodeBuildingCandidate(candidate, campus);
        return { candidate, result, error: null as string | null };
      } catch (error) {
        return {
          candidate,
          result: null,
          error: error instanceof Error ? error.message : String(error)
        };
      }
    });
    let local = loadCampusBuildingDirectory(campus);
    let suppressions = loadCampusBuildingSuppressions(campus);
    let uncachedCalls = 0;
    let matched = 0;
    let excluded = 0;
    let failed = 0;
    for (const outcome of results) {
      if (outcome.error) {
        uncachedCalls += 1;
        failed += 1;
        continue;
      }
      if (!outcome.result) continue;
      if (!outcome.result.cached) uncachedCalls += 1;
      const record = outcome.result.record;
      if (!record) continue;
      if (isCampusAffiliatedName(record.name, campus)) {
        local = saveCampusBuildingName(campus, outcome.candidate.provenance.rawId, record.name, {
          nameSource: "gaode_reverse_geocode",
          gcj02: record.gcj02,
          wgs84: record.wgs84
        });
        matched += 1;
      } else {
        suppressions = suppressCampusBuilding(campus, outcome.candidate.provenance.rawId, {
          wgs84: record.wgs84 ?? candidateCenter(outcome.candidate),
          reason: "Gaode returned no school- or campus-prefixed POI for this source object."
        });
        local = loadCampusBuildingDirectory(campus);
        excluded += 1;
      }
    }
    commitLocalBuildingDirectory(local);
    setReverseGeocodeCalls((value) => value + uncachedCalls);
    setNamingState("ready");
    setNamingMessage(
      `${t.reverseGeocodeFinished}: ${matched} · ${t.autoExcludedBuildings}: ${excluded} · ${t.failedLookups}: ${failed} · ${t.newApiCalls}: ${uncachedCalls}`
    );
  }

  function jumpToNamingCandidate(candidate: MapCandidate) {
    setNamingReferenceCenter(wgs84ToGcj02(candidateCenter(candidate)));
    window.setTimeout(() => document.querySelector(".campus-naming-workspace .gaode-3d-reference")?.scrollIntoView({ behavior: "smooth", block: "start" }), 0);
  }

  async function searchGaodeBuilding() {
    try {
      const input = buildingSearchQuery.trim();
      const query = campus.aliases.some((alias) => input.includes(alias)) || input.includes(campus.canonicalName)
        ? input
        : `${campus.canonicalName} ${input}`;
      if (!query) throw new Error(t.buildingSearchRequired);
      setGaodeSearchState("loading");
      setGaodeSearchError(null);
      setGaodeCandidates([]);
      setGaodeAnchor(null);
      setMapPickedPoint(null);
      setOpenGeodataAnchor(null);
      setConfirmedTarget(null);
      setArnisCandidates([]);
      setSchematicModel(null);
      setBuildingGeometry(null);
      const provider = createLiveGaodePoiProvider();
      const directoryMatches = buildingDirectory.filter((record) =>
        isIncludedCampusBuildingRecord(record) &&
        record.gcj02 &&
        record.name.toLowerCase().includes(input.toLowerCase())
      ).map((record, index) => pointCandidate({
        id: `candidate-campus-annotation-${index}`,
        name: record.name,
        kind: "building",
        source: "gaode_poi",
        confidence: "high",
        query,
        rawId: `campus-annotation:${record.sourceId}`,
        notes: [`Campus Building Directory · ${record.nameSource ?? "manual"} · ${record.sourceId}`],
        coordinateSystem: "GCJ-02",
        point: [record.gcj02!.lng, record.gcj02!.lat]
      }));
      let liveCandidates: MapCandidate[] = [];
      try {
        liveCandidates = filterBuildingCandidatesToCampus(await provider.query({
          ...campusOnlineQueryTarget(campus, query),
          radiusM: campus.radiusM
        }), campus);
      } catch (error) {
        if (!directoryMatches.length) throw error;
      }
      const candidates = [...directoryMatches, ...liveCandidates]
        .filter((candidate) => !slot || candidateMatchesBuildingSlot(candidate, slot))
        .filter((candidate, index, all) =>
        all.findIndex((item) => item.name === candidate.name && distanceBetweenCandidatePoints(item, candidate) < 5) === index
      );
      if (!candidates.length) throw new Error(t.noGaodeBuildingCandidates);
      setGaodeCandidates(candidates);
      setGaodePage(1);
      setGaodeSearchState("ready");
    } catch (error) {
      setGaodeSearchState("error");
      setGaodeSearchError(error instanceof Error ? error.message : t.schematicGenerationFailed);
    }
  }

  async function confirmGaodeCandidate(candidate: MapCandidate) {
    await confirmGaodeAnchor(gaodeCandidateToLocationAnchor(candidate));
  }

  async function confirmMapPickedLocation() {
    if (!mapPickedPoint) return;
    const query = buildingSearchQuery.trim();
    if (!query) {
      setGaodeSearchState("error");
      setGaodeSearchError(t.buildingSearchRequired);
      return;
    }
    await confirmGaodeAnchor(gaodeMapClickToLocationAnchor({
      name: query,
      query,
      point: mapPickedPoint
    }));
  }

  async function confirmGaodeAnchor(gaode: GaodeLocationAnchor) {
    try {
      setGenerationState("loading");
      setCandidateQueryStartedAt(Date.now());
      setCandidateQueryElapsed(0);
      const open = openGeodataAnchorFromGaode(gaode);
      const target = buildingTargetFromLocationAnchors(gaode, open, campus.canonicalName);
      setGaodeAnchor(gaode);
      setReferenceCenter(gaode.position);
      setOpenGeodataAnchor(open);
      setConfirmedTarget(target);
      const result = await queryArnisBuildingCandidates(target);
      setArnisCandidates(result.candidates);
      const importedNamingCandidates = result.candidates.map((candidate) =>
        arnisCandidateToNamingCandidate(candidate, campus.canonicalName)
      );
      setNamingCandidates((current) => mergeCampusCandidateCorpus(current, importedNamingCandidates));
      const unnamedImported = result.candidates.filter((candidate) => !directoryRecordForArnisCandidate(candidate)).length;
      if (unnamedImported > 0) {
        setNamingMessage(`${unnamedImported} ${t.arnisCandidatesNeedNaming}`);
        setNamingPage(1);
      }
      setRelatedPage(1);
      setNearbyPage(1);
      setCandidateWarnings(result.warnings);
      setSelectedArnisCandidate(null);
      setSchematicModel(null);
      setBuildingGeometry(null);
      setGenerationError(null);
      setGenerationState("ready");
    } catch (error) {
      setGenerationState("error");
      setBuildingGeometry(null);
      setSchematicModel(null);
      setArnisCandidates([]);
      setCandidateWarnings([]);
      setGenerationError(error instanceof Error ? error.message : String(error || t.schematicGenerationFailed));
    } finally {
      setCandidateQueryStartedAt(null);
    }
  }

  async function generateSelectedArnisCandidate(candidate: ArnisBuildingCandidate) {
    if (!confirmedTarget) return;
    try {
      setGenerationState("loading");
      const target: BuildingTarget = {
        ...confirmedTarget,
        reviewedSlot: {
          id: `reviewed-${candidate.id}`,
          footprint: candidate.components[0].exterior,
          approximateWidthMeters: candidate.widthM,
          approximateLengthMeters: candidate.lengthM
        }
      };
      setConfirmedTarget(target);
      const geometry = buildingGeometryFromArnisCandidate(target, candidate);
      let schematic = import.meta.env.VITE_USE_LEGACY_DETAILED_GENERATOR === "true"
        ? generateSchematicFromBuildingGeometry(geometry)
        : await generateSchematicWithArnisCore(geometry, candidate.id, candidate.source);
      for (const externalModel of externalModelCandidatesFromArnis(candidate)) {
        schematic = recordExternalModelReview(
          schematic,
          externalModel,
          "pending",
          "Discovered from selected Arnis candidate tags; awaiting external-model review."
        );
      }
      setSchematicModel(schematic);
      setBuildingGeometry(geometry);
      setSelectedArnisCandidate(candidate);
      setSelectedBlock(null);
      setReplacementResult(null);
      setManualCorrection(EMPTY_MANUAL_CORRECTION);
      setManualCorrectionResult(null);
      setDetailedExportSummary(null);
      setSourceBlock(
        schematic.palette.includes("minecraft:stone_bricks")
          ? "minecraft:stone_bricks"
          : schematic.palette[1] ?? "minecraft:smooth_stone"
      );
      setGenerationError(null);
      setGenerationState("ready");
    } catch (error) {
      setGenerationState("error");
      setBuildingGeometry(null);
      setSchematicModel(null);
      setGenerationError(error instanceof Error ? error.message : String(error || t.schematicGenerationFailed));
    }
  }

  function replaceBlocks() {
    if (!schematicModel) return;

    const result = replaceAllMatchingBlocks(schematicModel, sourceBlock, replacementBlock);
    setSchematicModel(result.model);
    setSelectedBlock(null);
    setReplacementResult(
      `${t.replacedPrefix} ${result.replacedCount} ${result.sourceBlock} ${t.replacedMiddle} ${result.replacementBlock}.`
    );
    setDetailedExportSummary(null);
  }

  async function applyManualCorrection() {
    if (!buildingGeometry || !slot || !selectedArnisCandidate) return;

    try {
      const correction = correctionFromDraft(manualCorrection, slot);
      const result = applyManualBuildingGeometryCorrection(buildingGeometry, correction);
      if (result.correctedFields.length === 0) {
        setManualCorrectionResult(t.noManualCorrections);
        return;
      }

      const schematic = import.meta.env.VITE_USE_LEGACY_DETAILED_GENERATOR === "true"
        ? generateSchematicFromBuildingGeometry(result.geometry)
        : await generateSchematicWithArnisCore(
            result.geometry,
            selectedArnisCandidate.id,
            selectedArnisCandidate.source
          );
      setBuildingGeometry(result.geometry);
      setSchematicModel(schematic);
      setSelectedBlock(null);
      setReplacementResult(null);
      setDetailedExportSummary(null);
      setManualCorrectionResult(
        `${t.manualCorrectionApplied}: ${result.correctedFields.join(", ")}`
      );
      setManualCorrection(EMPTY_MANUAL_CORRECTION);
    } catch (error) {
      setManualCorrectionResult(error instanceof Error ? error.message : t.schematicGenerationFailed);
    }
  }

  function reviewObservation(
    observationId: string,
    status: "accepted" | "rejected" | "supporting"
  ) {
    if (!buildingGeometry) return;
    setBuildingGeometry(applyObservationReviewDecision(buildingGeometry, observationId, status));
    setDetailedExportSummary(null);
  }

  async function exportSchematic() {
    if (!schematicModel) return;
    try {
      const exportResult = prepareDetailedSchematicExport(schematicModel);
      const saved = await saveExportBundleToChosenFolder([
        { fileName: exportResult.fileName, bytes: exportResult.bytes },
        { fileName: exportResult.provenanceFileName, bytes: utf8Bytes(exportResult.provenanceJson) }
      ]);
      if (!saved) return;
      setDetailedExportSummary(`${t.detailedExportReady}: ${saved.directory} · ${exportResult.fileName} + ${exportResult.provenanceFileName}, ${exportResult.width} x ${exportResult.height} x ${exportResult.length}, ${t.palette} ${exportResult.paletteSize}, ${t.nbtBytes} ${formatNumber(exportResult.bytes.length)}`);
    } catch (error) {
      setDetailedExportSummary(error instanceof Error ? error.message : String(error));
    }
  }

  function arnisCandidateCenter(candidate: ArnisBuildingCandidate) {
    const points = candidate.components.flatMap((component) => component.exterior);
    const count = Math.max(1, points.length);
    return points.reduce(
      (sum, point) => ({ lng: sum.lng + point.lng / count, lat: sum.lat + point.lat / count }),
      { lng: 0, lat: 0 }
    );
  }

  function namingRecordForCandidate(candidate: MapCandidate) {
    return findCampusBuildingRecordForGeometry(
      buildingDirectory,
      candidate.provenance.rawId,
      [candidate.geometry.points],
      candidateCenter(candidate)
    );
  }

  function suppressionForNamingCandidate(candidate: MapCandidate) {
    return findCampusBuildingSuppression(
      suppressedBuildings,
      candidate.provenance.rawId,
      [candidate.geometry.points],
      candidateCenter(candidate)
    );
  }

  function directoryRecordForArnisCandidate(candidate: ArnisBuildingCandidate) {
    return findCampusBuildingRecordForGeometry(
      buildingDirectory,
      candidate.id,
      candidate.components.map((component) => component.exterior),
      arnisCandidateCenter(candidate)
    );
  }

  function suppressionForArnisCandidate(candidate: ArnisBuildingCandidate) {
    return findCampusBuildingSuppression(
      suppressedBuildings,
      candidate.id,
      candidate.components.map((component) => component.exterior),
      arnisCandidateCenter(candidate)
    );
  }

  function isEffectivelyExcluded(record?: CampusBuildingNameRecord) {
    return Boolean(
      record?.status === "excluded" ||
      (
        record?.nameSource === "gaode_reverse_geocode" &&
        !isCampusAffiliatedName(record.name, campus)
      )
    );
  }

  function renameArnisCandidate(candidate: ArnisBuildingCandidate, name: string) {
    const wgs84 = arnisCandidateCenter(candidate);
    commitLocalBuildingDirectory(
      saveCampusBuildingName(campus, candidate.id, name, {
        nameSource: "manual",
        wgs84,
        gcj02: wgs84ToGcj02(wgs84)
      })
    );
  }

  function jumpToArnisCandidate(candidate: ArnisBuildingCandidate) {
    if (!gaodeAnchor || !openGeodataAnchor) return;
    setReferenceCenter(wgs84ToGcj02(arnisCandidateCenter(candidate)));
    window.setTimeout(() => document.querySelector(".evidence-review-disclosure .gaode-3d-reference")?.scrollIntoView({ behavior: "smooth", block: "start" }), 0);
  }

  function savedCandidateName(candidate: ArnisBuildingCandidate) {
    const record = directoryRecordForArnisCandidate(candidate);
    return record && !isEffectivelyExcluded(record) && isIncludedCampusBuildingRecord(record)
      ? record.name
      : undefined;
  }

  const replacementOptions = schematicModel ? replacementBlockOptions(schematicModel) : [];
  const visibleArnisCandidates = arnisCandidates.filter(
    (candidate) =>
      !suppressionForArnisCandidate(candidate) &&
      !isEffectivelyExcluded(directoryRecordForArnisCandidate(candidate))
  );
  const primaryArnisCandidate = visibleArnisCandidates.find(
    (candidate) => candidate.identityConfidence !== "low"
  ) ?? null;
  const relatedArnisCandidates = visibleArnisCandidates.filter(
    (candidate) => candidate !== primaryArnisCandidate && candidate.identityConfidence !== "low"
  );
  const nearbyArnisCandidates = visibleArnisCandidates.filter(
    (candidate) => candidate.identityConfidence === "low"
  );
  const includedBuildingDirectory = buildingDirectory.filter(
    (record) =>
      isIncludedCampusBuildingRecord(record) &&
      !isEffectivelyExcluded(record) &&
      !findCampusBuildingSuppression(
        suppressedBuildings,
        record.sourceId,
        [],
        record.wgs84
      )
  );
  const activeNamingCandidates = namingCandidates.filter(
    (candidate) =>
      !suppressionForNamingCandidate(candidate) &&
      !isEffectivelyExcluded(namingRecordForCandidate(candidate))
  );
  const orderedNamingCandidates = [...activeNamingCandidates].sort((left, right) =>
    Number(Boolean(namingRecordForCandidate(right)))
      - Number(Boolean(namingRecordForCandidate(left)))
  );
  const namingPageCandidates = paginate(orderedNamingCandidates, namingPage, 10);
  const gaodePageCandidates = paginate(gaodeCandidates, gaodePage, 10);
  const relatedPageCandidates = paginate(relatedArnisCandidates, relatedPage, 10);
  const nearbyPageCandidates = paginate(nearbyArnisCandidates, nearbyPage, 10);

  return (
    <div className="mode-panel">
      <div className="panel-icon">
        <WandSparkles aria-hidden="true" />
      </div>
      <p className="eyebrow">{t.mode02}</p>
      <h2>{t.detailedMode}</h2>
      <p>{t.detailedBodyPrefix}{slot?.name ?? t.noBuildingSlot}{t.detailedBodySuffix}</p>
      <DetailedSlotSummary slot={slot} t={t} />
      {false ? <details className="workflow-stage-disclosure" open={!gaodeAnchor}>
        <summary><span>01</span> {t.locationAndEvidence}{gaodeAnchor ? ` · ${gaodeAnchor!.name}` : ""}</summary>
      <section className="building-search-panel" aria-label={t.buildingSearch}>
        <p className="mini-label">{t.buildingSearch}</p>
        <label>
          <span>{t.buildingName}</span>
          <input
            type="text"
            list="campus-building-directory"
            value={buildingSearchQuery}
            onChange={(event) => setBuildingSearchQuery(event.target.value)}
            disabled={gaodeSearchState === "loading" || generationState === "loading"}
          />
          <datalist id="campus-building-directory">
            {includedBuildingDirectory.map((record) => <option value={record.name} key={record.sourceId} />)}
          </datalist>
        </label>
        <button
          className="primary-action"
          onClick={searchGaodeBuilding}
          disabled={gaodeSearchState === "loading" || generationState === "loading"}
        >
          <Search aria-hidden="true" />
          {gaodeSearchState === "loading" ? t.querying : t.searchGaodeBuilding}
        </button>
      </section>
      {gaodeSearchState === "error" ? (
        <div className="schematic-error">{gaodeSearchError}</div>
      ) : null}
      {gaodeCandidates.length ? (
        <section className="geometry-summary" aria-label={t.gaodeBuildingCandidates}>
          <p className="mini-label">{t.gaodeBuildingCandidates}</p>
          <h3>{t.confirmGaodeLocation}</h3>
          <div className="candidate-grid">
            {gaodePageCandidates.map((candidate) => {
              const point = candidate.geometry.points[0];
              const selected = gaodeAnchor?.poiId === candidate.provenance.rawId;
              return (
                <article className="candidate-card" key={candidate.id}>
                  <div className="candidate-title-row">
                    <strong>{candidate.name}</strong>
                    <span className={selected ? "confidence high" : "confidence"}>
                      {selected ? t.confirmed : "GCJ-02"}
                    </span>
                  </div>
                  <p>{point.lng.toFixed(6)}, {point.lat.toFixed(6)}</p>
                  <p>{candidate.provenance.notes.join(" · ")}</p>
                  <button
                    className="primary-action compact-action"
                    onClick={() => confirmGaodeCandidate(candidate)}
                    disabled={generationState === "loading"}
                  >{t.confirmAndQueryArnis}</button>
                </article>
              );
            })}
          </div>
          <Pagination current={gaodePage} total={gaodeCandidates.length} pageSize={10} onChange={setGaodePage} t={t} />
        </section>
      ) : null}
      <details className="map-location-fallback">
        <summary>{t.mapLocationFallback}</summary>
        <p>{t.mapLocationFallbackHelp}</p>
        <Gaode3DReference
          center={campus.center}
          t={t}
          mode="picker"
          onPickLocation={setMapPickedPoint}
        />
        {mapPickedPoint ? (
          <div className="map-picked-location">
            <strong>{t.selectedMapLocation}</strong>
            <span>{mapPickedPoint!.lng.toFixed(6)}, {mapPickedPoint!.lat.toFixed(6)} · GCJ-02</span>
            <button
              className="primary-action compact-action"
              onClick={confirmMapPickedLocation}
              disabled={generationState === "loading"}
            >{t.confirmMapLocation}</button>
          </div>
        ) : null}
      </details>
      {gaodeAnchor && openGeodataAnchor ? (
        <section className="coordinate-lineage" aria-label={t.coordinateLineage}>
          <strong>{t.coordinateLineage}</strong>
          <span>GCJ-02 {gaodeAnchor!.position.lng.toFixed(6)}, {gaodeAnchor!.position.lat.toFixed(6)}</span>
          <span>WGS-84 {openGeodataAnchor!.position.lng.toFixed(6)}, {openGeodataAnchor!.position.lat.toFixed(6)}</span>
          <small>{openGeodataAnchor!.transformation}</small>
        </section>
      ) : null}
      </details> : null}
      {candidateQueryStartedAt !== null ? <CandidateQueryProgress elapsed={candidateQueryElapsed} t={t} /> : null}
      {generationState === "error" ? (
        <div className="schematic-error">{generationError ?? t.schematicGenerationFailed}</div>
      ) : null}
      {confirmedTarget && openGeodataAnchor ? (
        <details className="workflow-stage-disclosure evidence-review-disclosure" open={!schematicModel}>
          <summary>{t.confirmedEvidence} · {confirmedTarget?.name}</summary>
        <section className="building-evidence-workspace" aria-label={t.buildingEvidenceWorkspace}>
          <Gaode3DReference center={referenceCenter} t={t} />
          <section className="geometry-summary" aria-label={t.liveArnisCandidates}>
            <p className="mini-label">{t.liveArnisCandidates}</p>
            <h3>{t.selectActualBuildingFootprint}</h3>
            <p><strong>{confirmedTarget?.name}</strong></p>
            <p>{t.compareFootprintHelp}</p>
            {candidateWarnings.length ? (
              <p className="tool-empty">{candidateWarnings.join(" · ")}</p>
            ) : null}
            {primaryArnisCandidate && openGeodataAnchor ? (
              <div className="primary-candidate-group">
                <p className="mini-label">{t.primaryMatchCandidate}</p>
                <ArnisCandidateCard
                  candidate={primaryArnisCandidate}
                  anchor={openGeodataAnchor}
                  t={t}
                  loading={generationState === "loading"}
                  onGenerate={generateSelectedArnisCandidate}
                  displayName={savedCandidateName(primaryArnisCandidate)}
                  onJump={jumpToArnisCandidate}
                />
              </div>
            ) : generationState !== "loading" ? <p className="tool-empty">{t.noStrongCandidate}</p> : null}
            {relatedArnisCandidates.length && openGeodataAnchor ? (
              <details className="candidate-disclosure">
                <summary>{t.showRelatedCandidates} ({relatedArnisCandidates.length})</summary>
                <div className="candidate-grid">
                  {relatedPageCandidates.map((candidate) => (
                    <ArnisCandidateCard key={candidate.id} candidate={candidate} anchor={openGeodataAnchor} t={t} loading={generationState === "loading"} onGenerate={generateSelectedArnisCandidate} displayName={savedCandidateName(candidate)} onJump={jumpToArnisCandidate} />
                  ))}
                </div>
                <Pagination current={relatedPage} total={relatedArnisCandidates.length} pageSize={10} onChange={setRelatedPage} t={t} />
              </details>
            ) : null}
            {nearbyArnisCandidates.length && openGeodataAnchor ? (
              <details className="candidate-disclosure nearby-candidates">
                <summary>{t.showAllNearbyCandidates} ({nearbyArnisCandidates.length})</summary>
                <div className="candidate-grid">
                  {nearbyPageCandidates.map((candidate) => (
                    <ArnisCandidateCard key={candidate.id} candidate={candidate} anchor={openGeodataAnchor} t={t} loading={generationState === "loading"} onGenerate={generateSelectedArnisCandidate} displayName={savedCandidateName(candidate)} onJump={jumpToArnisCandidate} />
                  ))}
                </div>
                <Pagination current={nearbyPage} total={nearbyArnisCandidates.length} pageSize={10} onChange={setNearbyPage} t={t} />
              </details>
            ) : null}
          </section>
        </section>
        </details>
      ) : null}
      {schematicModel ? (
        <>
          {buildingGeometry ? (
            <section className="generated-result-stage" aria-label={t.generatedResult}>
              <div className="stage-heading">
                <p className="eyebrow">02</p>
                <h2>{t.generatedResult}</h2>
              </div>
              <div className="evidence-interpretation-grid">
                <ObservedBuildingEvidencePanel geometry={buildingGeometry} t={t} />
                <GeneratedBuildingInterpretationPanel geometry={buildingGeometry} model={schematicModel} t={t} />
                <ExternalModelReviewPanel model={schematicModel} onModelChange={setSchematicModel} t={t} />
                <SourceConflictReviewPanel model={schematicModel} onModelChange={setSchematicModel} t={t} />
              </div>
              <details className="advanced-provenance-disclosure">
                <summary>{t.advancedDataAndProvenance}</summary>
                <BuildingGeometrySummary
                  geometry={buildingGeometry}
                  t={t}
                  onReviewObservation={reviewObservation}
                />
              </details>
            </section>
          ) : null}
          <SchematicWorkbench
            t={t}
            model={schematicModel}
            selectedBlock={selectedBlock}
            replacementOptions={replacementOptions}
            sourceBlock={sourceBlock}
            replacementBlock={replacementBlock}
            replacementResult={replacementResult}
            exportSummary={detailedExportSummary}
            onInspectBlock={setSelectedBlock}
            onSourceBlockChange={setSourceBlock}
            onReplacementBlockChange={setReplacementBlock}
            onReplaceBlocks={replaceBlocks}
            onExport={exportSchematic}
            onModelChange={setSchematicModel}
          />
          {slot ? <button className="primary-action" onClick={() => onConfirmRefinement(slot, schematicModel)}>{t.confirmSlotRefinement}</button> : null}
        </>
      ) : null}
    </div>
  );
}

function CandidateQueryProgress({ elapsed, t, naming = false }: { elapsed: number; t: Translation; naming?: boolean }) {
  const message = naming
    ? t.loadingCampusSourceObjects
    : elapsed < 3 ? t.preparingCandidateQuery
      : elapsed < 12 ? t.queryingOsmCandidates
        : elapsed < 35 ? t.queryingOvertureCandidates
          : t.candidateQueryTakingLonger;
  return <section className="candidate-query-progress" role="status" aria-live="polite">
    <span className="progress-spinner" aria-hidden="true" />
    <div><strong>{message}</strong><small>{naming ? t.namingLoadingHint : `${t.elapsedTime}: ${elapsed}s · ${t.candidateQueryWaitHint}`}</small></div>
  </section>;
}

function CampusNamingCard({ candidate, record, t, onJump, onSave, onExclude }: {
  candidate: MapCandidate;
  record?: CampusBuildingNameRecord;
  t: Translation;
  onJump: (candidate: MapCandidate) => void;
  onSave: (candidate: MapCandidate, name: string) => void;
  onExclude: (candidate: MapCandidate) => void;
}) {
  const [draft, setDraft] = useState(record?.name ?? "");
  useEffect(() => setDraft(record?.name ?? ""), [candidate.id, record?.name]);
  const projector = makeSvgProjector(candidate.geometry.points);
  return <article className="candidate-card campus-naming-card">
    <div className="candidate-title-row">
      <strong>{record?.name ?? t.unnamedBuilding}</strong>
      <span className={record ? "confidence high" : "confidence"}>{record ? t.named : t.unmatched}</span>
    </div>
    <svg className="arnis-candidate-footprint" viewBox={`0 0 ${projector.width} ${projector.height}`} aria-label={record?.name ?? candidate.provenance.rawId}>
      <path className="candidate-parent-footprint" d={svgPath(candidate.geometry.points.map(projector.toSvgPoint), true)} />
    </svg>
    <small>{canonicalBuildingSourceId(candidate.provenance.rawId)}{record?.nameSource ? ` · ${record.nameSource}` : ""}</small>
    <div className="candidate-actions">
      <button className="secondary-action compact-action" onClick={() => onJump(candidate)}>{t.jumpToGaode3d}</button>
      <button className="review-button reject" onClick={() => onExclude(candidate)}>{t.excludeCandidate}</button>
    </div>
    <div className="candidate-rename-row">
      <input value={draft} onChange={(event) => setDraft(event.target.value)} placeholder={t.renameBuildingPlaceholder} />
      <button className="review-button" onClick={() => onSave(candidate, draft)} disabled={!draft.trim()}>{t.saveBuildingName}</button>
    </div>
  </article>;
}

function ArnisCandidateCard({
  candidate,
  anchor,
  t,
  loading,
  onGenerate,
  displayName,
  onJump
}: {
  candidate: ArnisBuildingCandidate;
  anchor: OpenGeodataQueryAnchor;
  t: Translation;
  loading: boolean;
  onGenerate: (candidate: ArnisBuildingCandidate) => void;
  displayName?: string;
  onJump: (candidate: ArnisBuildingCandidate) => void;
}) {
  const descriptor = describeArnisCandidate(candidate, anchor, t);
  const externalModelSummary = summarizeExternalModelCandidates(candidate);
  const confidence = candidate.identityConfidence === "high"
    ? t.high
    : candidate.identityConfidence === "medium" ? t.medium : t.low;
  const source = candidate.source === "overture" ? t.overture : t.osm;
  const outlinePoints = candidate.components.reduce(
    (sum, component) => sum + component.exterior.length,
    0
  );
  return (
    <article className="candidate-card arnis-candidate-card">
      <div className="candidate-title-row">
        <strong>{displayName || descriptor.title}</strong>
        <span className={candidate.identityConfidence === "high" ? "confidence high" : "confidence"}>
          {confidence}
        </span>
      </div>
      <p className="candidate-location-label">{descriptor.type} · {descriptor.direction} · {candidate.distanceM.toFixed(1)}m {t.distanceFromTarget}</p>
      <ArnisCandidateFootprint candidate={candidate} />
      <p>{candidate.widthM.toFixed(1)} × {candidate.lengthM.toFixed(1)}m</p>
      <p>{candidate.components.length} {t.footprintComponentCount} · {candidate.parts.length} {t.buildingPartCount} · {outlinePoints} {t.outlinePointCount}</p>
      {externalModelSummary.total ? (
        <p className="external-model-summary">
          {t.externalModelCandidates}: {externalModelSummary.total} · {t.eligible}: {externalModelSummary.eligible} · {t.blocked}: {externalModelSummary.blocked}
        </p>
      ) : null}
      <small>{source} · {t.sourceObjectId}: {candidate.id}</small>
      <button className="secondary-action compact-action" onClick={() => onJump(candidate)}>{t.jumpToGaode3d}</button>
      <button
        className="primary-action compact-action"
        onClick={() => onGenerate(candidate)}
        disabled={loading}
      >{t.confirmReviewedSlotAndGenerate}</button>
    </article>
  );
}

function describeArnisCandidate(
  candidate: ArnisBuildingCandidate,
  anchor: OpenGeodataQueryAnchor,
  t: Translation
) {
  const points = candidate.components.flatMap((component) => component.exterior);
  const center = points.reduce(
    (sum, point) => ({ lng: sum.lng + point.lng, lat: sum.lat + point.lat }),
    { lng: 0, lat: 0 }
  );
  center.lng /= Math.max(points.length, 1);
  center.lat /= Math.max(points.length, 1);
  const angle = (Math.atan2(center.lng - anchor.position.lng, center.lat - anchor.position.lat) * 180 / Math.PI + 360) % 360;
  const directions = [t.directionN, t.directionNE, t.directionE, t.directionSE, t.directionS, t.directionSW, t.directionW, t.directionNW];
  const direction = directions[Math.round(angle / 45) % 8];
  const building = candidate.tags.building?.toLowerCase();
  const amenity = candidate.tags.amenity?.toLowerCase();
  const type = amenity === "library"
    ? t.libraryBuilding
    : building === "sports_hall" || building === "stadium"
      ? t.sportsBuilding
      : ["school", "university", "college", "education"].includes(building ?? "")
        ? t.educationBuilding
        : ["apartments", "residential", "dormitory", "house"].includes(building ?? "")
          ? t.residentialBuilding
          : t.unnamedBuilding;
  return {
    type,
    direction,
    title: candidate.name?.trim() || `${type} · ${direction} ${candidate.distanceM.toFixed(1)}m`
  };
}

function ArnisCandidateFootprint({ candidate }: { candidate: ArnisBuildingCandidate }) {
  const points = [
    ...candidate.components.flatMap((component) => [
      ...component.exterior,
      ...component.interiorRings.flat()
    ]),
    ...candidate.parts.flatMap((part) => part.component.exterior)
  ];
  const projector = makeSvgProjector(points);
  return (
    <svg
      className="arnis-candidate-footprint"
      viewBox={`0 0 ${projector.width} ${projector.height}`}
      role="img"
      aria-label={`${candidate.name ?? candidate.id} footprint and building parts`}
    >
      {candidate.components.map((component, index) => (
        <path
          key={`outer-${index}`}
          className="candidate-parent-footprint"
          d={svgPath(component.exterior.map(projector.toSvgPoint), true)}
        />
      ))}
      {candidate.parts.map((part) => (
        <path
          key={part.id}
          className="candidate-building-part"
          d={svgPath(part.component.exterior.map(projector.toSvgPoint), true)}
        />
      ))}
    </svg>
  );
}

function ManualBuildingGeometryPanel({
  geometry,
  draft,
  result,
  t,
  onDraftChange,
  onApply
}: {
  geometry: BuildingGeometry;
  draft: ManualCorrectionDraft;
  result: string | null;
  t: Translation;
  onDraftChange: (draft: ManualCorrectionDraft) => void;
  onApply: () => void;
}) {
  function update<K extends keyof ManualCorrectionDraft>(key: K, value: ManualCorrectionDraft[K]) {
    onDraftChange({ ...draft, [key]: value });
  }

  return (
    <section className="manual-geometry-panel" aria-label={t.manualGeometryCorrection}>
      <div className="candidate-heading">
        <p className="mini-label">{t.manualGeometryCorrection}</p>
        <strong>{t.overrideAutomaticData}</strong>
      </div>
      <p className="manual-correction-help">{t.manualCorrectionHelp}</p>
      <div className="manual-correction-grid">
        <label className="manual-correction-reason">
          {t.correctionReason}
          <textarea
            required
            value={draft.reason}
            placeholder={t.correctionReasonPlaceholder}
            onChange={(event) => update("reason", event.target.value)}
          />
        </label>
        <label className="manual-footprint-toggle">
          <input
            type="checkbox"
            checked={draft.useSlotFootprint}
            onChange={(event) => update("useSlotFootprint", event.target.checked)}
          />
          {t.useReviewedSlotFootprint}
        </label>
        <ManualCorrectionInput
          label={t.height}
          value={draft.heightM}
          placeholder={geometry.heightM?.toString() ?? t.missing}
          type="number"
          onChange={(value) => update("heightM", value)}
        />
        <ManualCorrectionInput
          label={t.floorCount}
          value={draft.floors}
          placeholder={geometry.floors?.toString() ?? t.missing}
          type="number"
          onChange={(value) => update("floors", value)}
        />
        <ManualCorrectionInput label={t.roofShape} value={draft.roofShape} placeholder={geometry.roof.shape ?? t.missing} onChange={(value) => update("roofShape", value)} />
        <ManualCorrectionInput label={t.roofMaterial} value={draft.roofMaterial} placeholder={geometry.roof.material ?? t.missing} onChange={(value) => update("roofMaterial", value)} />
        <ManualCorrectionInput label={t.roofOrientation} value={draft.roofOrientation} placeholder={geometry.roof.orientation ?? t.missing} onChange={(value) => update("roofOrientation", value)} />
        <ManualCorrectionInput label={t.facadeMaterial} value={draft.facadeMaterial} placeholder={geometry.facade.material ?? t.missing} onChange={(value) => update("facadeMaterial", value)} />
        <ManualCorrectionInput label={t.facadeColor} value={draft.facadeColor} placeholder={geometry.facade.color ?? t.missing} onChange={(value) => update("facadeColor", value)} />
      </div>
      <button
        className="secondary-action manual-correction-action"
        onClick={onApply}
        disabled={!draft.reason.trim()}
      >
        <PencilLine aria-hidden="true" />
        {t.applyManualCorrection}
      </button>
      {result ? <p className="manual-correction-result">{result}</p> : null}
    </section>
  );
}

function ManualCorrectionInput({
  label,
  value,
  placeholder,
  type = "text",
  onChange
}: {
  label: string;
  value: string;
  placeholder: string;
  type?: "text" | "number";
  onChange: (value: string) => void;
}) {
  return (
    <label>
      {label}
      <input
        type={type}
        min={type === "number" ? 1 : undefined}
        step={type === "number" ? 1 : undefined}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

function correctionFromDraft(
  draft: ManualCorrectionDraft,
  slot: BuildingSlot
): ManualBuildingGeometryCorrection {
  return {
    reason: draft.reason.trim(),
    footprint: draft.useSlotFootprint ? slot.geometry.points : undefined,
    heightM: optionalPositiveNumber(draft.heightM),
    floors: optionalPositiveInteger(draft.floors),
    roof: compactHints({
      shape: optionalText(draft.roofShape),
      material: optionalText(draft.roofMaterial),
      orientation: optionalText(draft.roofOrientation)
    }),
    facade: compactHints({
      material: optionalText(draft.facadeMaterial),
      color: optionalText(draft.facadeColor)
    })
  };
}

function optionalPositiveNumber(value: string) {
  if (!value.trim()) return undefined;
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

function optionalPositiveInteger(value: string) {
  const number = optionalPositiveNumber(value);
  return number !== undefined && Number.isInteger(number) ? number : undefined;
}

function optionalText(value: string) {
  return value.trim() || undefined;
}

function compactHints<T extends Record<string, string | undefined>>(hints: T) {
  return Object.values(hints).some((value) => value !== undefined) ? hints : undefined;
}

function DetailedSlotSummary({ slot, t }: { slot: BuildingSlot | null; t: Translation }) {
  if (!slot) {
    return (
      <section className="detailed-slot-summary empty" aria-label={t.selectedBuildingSlot}>
        <p className="mini-label">{t.selectedBuildingSlot}</p>
        <strong>{t.noBuildingSlot}</strong>
      </section>
    );
  }

  const target = buildingSlotToBuildingTarget(slot);

  return (
    <section className="detailed-slot-summary" aria-label={t.selectedBuildingSlot}>
      <div className="candidate-heading">
        <p className="mini-label">{t.selectedBuildingSlot}</p>
        <strong>{slot.name}</strong>
      </div>
      <div className="slot-summary-grid">
        <span>{t.representativeSlotId} {slot.id}</span>
        <span>{t.selectedBlocks} {slot.selectedBlock}</span>
        <span>
          {t.approximateDimensions} {slot.dimensions.approximateWidthMeters}m x{" "}
          {slot.dimensions.approximateLengthMeters}m
        </span>
        <span>{t.confidence} {confidenceLabel(slot.confidence, t)}</span>
        <span>{t.provenanceSource} {slot.provenance.sourceLabel}</span>
        <span>
          {t.adapterTargetCenter} {target.approximateCenter.lng.toFixed(6)},{" "}
          {target.approximateCenter.lat.toFixed(6)}
        </span>
      </div>
    </section>
  );
}

function SourceConflictReviewPanel({ model, onModelChange, t }: { model: SchematicModel; onModelChange: (model: SchematicModel) => void; t: Translation }) {
  const conflicts = sourceConflictsForReview(model);
  const [drafts, setDrafts] = useState<Record<string, { decision: Exclude<SourceConflictDecision, "unresolved">; reason: string }>>({});
  const [error, setError] = useState<string | null>(null);
  if (!conflicts.length) return null;

  function draftFor(conflict: SourceConflictRecord) {
    return drafts[conflict.id] ?? {
      decision: conflict.decision === "unresolved" ? "supporting_only" : conflict.decision,
      reason: conflict.decisionReason ?? ""
    };
  }

  function updateDraft(conflict: SourceConflictRecord, change: Partial<{ decision: Exclude<SourceConflictDecision, "unresolved">; reason: string }>) {
    setDrafts((current) => ({
      ...current,
      [conflict.id]: { ...draftFor(conflict), ...change }
    }));
  }

  function save(conflict: SourceConflictRecord) {
    try {
      const draft = draftFor(conflict);
      onModelChange(recordSourceConflictDecision(model, conflict, draft.decision, draft.reason));
      setError(null);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  return (
    <section className="source-conflict-review-panel" aria-label={t.sourceConflictReview}>
      <div className="section-title-row">
        <p className="mini-label">{t.sourceConflictReview}</p>
        <span className="confidence">{conflicts.length}</span>
      </div>
      <p className="tool-empty">{t.sourceConflictReviewHelp}</p>
      <div className="external-model-grid">
        {conflicts.map((conflict) => {
          const draft = draftFor(conflict);
          return (
            <article key={conflict.id} className="external-model-card source-conflict-card">
              <div className="candidate-title-row">
                <strong>{conflict.summary}</strong>
                <span className={conflict.severity === "blocking" ? "confidence" : "confidence high"}>{conflict.severity}</span>
              </div>
              <ul>
                {conflict.evidence.map((evidence) => <li key={evidence.label}>{evidence.label}: {evidence.value}</li>)}
              </ul>
              <label>
                {t.sourceConflictDecision}
                <select value={draft.decision} onChange={(event) => updateDraft(conflict, { decision: event.target.value as Exclude<SourceConflictDecision, "unresolved"> })}>
                  <option value="primary_selected">{t.primarySelected}</option>
                  <option value="supporting_only">{t.supporting}</option>
                  <option value="rejected">{t.rejected}</option>
                </select>
              </label>
              <label>
                {t.sourceConflictReason}
                <textarea value={draft.reason} onChange={(event) => updateDraft(conflict, { reason: event.target.value })} rows={2} />
              </label>
              <button className="secondary-action compact-action" onClick={() => save(conflict)}>{t.saveSourceConflictDecision}</button>
              <p className="comparison-saved">{t.currentDecision}: {sourceConflictDecisionLabel(conflict.decision, t)}</p>
            </article>
          );
        })}
      </div>
      {error ? <p className="schematic-error">{error}</p> : null}
    </section>
  );
}

function sourceConflictDecisionLabel(value: SourceConflictDecision, t: Translation) {
  return {
    unresolved: t.pending,
    primary_selected: t.primarySelected,
    supporting_only: t.supporting,
    rejected: t.rejected
  }[value];
}

function ExternalModelReviewPanel({ model, onModelChange, t }: { model: SchematicModel; onModelChange: (model: SchematicModel) => void; t: Translation }) {
  const externalModels = model.metadata.provenance?.externalModels ?? [];
  const [drafts, setDrafts] = useState<Record<string, { decision: ExternalModelReviewDecision; reason: string }>>({});
  const [error, setError] = useState<string | null>(null);
  if (!externalModels.length) return null;

  function draftFor(id: string, currentDecision: ExternalModelReviewDecision) {
    return drafts[id] ?? { decision: currentDecision, reason: "" };
  }

  function updateDraft(id: string, change: Partial<{ decision: ExternalModelReviewDecision; reason: string }>, currentDecision: ExternalModelReviewDecision) {
    setDrafts((current) => ({
      ...current,
      [id]: { ...draftFor(id, currentDecision), ...change }
    }));
  }

  function save(item: ExternalModelProvenance) {
    try {
      const draft = draftFor(item.candidate.id, item.decision);
      const next = recordExternalModelReview(model, item.candidate, draft.decision, draft.reason || item.decisionReason);
      onModelChange(next);
      setError(null);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  return (
    <section className="external-model-review-panel" aria-label={t.externalModelReview}>
      <div className="section-title-row">
        <p className="mini-label">{t.externalModelReview}</p>
        <span className="confidence">{externalModels.length}</span>
      </div>
      <p className="tool-empty">{t.externalModelReviewHelp}</p>
      <div className="external-model-grid">
        {externalModels.map((item) => {
          const licenseReview = classifyExternalModelLicense(item.candidate);
          const draft = draftFor(item.candidate.id, item.decision);
          return (
            <article key={item.candidate.id} className="external-model-card">
              <div className="candidate-title-row">
                <strong>{item.candidate.title}</strong>
                <span className={licenseReview.eligibility === "eligible" ? "confidence high" : "confidence"}>
                  {externalEligibilityLabel(licenseReview.eligibility, t)}
                </span>
              </div>
              <p>{item.candidate.source.toUpperCase()} · {item.candidate.author || t.unknown}</p>
              <small>{item.candidate.sourceUrl}</small>
              <ul>
                {licenseReview.reasons.map((reason) => <li key={reason}>{reason}</li>)}
                {licenseReview.obligations.map((obligation) => <li key={obligation}>{obligation}</li>)}
              </ul>
              <label>
                {t.externalModelDecision}
                <select
                  value={draft.decision}
                  onChange={(event) => updateDraft(item.candidate.id, { decision: event.target.value as ExternalModelReviewDecision }, item.decision)}
                >
                  <option value="pending">{t.pending}</option>
                  <option value="eligible_primary">{t.eligiblePrimary}</option>
                  <option value="supporting_evidence">{t.supporting}</option>
                  <option value="rejected">{t.rejected}</option>
                </select>
              </label>
              <label>
                {t.externalModelDecisionReason}
                <textarea value={draft.reason} onChange={(event) => updateDraft(item.candidate.id, { reason: event.target.value }, item.decision)} rows={2} />
              </label>
              <button className="secondary-action compact-action" onClick={() => save(item)}>{t.saveExternalModelReview}</button>
              <p className="comparison-saved">{t.currentDecision}: {externalDecisionLabel(item.decision, t)}</p>
            </article>
          );
        })}
      </div>
      {error ? <p className="schematic-error">{error}</p> : null}
    </section>
  );
}

function externalEligibilityLabel(value: "eligible" | "review_only" | "blocked", t: Translation) {
  return {
    eligible: t.eligible,
    review_only: t.reviewOnly,
    blocked: t.blocked
  }[value];
}

function externalDecisionLabel(value: ExternalModelReviewDecision, t: Translation) {
  return {
    pending: t.pending,
    eligible_primary: t.eligiblePrimary,
    supporting_evidence: t.supporting,
    rejected: t.rejected
  }[value];
}

function ObservedBuildingEvidencePanel({
  geometry,
  t
}: {
  geometry: BuildingGeometry;
  t: Translation;
}) {
  const parts = geometry.buildingParts ?? [];
  const heights = parts.flatMap((part) => part.heightM === null ? [] : [part.heightM]);
  const floors = parts.flatMap((part) => part.floors === null ? [] : [part.floors]);
  const roofs = Array.from(new Set(parts.flatMap((part) => part.roofShape ? [part.roofShape] : [])));
  const interiorRings = geometry.footprintComponents.reduce(
    (sum, component) => sum + component.interiorRings.length,
    0
  );
  return (
    <section className="result-evidence-card" aria-label={t.observedBuildingEvidence}>
      <p className="mini-label">{t.observedBuildingEvidence}</p>
      <h3>{geometry.buildingName}</h3>
      <dl className="result-fact-list">
        <div><dt>{t.sources}</dt><dd>{geometry.provenance.usedSources.map((source) => source === "osm_overpass" ? t.osm : t.overture).join(", ")}</dd></div>
        <div><dt>{t.footprintComponents}</dt><dd>{geometry.footprintComponents.length}</dd></div>
        <div><dt>{t.interiorRingCount}</dt><dd>{interiorRings}</dd></div>
        <div><dt>{t.observedParts}</dt><dd>{parts.length}</dd></div>
        <div><dt>{t.heightCoverage}</dt><dd>{heights.length} / {parts.length}</dd></div>
        <div><dt>{t.observedHeightRange}</dt><dd>{numericRange(heights, "m", t.sourceNotProvided)}</dd></div>
        <div><dt>{t.floorCoverage}</dt><dd>{floors.length} / {parts.length}</dd></div>
        <div><dt>{t.observedFloorRange}</dt><dd>{numericRange(floors, "", t.sourceNotProvided)}</dd></div>
        <div><dt>{t.observedRoofShapes}</dt><dd>{roofs.join(", ") || t.sourceNotProvided}</dd></div>
      </dl>
    </section>
  );
}

function GeneratedBuildingInterpretationPanel({
  geometry,
  model,
  t
}: {
  geometry: BuildingGeometry;
  model: SchematicModel;
  t: Translation;
}) {
  const parts = geometry.buildingParts ?? [];
  const nonAirBlocks = model.blockData.reduce((count, paletteIndex) => count + (paletteIndex === 0 ? 0 : 1), 0);
  return (
    <section className="result-evidence-card generated-interpretation-card" aria-label={t.generatedBuildingInterpretation}>
      <p className="mini-label">{t.generatedBuildingInterpretation}</p>
      <h3>{model.name}</h3>
      <dl className="result-fact-list">
        <div><dt>{t.generatedDimensions}</dt><dd>{model.width} × {model.height} × {model.length}</dd></div>
        <div><dt>{t.nonAirBlocks}</dt><dd>{formatNumber(nonAirBlocks)}</dd></div>
        <div><dt>{t.generatedRoofHandling}</dt><dd>{model.metadata.roofShape ?? t.none}</dd></div>
        <div><dt>{t.fallbackHeightParts}</dt><dd>{parts.filter((part) => part.heightM === null).length}</dd></div>
        <div><dt>{t.fallbackFloorParts}</dt><dd>{parts.filter((part) => part.floors === null).length}</dd></div>
        <div><dt>{t.fallbackRoofParts}</dt><dd>{parts.filter((part) => !part.roofShape).length}</dd></div>
        <div><dt>{t.generatedPalette}</dt><dd>{model.palette.filter((block) => block !== "minecraft:air").join(", ")}</dd></div>
      </dl>
      <p className="generation-rule-notice">{t.generationRuleNotice}</p>
    </section>
  );
}

function numericRange(values: number[], suffix: string, emptyLabel: string) {
  if (!values.length) return emptyLabel;
  const minimum = Math.min(...values);
  const maximum = Math.max(...values);
  return minimum === maximum ? `${minimum}${suffix}` : `${minimum}${suffix} – ${maximum}${suffix}`;
}

function SchematicSummary({
  model,
  t
}: {
  model: SchematicModel;
  t: Translation;
}) {
  const bytes = writeSpongeV2Schematic(model);
  const provenance = model.metadata.provenance;
  const report = model.metadata.generationReport;

  return (
    <section className="schematic-summary" aria-label="Generated schematic summary">
      <div className="candidate-heading">
        <p className="mini-label">{t.schematicGenerated}</p>
        <strong>{model.name}</strong>
      </div>
      <div className="schematic-stat-grid">
        <span>{t.dimensions} {model.width} x {model.height} x {model.length}</span>
        <span>{t.palette} {model.palette.length}</span>
        <span>{t.blocks} {model.blockData.length}</span>
        <span>{t.nbtBytes} {bytes.length}</span>
        <span>{t.footprint} {model.metadata.nonRectangularFootprint ? t.nonRectangular : t.rectangular}</span>
        <span>{t.roof} {model.metadata.roofShape ?? "none"}</span>
        <span>{t.sources} {provenance?.usedSources.join(", ") || t.none}</span>
        <span>{t.recordedEdits} {provenance?.blockReplacements.length ?? 0}</span>
        {report ? (
          <>
            <span>{t.orientation} {report.orientationDegrees.toFixed(1)}°</span>
            <span>{t.floorCount} {report.floorCount} / {t.floorSpacing} {report.floorSpacingBlocks}</span>
            <span>{t.roofSilhouette} {report.roof.shape} / {report.roof.heightBlocks} blocks</span>
            <span>{t.roofAssumption} {report.roof.assumption ?? t.none}</span>
            <span>{t.footprintIoU} {(report.fidelity.footprintIoU * 100).toFixed(1)}%</span>
            <span>{t.areaError} {report.fidelity.areaErrorPercent.toFixed(2)}%</span>
            <span>{t.dimensionError} {report.fidelity.widthErrorMeters.toFixed(2)}m / {report.fidelity.lengthErrorMeters.toFixed(2)}m</span>
            <span>{t.entranceEmphasis} {report.entrance.side} / {report.entrance.widthBlocks} blocks</span>
            <span>{t.facadeRhythm} {report.facadeRhythm}</span>
            <span>
              {t.semanticBlocks} W {report.semanticBlockCounts.walls} · G {report.semanticBlockCounts.windows} · R {report.semanticBlockCounts.roof} · E {report.semanticBlockCounts.entrance}
            </span>
          </>
        ) : null}
      </div>
    </section>
  );
}

function BuildingGeometrySummary({
  geometry,
  t,
  onReviewObservation
}: {
  geometry: BuildingGeometry;
  t: Translation;
  onReviewObservation: (
    observationId: string,
    status: "accepted" | "rejected" | "supporting"
  ) => void;
}) {
  const confidenceEntries: Array<[string, GeometryConfidence]> = [
    [t.footprint, geometry.confidence.footprint],
    [t.height, geometry.confidence.height],
    [t.floorCount, geometry.confidence.floors],
    [t.roof, geometry.confidence.roof],
    [t.facade, geometry.confidence.facade]
  ];
  const handoff = geometry.provenance.handoff;

  return (
    <section className="building-geometry-summary" aria-label={t.buildingGeometrySummary}>
      <div className="candidate-heading">
        <p className="mini-label">{t.buildingGeometrySummary}</p>
        <strong>{geometry.buildingName}</strong>
      </div>

      <div className="geometry-facts-grid">
        <span>{t.footprintPoints} {geometry.footprint.length}</span>
        <span>{t.footprintComponents} {geometry.footprintComponents.length}</span>
        <span>{t.orientation} {geometry.orientationDegrees.toFixed(1)}°</span>
        <span>{t.scale} {geometry.scale.widthMeters.toFixed(1)} × {geometry.scale.lengthMeters.toFixed(1)}m</span>
        <span>{t.height} {geometry.heightM !== null ? `${geometry.heightM}m` : t.missing}</span>
        <span>{t.floorCount} {geometry.floors ?? t.missing}</span>
        <span>{t.floorSpacing} {geometry.floorSpacingMeters !== null ? `${geometry.floorSpacingMeters.toFixed(2)}m` : t.missing}</span>
        <span>{t.roofShape} {geometry.roof.shape ?? t.missing}</span>
        <span>{t.roofMaterial} {geometry.roof.material ?? t.missing}</span>
        <span>{t.roofOrientation} {geometry.roof.orientation ?? t.missing}</span>
        <span>{t.facadeMaterial} {geometry.facade.material ?? t.missing}</span>
        <span>{t.facadeColor} {geometry.facade.color ?? t.missing}</span>
      </div>

      <BuildingObservationComparison
        geometry={geometry}
        t={t}
        onReviewObservation={onReviewObservation}
      />

      <section className="evidence-derivation" aria-label="Evidence derivation decisions">
        <div className="candidate-heading">
          <p className="mini-label">Field-level evidence derivation</p>
          <strong>{geometry.provenance.fieldDecisions.length} decisions</strong>
        </div>
        <div className="field-decision-grid">
          {geometry.provenance.fieldDecisions.map((decision) => (
            <article key={decision.field}>
              <strong>{decision.field}</strong>
              <span>{String(Array.isArray(decision.value) ? `${decision.value.length} points` : decision.value)}</span>
              <span>{decision.source} / {(decision.qualityScore * 100).toFixed(0)}% / {decision.confidence}</span>
              <span>{decision.explanation}</span>
            </article>
          ))}
        </div>
        {geometry.provenance.contradictions.length ? (
          <div className="contradiction-list" role="status">
            <p className="mini-label">Contradictory evidence</p>
            {geometry.provenance.contradictions.map((contradiction) => (
              <span key={contradiction.field}>{contradiction.message}</span>
            ))}
          </div>
        ) : null}
        <div className="arnis-rule-list">
          <p className="mini-label">Arnis-derived interpretation rules</p>
          {geometry.provenance.arnisRuleDecisions.length
            ? geometry.provenance.arnisRuleDecisions.map((decision) => (
                <article key={`${decision.ruleId}-${decision.field}`}>
                  <strong>{decision.ruleId}</strong>
                  <span>{decision.field}: {String(decision.output)}</span>
                  <span>{decision.explanation}</span>
                  <small>{decision.upstreamReference}</small>
                </article>
              ))
            : <span>{t.none}</span>}
        </div>
      </section>

      <section className="geometry-integrity-grid" aria-label={t.geometryValidation}>
        <article className={geometry.validation.valid ? "validation-card valid" : "validation-card invalid"}>
          <p className="mini-label">{t.geometryValidation}</p>
          <strong>{geometry.validation.valid ? t.validGeometry : t.invalidGeometry}</strong>
          <span>{geometry.validation.errors.join(" | ") || t.none}</span>
          {geometry.validation.warnings.length ? (
            <small>{t.validationWarnings}: {geometry.validation.warnings.join(" | ")}</small>
          ) : null}
        </article>
        <article>
          <p className="mini-label">{t.generationAssumptions}</p>
          {geometry.provenance.generationAssumptions.length
            ? geometry.provenance.generationAssumptions.map((assumption) => (
                <span key={`${assumption.field}-${assumption.ruleId}`}>
                  {assumption.field}: {String(assumption.value)} — {assumption.reason}
                </span>
              ))
            : <span>{t.none}</span>}
        </article>
        <article>
          <p className="mini-label">{t.correctionHistory}</p>
          {geometry.provenance.corrections.length
            ? geometry.provenance.corrections.map((correction) => (
                <span key={correction.id}>
                  {correction.id}: {correction.correctedFields.join(", ")} — {correction.reason}
                </span>
              ))
            : <span>{t.none}</span>}
        </article>
      </section>

      <div className="geometry-explanation-grid">
        <article>
          <p className="mini-label">{t.fieldConfidence}</p>
          <div className="confidence-list">
            {confidenceEntries.map(([label, confidence]) => (
              <span key={label}>{label}: {geometryConfidenceLabel(confidence, t)}</span>
            ))}
          </div>
        </article>
        <article>
          <p className="mini-label">{t.geometryProvenance}</p>
          <dl className="geometry-provenance-list">
            <div><dt>{t.sourcePriority}</dt><dd>{geometry.provenance.sourcePriority.join(" -> ")}</dd></div>
            <div><dt>{t.usedSources}</dt><dd>{geometry.provenance.usedSources.join(", ") || t.none}</dd></div>
            <div><dt>{t.missingFields}</dt><dd>{geometry.provenance.missingFields.join(", ") || t.none}</dd></div>
            <div><dt>{t.provenanceNotes}</dt><dd>{geometry.provenance.notes.join(" | ") || t.none}</dd></div>
            <div>
              <dt>{t.overture} records</dt>
              <dd>
                {geometry.provenance.sourceRecords.length
                  ? geometry.provenance.sourceRecords.map((record) => [
                      record.featureId,
                      record.releaseId ?? "provider-default",
                      `${record.components.length} component(s)`,
                      `${record.queryLimit} max`
                    ].join(" / ")).join(" | ")
                  : t.none}
              </dd>
            </div>
            <div>
              <dt>{t.buildingGeometryHandoff}</dt>
              <dd>{handoff ? `${handoff.foundationSlotId} / ${handoff.selectedBlock}` : t.none}</dd>
            </div>
          </dl>
        </article>
      </div>
    </section>
  );
}

function BuildingObservationComparison({
  geometry,
  t,
  onReviewObservation
}: {
  geometry: BuildingGeometry;
  t: Translation;
  onReviewObservation: (
    observationId: string,
    status: "accepted" | "rejected" | "supporting"
  ) => void;
}) {
  const observations = geometry.provenance.observations;
  if (!observations.length) return null;
  const resolution = geometry.provenance.identityResolution;
  const points = observations.flatMap((observation) =>
    observation.components.flatMap((component) => [component.exterior, ...component.interiorRings].flat())
  );
  const projector = makeSvgProjector(points);
  const overlaps = pairwiseObservationOverlaps(observations);

  return (
    <section className="observation-comparison" aria-label="Building source comparison">
      <div className="candidate-heading">
        <p className="mini-label">Building Geometry Observations</p>
        <strong>
          {resolution.ambiguous
            ? "Ambiguous match — review required"
            : `${observations.length} candidates`}
        </strong>
      </div>
      <svg
        className="observation-overlay"
        viewBox={`0 0 ${projector.width} ${projector.height}`}
        role="img"
        aria-label="Overlaid Overture, OSM, and reviewed Building Slot footprints"
      >
        {observations.flatMap((observation) =>
          observation.components.flatMap((component, componentIndex) => [
            <path
              key={`${observation.id}-${componentIndex}-outer`}
              className={`observation-footprint ${observation.source}`}
              d={svgPath(component.exterior.map(projector.toSvgPoint), true)}
            />,
            ...component.interiorRings.map((ring, ringIndex) => (
              <path
                key={`${observation.id}-${componentIndex}-inner-${ringIndex}`}
                className="observation-hole"
                d={svgPath(ring.map(projector.toSvgPoint), true)}
              />
            ))
          ])
        )}
      </svg>
      <div className="observation-grid">
        {observations.map((observation) => {
          const match = resolution.matches.find((item) => item.observationId === observation.id);
          const review = geometry.provenance.observationReviews[observation.id] ?? "pending";
          return (
          <article
            key={observation.id}
            className={resolution.selectedObservationId === observation.id ? "selected" : ""}
          >
            <strong>{observation.source}</strong>
            <span>{observation.name ?? observation.sourceFeatureId}</span>
            <span>
              Identity: {match ? `${(match.score * 100).toFixed(1)}% / ${match.confidence}` : "unscored"}
            </span>
            <span>{Math.round(observation.metrics.areaSquareMeters)} m²</span>
            <span>
              {observation.metrics.widthMeters.toFixed(1)} × {observation.metrics.lengthMeters.toFixed(1)} m
            </span>
            <span>{observation.metrics.orientationDegrees.toFixed(1)}° / {observation.metrics.pointCount} points</span>
            <span>{Object.keys(observation.tags).length} tags</span>
            {match?.reasons.map((reason) => (
              <span className="identity-reason" key={reason.criterion}>{reason.message}</span>
            ))}
            <div className="observation-review-actions" aria-label={`Review ${observation.id}`}>
              {(["accepted", "supporting", "rejected"] as const).map((status) => (
                <button
                  key={status}
                  className={review === status ? `review-button ${status}` : "review-button"}
                  onClick={() => onReviewObservation(observation.id, status)}
                  aria-pressed={review === status}
                >
                  {{ accepted: t.accepted, supporting: t.supporting, rejected: t.rejected }[status]}
                </button>
              ))}
            </div>
          </article>
        );})}
      </div>
      <div className="observation-overlaps">
        <p className="mini-label">Pairwise footprint overlap</p>
        {overlaps.length
          ? overlaps.map((overlap) => (
              <span key={`${overlap.leftObservationId}-${overlap.rightObservationId}`}>
                {overlap.leftObservationId} ↔ {overlap.rightObservationId}: {(overlap.score * 100).toFixed(1)}%
              </span>
            ))
          : <span>{t.none}</span>}
      </div>
    </section>
  );
}

function SchematicWorkbench({
  t,
  model,
  selectedBlock,
  replacementOptions,
  sourceBlock,
  replacementBlock,
  replacementResult,
  exportSummary,
  onInspectBlock,
  onSourceBlockChange,
  onReplacementBlockChange,
  onReplaceBlocks,
  onExport,
  onModelChange
}: {
  t: Translation;
  model: SchematicModel;
  selectedBlock: BlockInspection | null;
  replacementOptions: MinecraftBlockName[];
  sourceBlock: MinecraftBlockName;
  replacementBlock: MinecraftBlockName;
  replacementResult: string | null;
  exportSummary: string | null;
  onInspectBlock: (block: BlockInspection) => void;
  onSourceBlockChange: (block: MinecraftBlockName) => void;
  onReplacementBlockChange: (block: MinecraftBlockName) => void;
  onReplaceBlocks: () => void;
  onExport: () => void;
  onModelChange: (model: SchematicModel) => void;
}) {
  const matchingBlockCount = countMatchingBlocks(model, sourceBlock);
  const review = visualReviewFor(model);
  const [cameraView, setCameraView] = useState<PreviewCameraView>("perspective");
  const [showFootprintOverlay, setShowFootprintOverlay] = useState(true);
  const [comparisonEvidenceSource, setComparisonEvidenceSource] = useState<"arnis_reference_reconstruction" | "accepted_real_result">(
    review.resultComparison?.evidence.source ?? "arnis_reference_reconstruction"
  );
  const [comparisonEvidenceLabel, setComparisonEvidenceLabel] = useState(
    review.resultComparison?.evidence.label ?? t.arnisReferenceReconstruction
  );
  const [comparisonEvidenceDescription, setComparisonEvidenceDescription] = useState(
    review.resultComparison?.evidence.description ?? t.comparisonEvidenceDefaultDescription
  );
  const [comparisonOutcome, setComparisonOutcome] = useState<VisualComparisonOutcome>(
    review.resultComparison?.outcome ?? "pending"
  );
  const [comparisonSummary, setComparisonSummary] = useState(review.resultComparison?.summary ?? "");
  const [comparisonCorrectionNotes, setComparisonCorrectionNotes] = useState(
    review.resultComparison?.correctionNotes.join("\n") ?? ""
  );
  const placementCheck = checkAxiomPlacement(model);
  const axiomAcceptance = model.metadata.provenance?.axiomAcceptance ?? null;
  const [minecraftVersion, setMinecraftVersion] = useState(axiomAcceptance?.minecraftVersion ?? "");
  const [axiomVersion, setAxiomVersion] = useState(axiomAcceptance?.axiomVersion ?? "");
  const [axiomImportResult, setAxiomImportResult] = useState<AxiomImportResult>(axiomAcceptance?.importResult ?? "not_tested");
  const [orientationCheck, setOrientationCheck] = useState<AxiomCheckDecision>(axiomAcceptance?.checks.orientation ?? "pending");
  const [scaleCheck, setScaleCheck] = useState<AxiomCheckDecision>(axiomAcceptance?.checks.scale ?? "pending");
  const [paletteCheck, setPaletteCheck] = useState<AxiomCheckDecision>(axiomAcceptance?.checks.palette ?? "pending");
  const [blockPlacementCheck, setBlockPlacementCheck] = useState<AxiomCheckDecision>(axiomAcceptance?.checks.blockPlacement ?? "pending");
  const [axiomScreenshotRefs, setAxiomScreenshotRefs] = useState(
    axiomAcceptance?.screenshots.map((screenshot) => `${screenshot.uri} | ${screenshot.note}`).join("\n") ?? ""
  );
  const [axiomCorrectionNotes, setAxiomCorrectionNotes] = useState(axiomAcceptance?.correctionNotes.join("\n") ?? "");
  const [checkpointError, setCheckpointError] = useState<string | null>(null);

  function capturePreview(dataUrl: string) {
    downloadDataUrl(`${model.name}-${cameraView}.png`, dataUrl);
    onModelChange(recordCapturedView(model, cameraView));
  }

  function saveResultComparison() {
    try {
      const next = recordResultComparison(model, {
        evidence: {
          source: comparisonEvidenceSource,
          label: comparisonEvidenceLabel,
          description: comparisonEvidenceDescription,
          capturedViews: review.capturedViews
        },
        outcome: comparisonOutcome,
        summary: comparisonSummary,
        correctionNotes: comparisonCorrectionNotes.split(/\r?\n/)
      });
      onModelChange(next);
      setCheckpointError(null);
    } catch (error) {
      setCheckpointError(error instanceof Error ? error.message : String(error));
    }
  }

  function saveAxiomAcceptance() {
    try {
      const screenshots = axiomScreenshotRefs
        .split(/\r?\n/)
        .map((line) => {
          const [uri = "", ...noteParts] = line.split("|");
          return { view: "axiom" as const, uri: uri.trim(), note: noteParts.join("|").trim() };
        })
        .filter((screenshot) => screenshot.uri);
      const next = recordAxiomAcceptance(model, {
        minecraftVersion,
        axiomVersion,
        importResult: axiomImportResult,
        orientationCheck,
        scaleCheck,
        paletteCheck,
        blockPlacementCheck,
        screenshots,
        correctionNotes: axiomCorrectionNotes.split(/\r?\n/)
      });
      onModelChange(next);
      setCheckpointError(null);
    } catch (error) {
      setCheckpointError(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <section className="previewer-workbench" aria-label={t.previewerAria}>
      <div className="stage-heading review-stage-heading">
        <p className="eyebrow">03</p>
        <h2>{t.reviewAndExport}</h2>
      </div>
      <div className="visual-preview-column">
        <div className="visual-review-toolbar" aria-label="Fixed preview controls">
          <div className="camera-view-buttons">
            {PREVIEW_CAMERA_VIEWS.map((view) => (
              <button
                key={view}
                className={cameraView === view ? "review-button accepted" : "review-button"}
                onClick={() => setCameraView(view)}
              >
                {previewCameraViewLabel(view, t)}
                {review.capturedViews.includes(view) ? " ✓" : ""}
              </button>
            ))}
          </div>
          <div className="camera-view-buttons">
            <label className="overlay-toggle">
              <input
                type="checkbox"
                checked={showFootprintOverlay}
                onChange={(event) => setShowFootprintOverlay(event.target.checked)}
              /> {t.geographicFootprint}
            </label>
          </div>
        </div>
        <SchematicPreviewer
          model={model}
          selectedBlock={selectedBlock}
          previewHint={t.previewHint}
          onInspectBlock={onInspectBlock}
          cameraView={cameraView}
          showFootprintOverlay={showFootprintOverlay}
          onCapture={capturePreview}
          captureLabel={t.capturePng}
        />
      </div>
      <aside className="block-tools" aria-label={t.blockTools}>
        <div aria-live="polite" aria-atomic="true">
          <p className="mini-label">{t.inspectedBlock}</p>
          {selectedBlock ? (
            <dl className="block-inspection">
              <div>
                <dt>{t.type}</dt>
                <dd>{selectedBlock.block}</dd>
              </div>
              <div>
                <dt>{t.position}</dt>
                <dd>{selectedBlock.x}, {selectedBlock.y}, {selectedBlock.z}</dd>
              </div>
              <div>
                <dt>{t.paletteIndex}</dt>
                <dd>{selectedBlock.paletteIndex}</dd>
              </div>
            </dl>
          ) : (
            <p className="tool-empty">{t.clickVisibleBlock}</p>
          )}
        </div>
        <div className="replacement-panel">
          <p className="mini-label">{t.batchBlockReplacement}</p>
          <p className="replacement-match-count">
            {t.matchingBlocks}: <strong>{formatNumber(matchingBlockCount)}</strong>
          </p>
          <MinecraftBlockPicker
            value={sourceBlock}
            label={t.sourceBlock}
            searchLabel={t.searchBlocks}
            allowedBlocks={model.palette.filter((block) => block !== "minecraft:air")}
            onChange={onSourceBlockChange}
          />
          <MinecraftBlockPicker
            value={replacementBlock}
            label={t.replacementBlock}
            searchLabel={t.searchBlocks}
            onChange={onReplacementBlockChange}
          />
          <button
            className="primary-action compact-action"
            onClick={onReplaceBlocks}
            disabled={matchingBlockCount === 0 || sourceBlock === replacementBlock}
          >
            <Replace aria-hidden="true" />
            {t.replaceMatchingBlocks}
          </button>
          {replacementResult ? (
            <p className="replacement-result" role="status">{replacementResult}</p>
          ) : null}
        </div>
        <button className="secondary-action export-action" onClick={onExport}>
          <FileJson2 aria-hidden="true" />
          {t.exportUpdatedSchem}
        </button>
        {exportSummary ? (
          <p className="detailed-export-summary" role="status">{exportSummary}</p>
        ) : null}
        <section className="result-comparison-panel" aria-label={t.referenceComparison}>
          <p className="mini-label">{t.referenceComparison}</p>
          <p className="tool-empty">{t.referenceComparisonHelp}</p>
          <label>
            {t.comparisonEvidence}
            <select
              value={comparisonEvidenceSource}
              onChange={(event) => {
                const source = event.target.value as "arnis_reference_reconstruction" | "accepted_real_result";
                setComparisonEvidenceSource(source);
                setComparisonEvidenceLabel(source === "arnis_reference_reconstruction" ? t.arnisReferenceReconstruction : t.acceptedRealResultEvidence);
              }}
            >
              <option value="arnis_reference_reconstruction">{t.arnisReferenceReconstruction}</option>
              <option value="accepted_real_result">{t.acceptedRealResultEvidence}</option>
            </select>
          </label>
          <label>
            {t.comparisonLabel}
            <input value={comparisonEvidenceLabel} onChange={(event) => setComparisonEvidenceLabel(event.target.value)} />
          </label>
          <label>
            {t.comparisonEvidenceDescription}
            <textarea value={comparisonEvidenceDescription} onChange={(event) => setComparisonEvidenceDescription(event.target.value)} rows={3} />
          </label>
          <label>
            {t.comparisonOutcome}
            <select value={comparisonOutcome} onChange={(event) => setComparisonOutcome(event.target.value as VisualComparisonOutcome)}>
              <option value="pending">{t.comparisonPending}</option>
              <option value="matches">{t.comparisonMatches}</option>
              <option value="differs">{t.comparisonDiffers}</option>
              <option value="inconclusive">{t.comparisonInconclusive}</option>
            </select>
          </label>
          <label>
            {t.comparisonSummary}
            <textarea value={comparisonSummary} onChange={(event) => setComparisonSummary(event.target.value)} rows={3} />
          </label>
          <label>
            {t.comparisonCorrectionNotes}
            <textarea value={comparisonCorrectionNotes} onChange={(event) => setComparisonCorrectionNotes(event.target.value)} rows={3} />
          </label>
          <button className="secondary-action compact-action" onClick={saveResultComparison}>{t.recordComparison}</button>
          {review.resultComparison ? (
            <p className="comparison-saved" role="status">
              {t.comparisonSaved}: {review.resultComparison.evidence.label} · {review.resultComparison.outcome} · {new Date(review.resultComparison.comparedAt).toLocaleString()}
            </p>
          ) : null}
          {checkpointError ? <p className="schematic-error">{checkpointError}</p> : null}
        </section>
        <section className="axiom-acceptance-panel" aria-label={t.axiomAcceptance}>
          <p className="mini-label">{t.axiomAcceptance}</p>
          <p className="tool-empty">{t.axiomAcceptanceHelp}</p>
          <div className="placement-check-card">
            <strong>{t.buildingSlotDimensionCheck}: {placementStatusLabel(placementCheck.status, t)}</strong>
            <span>{t.origin}: {placementCheck.origin.x}, {placementCheck.origin.y}, {placementCheck.origin.z}</span>
            <span>{t.orientation}: {placementCheck.orientationDegrees ?? t.unknown}</span>
            <span>{t.scale}: {placementCheck.blocksPerMeter ?? t.unknown}</span>
            <span>{t.dimensionDelta}: {placementCheck.widthDeltaBlocks ?? t.unknown} / {placementCheck.lengthDeltaBlocks ?? t.unknown}</span>
          </div>
          <label>
            {t.minecraftVersion}
            <input value={minecraftVersion} onChange={(event) => setMinecraftVersion(event.target.value)} />
          </label>
          <label>
            {t.axiomVersion}
            <input value={axiomVersion} onChange={(event) => setAxiomVersion(event.target.value)} />
          </label>
          <label>
            {t.axiomImportResult}
            <select value={axiomImportResult} onChange={(event) => setAxiomImportResult(event.target.value as AxiomImportResult)}>
              <option value="not_tested">{t.notTested}</option>
              <option value="succeeded">{t.importSucceeded}</option>
              <option value="failed">{t.importFailed}</option>
            </select>
          </label>
          <div className="axiom-check-grid">
            <label>{t.orientation}<select value={orientationCheck} onChange={(event) => setOrientationCheck(event.target.value as AxiomCheckDecision)}>{axiomCheckOptions(t)}</select></label>
            <label>{t.scale}<select value={scaleCheck} onChange={(event) => setScaleCheck(event.target.value as AxiomCheckDecision)}>{axiomCheckOptions(t)}</select></label>
            <label>{t.palette}<select value={paletteCheck} onChange={(event) => setPaletteCheck(event.target.value as AxiomCheckDecision)}>{axiomCheckOptions(t)}</select></label>
            <label>{t.blockPlacement}<select value={blockPlacementCheck} onChange={(event) => setBlockPlacementCheck(event.target.value as AxiomCheckDecision)}>{axiomCheckOptions(t)}</select></label>
          </div>
          <label>
            {t.axiomScreenshots}
            <textarea value={axiomScreenshotRefs} onChange={(event) => setAxiomScreenshotRefs(event.target.value)} rows={3} placeholder={t.axiomScreenshotsPlaceholder} />
          </label>
          <label>
            {t.axiomCorrectionNotes}
            <textarea value={axiomCorrectionNotes} onChange={(event) => setAxiomCorrectionNotes(event.target.value)} rows={3} />
          </label>
          <button className="secondary-action compact-action" onClick={saveAxiomAcceptance}>{t.recordAxiomAcceptance}</button>
          {axiomAcceptance ? (
            <p className="comparison-saved" role="status">
              {t.axiomAcceptanceSaved}: {axiomAcceptance.importResult} · {axiomAcceptance.minecraftVersion || t.unknown} · {axiomAcceptance.axiomVersion || t.unknown}
            </p>
          ) : null}
        </section>
      </aside>

    </section>
  );
}

function placementStatusLabel(status: "fits" | "exceeds" | "unknown", t: Translation) {
  return {
    fits: t.fits,
    exceeds: t.exceeds,
    unknown: t.unknown
  }[status];
}

function axiomCheckOptions(t: Translation) {
  return (
    <>
      <option value="pending">{t.pending}</option>
      <option value="passed">{t.passed}</option>
      <option value="failed">{t.failed}</option>
      <option value="not_applicable">{t.notApplicable}</option>
    </>
  );
}

function previewCameraViewLabel(view: PreviewCameraView, t: Translation) {
  return {
    top: t.topView,
    front: t.frontView,
    side: t.sideView,
    perspective: t.perspectiveView
  }[view];
}

function replacementBlockOptions(model: SchematicModel): MinecraftBlockName[] {
  return Array.from(
    new Set<MinecraftBlockName>([
      ...model.palette,
      "minecraft:mossy_stone_bricks",
      "minecraft:quartz_block",
      "minecraft:bricks",
      "minecraft:sandstone",
      "minecraft:dark_prismarine"
    ])
  ).filter((block) => block !== "minecraft:air");
}

function downloadDataUrl(fileName: string, dataUrl: string) {
  const anchor = document.createElement("a");
  anchor.href = dataUrl;
  anchor.download = fileName;
  anchor.click();
}

function candidateGeometryHitsPoint(candidate: MapCandidate, point: { lng: number; lat: number }) {
  const points = candidate.geometry.points;
  if (candidate.geometry.type === "polygon" && points.length >= 3) {
    let inside = false;
    for (let current = 0, previous = points.length - 1; current < points.length; previous = current++) {
      const a = points[current], b = points[previous];
      if ((a.lat > point.lat) !== (b.lat > point.lat) && point.lng < ((b.lng - a.lng) * (point.lat - a.lat)) / (b.lat - a.lat) + a.lng) inside = !inside;
    }
    return inside;
  }
  return points.some((value) => Math.hypot(value.lng - point.lng, value.lat - point.lat) < 0.00008);
}

function paginate<T>(items: T[], page: number, pageSize: number) {
  const safePage = Math.max(1, Math.min(page, Math.max(1, Math.ceil(items.length / pageSize))));
  return items.slice((safePage - 1) * pageSize, safePage * pageSize);
}

function distanceBetweenCandidatePoints(left: MapCandidate, right: MapCandidate) {
  const leftPoint = left.geometry.points[0];
  const rightPoint = right.geometry.points[0];
  if (!leftPoint || !rightPoint) return Number.POSITIVE_INFINITY;
  const latScale = 111_320;
  const lngScale = 111_320 * Math.cos(((leftPoint.lat + rightPoint.lat) / 2) * Math.PI / 180);
  return Math.hypot((leftPoint.lng - rightPoint.lng) * lngScale, (leftPoint.lat - rightPoint.lat) * latScale);
}

function candidateMatchesBuildingSlot(candidate: MapCandidate, slot: BuildingSlot) {
  const point = candidate.geometry.points[0];
  if (!point) return false;
  const center = slot.geometry.points.reduce((sum, item) => ({ lng: sum.lng + item.lng / slot.geometry.points.length, lat: sum.lat + item.lat / slot.geometry.points.length }), { lng: 0, lat: 0 });
  const lat = ((point.lat + center.lat) / 2) * Math.PI / 180;
  const distance = Math.hypot((point.lng - center.lng) * 111_320 * Math.cos(lat), (point.lat - center.lat) * 111_320);
  return distance <= Math.max(40, slot.dimensions.approximateWidthMeters, slot.dimensions.approximateLengthMeters) * 1.5;
}

function Pagination({ current, total, pageSize, onChange, t }: {
  current: number;
  total: number;
  pageSize: number;
  onChange: (page: number) => void;
  t: Translation;
}) {
  const pages = Math.max(1, Math.ceil(total / pageSize));
  if (pages <= 1) return null;
  return <nav className="pagination" aria-label={t.pagination}>
    <button className="review-button" disabled={current <= 1} onClick={() => onChange(current - 1)}>{t.previousPage}</button>
    <strong>{current} / {pages}</strong>
    <button className="review-button" disabled={current >= pages} onClick={() => onChange(current + 1)}>{t.nextPage}</button>
  </nav>;
}

function sourceLabel(source: CandidateSource, t: Translation) {
  return {
    arnis_open_geodata: t.arnisStyle,
    overture: t.overture,
    osm_overpass: t.osm,
    gaode_poi: t.gaodePoi,
    gaode_aoi: t.gaodeAoi,
    screenshot_analysis: t.screenshot,
    manual_drawing: t.manual
  }[source];
}

function featureKindLabel(kind: MapCandidate["kind"], t: Translation) {
  return {
    campus: t.campus,
    building: t.building,
    road: t.road,
    vegetation: t.vegetation,
    water: t.water,
    sports: t.sports
  }[kind];
}

function geometryTypeLabel(type: MapCandidate["geometry"]["type"], t: Translation) {
  return {
    polygon: t.polygon,
    polyline: t.polyline,
    point: t.point,
    line: t.line
  }[type];
}

function confidenceLabel(confidence: MapCandidate["confidence"], t: Translation) {
  return {
    high: t.high,
    medium: t.medium,
    low: t.low,
    manual: t.manual
  }[confidence];
}

function geometryConfidenceLabel(confidence: GeometryConfidence, t: Translation) {
  return {
    high: t.high,
    medium: t.medium,
    low: t.low,
    manual: t.manual,
    missing: t.missing
  }[confidence];
}

function exportRiskLabel(risk: FoundationSchematicPreview["risk"], t: Translation) {
  return {
    ready: t.exportSizeReady,
    large: t.exportSizeLarge,
    very_large: t.exportSizeVeryLarge
  }[risk];
}

function formatNumber(value: number) {
  return new Intl.NumberFormat("en-US").format(value);
}

function providerRoleLabel(
  role: OnlineMapQueryResult["providerDebug"][number]["role"],
  t: Translation
) {
  return {
    preferred_geometry: t.preferredGeometry,
    search_and_naming: t.searchAndNaming,
    fallback: t.fallback
  }[role];
}

function makeSvgProjector(points: Array<{ lng: number; lat: number }>) {
  const width = 620;
  const height = 260;
  const padding = 24;
  const safePoints = points.length
    ? points
    : [PUTUO_ONLINE_QUERY_TARGET.center];
  const minLng = Math.min(...safePoints.map((point) => point.lng));
  const maxLng = Math.max(...safePoints.map((point) => point.lng));
  const minLat = Math.min(...safePoints.map((point) => point.lat));
  const maxLat = Math.max(...safePoints.map((point) => point.lat));
  const lngSpan = Math.max(0.0001, maxLng - minLng);
  const latSpan = Math.max(0.0001, maxLat - minLat);

  function toSvgPoint(point: { lng: number; lat: number }) {
    return {
      x: padding + ((point.lng - minLng) / lngSpan) * (width - padding * 2),
      y: padding + ((maxLat - point.lat) / latSpan) * (height - padding * 2)
    };
  }

  function toLngLat(point: { x: number; y: number }) {
    return {
      lng: minLng + ((point.x - padding) / (width - padding * 2)) * lngSpan,
      lat: maxLat - ((point.y - padding) / (height - padding * 2)) * latSpan
    };
  }

  return {
    width,
    height,
    toSvgPoint,
    toLngLat
  };
}

function svgPath(points: Array<{ x: number; y: number }>, closed: boolean) {
  if (points.length === 0) return "";
  const [first, ...rest] = points;
  return [
    `M ${first.x} ${first.y}`,
    ...rest.map((point) => `L ${point.x} ${point.y}`),
    closed ? "Z" : ""
  ].join(" ");
}
function arnisCandidateToNamingCandidate(candidate: ArnisBuildingCandidate, query: string): MapCandidate {
  const exterior = candidate.components[0]?.exterior ?? [];
  return polygonCandidate({
    id: `campus-corpus-${candidate.source}-${candidate.id}`,
    name: candidate.name?.trim() || `${candidate.source.toUpperCase()} ${candidate.id}`,
    kind: "building",
    source: candidate.source,
    confidence: candidate.identityConfidence === "low" ? "low" : "medium",
    query,
    rawId: candidate.id,
    notes: ["Imported from Arnis live candidate query into the shared Campus Building Corpus."],
    points: exterior.map((point) => [point.lng, point.lat])
  });
}

function mergeCampusCandidateCorpus(current: MapCandidate[], incoming: MapCandidate[]) {
  const bySourceId = new globalThis.Map<string, MapCandidate>();
  for (const candidate of [...current, ...incoming]) {
    bySourceId.set(canonicalBuildingSourceId(candidate.provenance.rawId), candidate);
  }
  return mergeCampusBuildingCandidates([], Array.from(bySourceId.values()));
}
function candidateWithinBoundary(candidate: MapCandidate, boundary: Array<{ lng: number; lat: number }>) {
  if (boundary.length < 3) return false;
  const points = candidate.geometry.points;
  if (!points.length) return false;
  const center = points.reduce((sum, point) => ({ lng: sum.lng + point.lng / points.length, lat: sum.lat + point.lat / points.length }), { lng: 0, lat: 0 });
  let inside = false;
  for (let index = 0, previous = boundary.length - 1; index < boundary.length; previous = index++) {
    const left = boundary[index];
    const right = boundary[previous];
    if (((left.lat > center.lat) !== (right.lat > center.lat)) && center.lng < (right.lng - left.lng) * (center.lat - left.lat) / ((right.lat - left.lat) || Number.EPSILON) + left.lng) inside = !inside;
  }
  return inside;
}
