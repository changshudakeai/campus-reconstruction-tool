import type {
  ArnisRuleDecision,
  BuildingFieldContradiction,
  BuildingFieldDecision,
  BuildingGeometry,
  BuildingGeometryField,
  BuildingGeometryObservation,
  BuildingGeometrySource,
  GeometryConfidence
} from "../domain/buildingGeometry";
import type { SourceResult } from "../adapters/minimalArnisAdapter";
import { createBuildingGeometryObservation } from "./buildingObservation";
import { synchronizeBuildingGeometryDerivedState } from "./buildingGeometryValidation";

const UPSTREAM_COMMIT = "7d2a0ebed00f0b023a4bb8238ea7cbe9d35aa148";
const BUILDINGS_REFERENCE = `louis-e/arnis@${UPSTREAM_COMMIT}:src/element_processing/buildings.rs`;

interface FieldCandidate {
  field: BuildingGeometryField;
  value: unknown;
  source: BuildingGeometrySource;
  observationId: string | null;
  qualityScore: number;
  confidence: GeometryConfidence;
}

export function applyEvidenceDrivenDerivation(
  input: BuildingGeometry,
  results: SourceResult[]
): BuildingGeometry {
  const geometry = structuredClone(input);
  const candidates = collectCandidates(geometry, results);
  const decisions = chooseFieldDecisions(candidates);
  const contradictions = findContradictions(candidates);
  applyDecisions(geometry, decisions);
  const rules = applyArnisRules(geometry, decisions);
  geometry.provenance.fieldDecisions = decisions;
  geometry.provenance.contradictions = contradictions;
  geometry.provenance.arnisRuleDecisions = rules;

  if (rules.length) {
    const baseObservation = selectedRawObservation(geometry);
    if (baseObservation) {
      geometry.provenance.observations.push(createBuildingGeometryObservation({
        id: `arnis-derived:${baseObservation.sourceFeatureId}`,
        source: "arnis_derived",
        sourceFeatureId: baseObservation.sourceFeatureId,
        name: baseObservation.name,
        tags: Object.fromEntries(rules.map((rule) => [`arnis:${rule.ruleId}`, String(rule.output)])),
        components: baseObservation.components,
        normalizationNotes: rules.map((rule) => rule.explanation)
      }));
    }
    geometry.provenance.notes.push(...rules.map((rule) => `Arnis rule ${rule.ruleId}: ${rule.explanation}`));
  }
  geometry.provenance.missingFields = findMissingFields(geometry);
  const footprintDecision = decisions.find((decision) => decision.field === "footprint");
  const acceptedFootprint = geometry.provenance.observations.find((observation) =>
    observation.id === footprintDecision?.observationId
  );
  geometry.footprintComponents = acceptedFootprint?.components.length
    ? structuredClone(acceptedFootprint.components)
    : geometry.footprint.length >= 3
      ? [{ exterior: structuredClone(geometry.footprint), interiorRings: [] }]
      : [];
  return synchronizeBuildingGeometryDerivedState(geometry);
}

function collectCandidates(geometry: BuildingGeometry, results: SourceResult[]) {
  const candidates: FieldCandidate[] = [];
  for (const result of results) {
    const observation = bestObservationForSource(geometry, result.source, result.geometry.observations ?? []);
    const observationId = observation?.id ?? null;
    const quality = sourceQuality(result.source, geometry, observationId);
    const confidence = result.geometry.confidence ?? {};
    push(candidates, "footprint", result.geometry.footprint, result.source, observationId, quality, confidence.footprint);
    push(candidates, "heightM", result.geometry.heightM, result.source, observationId, quality, confidence.height);
    push(candidates, "floors", result.geometry.floors, result.source, observationId, quality, confidence.floors);
    push(candidates, "roof.shape", result.geometry.roof?.shape, result.source, observationId, quality, confidence.roof);
    push(candidates, "roof.material", result.geometry.roof?.material, result.source, observationId, quality, confidence.roof);
    push(candidates, "roof.orientation", result.geometry.roof?.orientation, result.source, observationId, quality, confidence.roof);
    push(candidates, "facade.material", result.geometry.facade?.material, result.source, observationId, quality, confidence.facade);
    push(candidates, "facade.color", result.geometry.facade?.color, result.source, observationId, quality, confidence.facade);
  }
  return candidates;
}

function push(
  candidates: FieldCandidate[],
  field: BuildingGeometryField,
  value: unknown,
  source: BuildingGeometrySource,
  observationId: string | null,
  qualityScore: number,
  confidence: GeometryConfidence | undefined
) {
  if (value === undefined || value === null || value === "") return;
  if (Array.isArray(value) && value.length < 3) return;
  candidates.push({
    field,
    value,
    source,
    observationId,
    qualityScore: Math.min(1, qualityScore + confidenceBonus(confidence)),
    confidence: confidence && confidence !== "missing" ? confidence : defaultConfidence(source)
  });
}

function chooseFieldDecisions(candidates: FieldCandidate[]): BuildingFieldDecision[] {
  const fields = Array.from(new Set(candidates.map((candidate) => candidate.field)));
  return fields.flatMap((field): BuildingFieldDecision[] => {
    const selected = candidates
      .filter((candidate) => candidate.field === field)
      .sort((left, right) => right.qualityScore - left.qualityScore)[0];
    if (!selected) return [];
    return [{
      ...selected,
      ruleId: null,
      explanation: `Selected ${field} from ${selected.source} using evidence quality ${(selected.qualityScore * 100).toFixed(0)}%.`
    }];
  });
}

function findContradictions(candidates: FieldCandidate[]): BuildingFieldContradiction[] {
  const contradictions: BuildingFieldContradiction[] = [];
  for (const field of Array.from(new Set(candidates.map((candidate) => candidate.field)))) {
    const fieldCandidates = candidates.filter((candidate) => candidate.field === field);
    const distinct = fieldCandidates.filter((candidate, index) =>
      fieldCandidates.findIndex((other) => equivalent(field, candidate.value, other.value)) === index
    );
    if (distinct.length > 1) {
      contradictions.push({
        field,
        candidates: distinct.map(({ source, observationId, value, confidence, qualityScore }) => ({
          source,
          observationId,
          value,
          confidence,
          qualityScore
        })),
        message: `${field} has ${distinct.length} contradictory source values; the highest-quality evidence is used.`
      });
    }
  }
  return contradictions;
}

function applyDecisions(geometry: BuildingGeometry, decisions: BuildingFieldDecision[]) {
  for (const decision of decisions) {
    if (decision.field === "footprint") geometry.footprint = decision.value as BuildingGeometry["footprint"];
    if (decision.field === "heightM") geometry.heightM = decision.value as number;
    if (decision.field === "floors") geometry.floors = decision.value as number;
    if (decision.field === "roof.shape") geometry.roof.shape = decision.value as string;
    if (decision.field === "roof.material") geometry.roof.material = decision.value as string;
    if (decision.field === "roof.orientation") geometry.roof.orientation = decision.value as string;
    if (decision.field === "facade.material") geometry.facade.material = decision.value as string;
    if (decision.field === "facade.color") geometry.facade.color = decision.value as string;
    setConfidence(geometry, decision);
  }
}

function applyArnisRules(geometry: BuildingGeometry, decisions: BuildingFieldDecision[]) {
  const rules: ArnisRuleDecision[] = [];
  const selectedObservation = selectedRawObservation(geometry);
  if (!selectedObservation) return rules;
  const tags = selectedObservation?.tags ?? {};
  const inputIds = [selectedObservation.id];
  const selectedHeightDecision = decisions.find((decision) => decision.field === "heightM");
  const mayInterpretSelectedHeight = !selectedHeightDecision ||
    selectedHeightDecision.observationId === selectedObservation.id;

  const explicitHeight = parseMeters(tags.height);
  const minHeight = parseMeters(tags.min_height) ?? 0;
  if (explicitHeight !== null && mayInterpretSelectedHeight) {
    const effectiveHeight = Math.max(1, explicitHeight - minHeight);
    geometry.heightM = effectiveHeight;
    replaceDecision(decisions, {
      field: "heightM",
      value: effectiveHeight,
      source: selectedObservation.source,
      observationId: selectedObservation.id,
      qualityScore: 0.98,
      confidence: "high",
      ruleId: "arnis-explicit-height-overrides-levels",
      explanation: minHeight
        ? `Explicit height ${explicitHeight} m overrides levels; min_height ${minHeight} m is subtracted.`
        : `Explicit height ${explicitHeight} m overrides building levels.`
    });
    geometry.confidence.height = "high";
    rules.push(rule(
      "arnis-explicit-height-overrides-levels",
      "heightM",
      inputIds,
      effectiveHeight,
      minHeight
        ? "Applied Arnis height precedence and subtracted min_height."
        : "Applied Arnis precedence: explicit height overrides building:levels."
    ));
  } else if (geometry.heightM === null && geometry.floors !== null) {
    const derivedHeight = geometry.floors * 4 + 2;
    geometry.heightM = derivedHeight;
    replaceDecision(decisions, {
      field: "heightM",
      value: derivedHeight,
      source: "arnis_derived",
      observationId: selectedObservation.id,
      qualityScore: 0.65,
      confidence: "medium",
      ruleId: "arnis-levels-to-height",
      explanation: `Derived ${derivedHeight} m-equivalent generation height from ${geometry.floors} levels using levels × 4 + 2.`
    });
    geometry.confidence.height = "medium";
    rules.push(rule(
      "arnis-levels-to-height",
      "heightM",
      inputIds,
      derivedHeight,
      "Adapted Arnis levels × 4 + 2 generation-height rule before world output."
    ));
  }

  if (geometry.roof.shape) {
    const normalized = normalizeArnisRoofShape(geometry.roof.shape);
    if (normalized !== geometry.roof.shape) {
      geometry.roof.shape = normalized;
      rules.push(rule(
        "arnis-roof-shape-synonyms",
        "roof.shape",
        inputIds,
        normalized,
        "Normalized an explicit roof:shape synonym using Arnis parse_roof_type mappings."
      ));
      const decision = decisions.find((item) => item.field === "roof.shape");
      if (decision) {
        decision.value = normalized;
        decision.ruleId = "arnis-roof-shape-synonyms";
        decision.explanation = "Explicit roof shape retained and normalized through Arnis synonym mapping.";
      }
    }
  } else {
    geometry.roof.shape = "flat";
    geometry.confidence.roof = "low";
    replaceDecision(decisions, {
      field: "roof.shape",
      value: "flat",
      source: "arnis_derived",
      observationId: selectedObservation.id,
      qualityScore: 0.45,
      confidence: "low",
      ruleId: "arnis-default-flat-roof",
      explanation: "No explicit roof:shape was observed; Arnis falls back to Flat for non-residential auto-gable categories."
    });
    rules.push(rule(
      "arnis-default-flat-roof",
      "roof.shape",
      inputIds,
      "flat",
      "Used Arnis default roof behavior because the library is not an auto-gabled residential/agricultural type."
    ));
  }

  if (isInstitutional(tags, geometry.buildingName)) {
    rules.push(rule(
      "arnis-school-institutional-bands",
      "facade.rhythm",
      inputIds,
      "institutional_bands",
      "Applied Arnis School preset guidance: regular windows, accent roof line, parapet, and InstitutionalBands wall depth."
    ));
  }
  return rules;
}

function rule(
  ruleId: string,
  field: ArnisRuleDecision["field"],
  inputObservationIds: string[],
  output: unknown,
  explanation: string
): ArnisRuleDecision {
  return { ruleId, field, upstreamReference: BUILDINGS_REFERENCE, inputObservationIds, output, explanation };
}

function replaceDecision(decisions: BuildingFieldDecision[], decision: BuildingFieldDecision) {
  const index = decisions.findIndex((item) => item.field === decision.field);
  if (index >= 0) decisions[index] = decision;
  else decisions.push(decision);
}

function selectedRawObservation(geometry: BuildingGeometry) {
  const selectedId = geometry.provenance.identityResolution.selectedObservationId;
  const isRawProvider = (observation: BuildingGeometryObservation) =>
    observation.source === "overture" || observation.source === "osm_overpass";
  return geometry.provenance.observations.find((observation) =>
    observation.id === selectedId && isRawProvider(observation)
  ) ?? geometry.provenance.observations.find(isRawProvider) ?? null;
}

function bestObservationForSource(
  geometry: BuildingGeometry,
  source: BuildingGeometrySource,
  observations: BuildingGeometryObservation[]
) {
  const selectedId = geometry.provenance.identityResolution.selectedObservationId;
  return observations.find((observation) => observation.id === selectedId) ??
    observations.find((observation) => observation.source === source) ?? null;
}

function sourceQuality(source: BuildingGeometrySource, geometry: BuildingGeometry, observationId: string | null) {
  const base = {
    manual_correction: 0.98,
    existing_project: 0.82,
    overture: 0.76,
    osm_overpass: 0.72,
    arnis_derived: 0.65
  }[source];
  const match = geometry.provenance.identityResolution.matches.find((item) => item.observationId === observationId);
  const identityBonus = match ? match.score * 0.16 : 0;
  const selectedBonus = geometry.provenance.identityResolution.selectedObservationId === observationId ? 0.08 : 0;
  return Math.min(1, base + identityBonus + selectedBonus);
}

function confidenceBonus(confidence: GeometryConfidence | undefined) {
  return { high: 0.08, medium: 0.04, low: 0, manual: 0.1, missing: 0 }[confidence ?? "missing"];
}

function defaultConfidence(source: BuildingGeometrySource): GeometryConfidence {
  if (source === "manual_correction") return "manual";
  if (source === "overture") return "high";
  if (source === "osm_overpass" || source === "arnis_derived") return "medium";
  return "low";
}

function equivalent(field: BuildingGeometryField, left: unknown, right: unknown) {
  if (typeof left === "number" && typeof right === "number") {
    return Math.abs(left - right) <= (field === "heightM" ? 1 : 0);
  }
  return JSON.stringify(left) === JSON.stringify(right);
}

function setConfidence(geometry: BuildingGeometry, decision: BuildingFieldDecision) {
  if (decision.field === "footprint") geometry.confidence.footprint = decision.confidence;
  if (decision.field === "heightM") geometry.confidence.height = decision.confidence;
  if (decision.field === "floors") geometry.confidence.floors = decision.confidence;
  if (decision.field.startsWith("roof.")) geometry.confidence.roof = decision.confidence;
  if (decision.field.startsWith("facade.")) geometry.confidence.facade = decision.confidence;
}

function parseMeters(value: string | undefined) {
  if (!value) return null;
  const parsed = Number.parseFloat(value.trim().replace(/m$/i, ""));
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function normalizeArnisRoofShape(value: string) {
  const shape = value.toLocaleLowerCase();
  if (["gabled", "gable", "pitched", "saltbox", "double_saltbox", "quadruple_saltbox", "gabled_row"].includes(shape)) return "gabled";
  if (["hipped", "hip", "half-hipped", "gambrel", "mansard", "round", "side_hipped", "side_half-hipped"].includes(shape)) return "hipped";
  if (["skillion", "shed", "lean_to", "monopitch"].includes(shape)) return "skillion";
  if (["pyramidal", "pyramid"].includes(shape)) return "pyramidal";
  if (["dome", "spherical"].includes(shape)) return "dome";
  if (["cone", "conical", "circular", "spire"].includes(shape)) return "cone";
  if (shape === "onion") return "onion";
  return "flat";
}

function isInstitutional(tags: Record<string, string>, buildingName: string) {
  const values = [tags.building, tags.amenity, tags.class, tags.subtype, buildingName]
    .filter(Boolean)
    .join(" ")
    .toLocaleLowerCase();
  return /school|university|college|library|教育|大学|图书馆/.test(values);
}

function findMissingFields(geometry: BuildingGeometry) {
  const missing: string[] = [];
  if (geometry.footprint.length < 3) missing.push("footprint");
  if (!geometry.heightM) missing.push("heightM");
  if (!geometry.floors) missing.push("floors");
  if (!geometry.roof.shape) missing.push("roof.shape");
  if (!geometry.facade.material && !geometry.facade.color) missing.push("facade");
  return missing;
}
