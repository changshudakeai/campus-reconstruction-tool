import {
  BUILDING_GEOMETRY_SOURCE_PRIORITY,
  BuildingGeometry,
  BuildingGeometryHandoff,
  BuildingGeometrySource,
  BuildingSourceRecord,
  BuildingGeometryObservation,
  BuildingTarget,
  FacadeHints,
  FieldConfidence,
  GeometryConfidence,
  LngLatPoint,
  RoofHints
} from "../domain/buildingGeometry";
import { resolveBuildingIdentity } from "../services/buildingIdentity";
import { applyEvidenceDrivenDerivation } from "../services/buildingGeometryDerivation";

export type PartialGeometry = {
  footprint?: LngLatPoint[];
  heightM?: number | null;
  floors?: number | null;
  roof?: Partial<RoofHints>;
  facade?: Partial<FacadeHints>;
  confidence?: Partial<FieldConfidence>;
  notes?: string[];
  handoff?: BuildingGeometryHandoff | null;
  sourceRecords?: BuildingSourceRecord[];
  observations?: BuildingGeometryObservation[];
};

export interface BuildingGeometryProvider {
  source: BuildingGeometrySource;
  fetchBuildingGeometry(target: BuildingTarget): Promise<PartialGeometry | null>;
}

export type SourceResult = {
  source: BuildingGeometrySource;
  geometry: PartialGeometry;
};

const EMPTY_ROOF: RoofHints = {
  shape: null,
  material: null,
  orientation: null
};

const EMPTY_FACADE: FacadeHints = {
  material: null,
  color: null
};

const MISSING_CONFIDENCE: FieldConfidence = {
  footprint: "missing",
  height: "missing",
  floors: "missing",
  roof: "missing",
  facade: "missing"
};

export class MinimalArnisAdapter {
  constructor(
    private readonly providers: BuildingGeometryProvider[],
    private readonly sourcePriority = BUILDING_GEOMETRY_SOURCE_PRIORITY
  ) {}

  async getBuildingGeometry(target: BuildingTarget): Promise<BuildingGeometry> {
    const orderedProviders = this.sourcePriority
      .map((source) => this.providers.find((provider) => provider.source === source))
      .filter((provider): provider is BuildingGeometryProvider => Boolean(provider));

    const results: SourceResult[] = [];
    for (const provider of orderedProviders) {
      const geometry = await provider.fetchBuildingGeometry(target);
      if (geometry) {
        results.push({ source: provider.source, geometry });
      }
    }

    return mergeBuildingGeometry(target, this.sourcePriority, results);
  }
}

export function mergeBuildingGeometry(
  target: BuildingTarget,
  sourcePriority: BuildingGeometrySource[],
  results: SourceResult[]
): BuildingGeometry {
  const footprint = firstValue(results, (result) => validFootprint(result.geometry.footprint));
  const heightM = firstValue(results, (result) => validNumber(result.geometry.heightM));
  const floors = firstValue(results, (result) => validNumber(result.geometry.floors));
  const roof = mergeObject(results, EMPTY_ROOF, (result) => result.geometry.roof);
  const facade = mergeObject(results, EMPTY_FACADE, (result) => result.geometry.facade);
  const confidence = mergeConfidence(results);
  const missingFields = missingGeometryFields({ footprint, heightM, floors, roof, facade });
  const handoff = firstValue(results, (result) => result.geometry.handoff ?? undefined) ?? null;
  const observations = results.flatMap((result) => result.geometry.observations ?? []);
  const identityResolution = resolveBuildingIdentity(target, observations);

  const geometry: BuildingGeometry = {
    schemaVersion: "0.1.0",
    buildingName: target.name,
    target,
    footprint: footprint ?? [],
    footprintComponents: footprint ? [{ exterior: footprint, interiorRings: [] }] : [],
    orientationDegrees: 0,
    scale: { areaSquareMeters: 0, widthMeters: 0, lengthMeters: 0 },
    heightM: heightM ?? null,
    floors: floors ?? null,
    floorSpacingMeters: null,
    roof,
    facade,
    confidence,
    validation: {
      valid: false,
      errors: [],
      warnings: [],
      componentCount: 0,
      orientationDegrees: 0,
      scale: { areaSquareMeters: 0, widthMeters: 0, lengthMeters: 0 },
      floorSpacingMeters: null
    },
    provenance: {
      sourcePriority,
      usedSources: results.map((result) => result.source),
      missingFields,
      notes: results.flatMap((result) => result.geometry.notes ?? []),
      handoff,
      sourceRecords: results.flatMap((result) => result.geometry.sourceRecords ?? []),
      observations,
      identityResolution,
      observationReviews: Object.fromEntries(observations.map((observation) => [
        observation.id,
        observation.source === "existing_project" ? "supporting" : "pending"
      ])),
      fieldDecisions: [],
      contradictions: [],
      arnisRuleDecisions: [],
      generationAssumptions: [],
      corrections: []
    }
  };
  return applyEvidenceDrivenDerivation(geometry, results);
}

function firstValue<T>(
  results: SourceResult[],
  getter: (result: SourceResult) => T | undefined
): T | undefined {
  for (const result of results) {
    const value = getter(result);
    if (value !== undefined) {
      return value;
    }
  }
  return undefined;
}

function validFootprint(value: LngLatPoint[] | undefined): LngLatPoint[] | undefined {
  return value && value.length >= 3 ? value : undefined;
}

function validNumber(value: number | null | undefined): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : undefined;
}

function mergeObject<T extends object>(
  results: SourceResult[],
  emptyValue: T,
  getter: (result: SourceResult) => Partial<T> | undefined
): T {
  const merged = { ...emptyValue };
  for (const key of Object.keys(emptyValue) as Array<keyof T & string>) {
    for (const result of results) {
      const value = getter(result)?.[key];
      if (value !== undefined && value !== null && value !== "") {
        merged[key] = value as T[typeof key];
        break;
      }
    }
  }
  return merged;
}

function mergeConfidence(results: SourceResult[]): FieldConfidence {
  const confidence = { ...MISSING_CONFIDENCE };
  for (const field of Object.keys(confidence) as Array<keyof FieldConfidence>) {
    for (const result of results) {
      const value = result.geometry.confidence?.[field];
      if (value && value !== "missing") {
        confidence[field] = value;
        break;
      }
    }
  }
  return confidence;
}

function missingGeometryFields(fields: {
  footprint?: LngLatPoint[];
  heightM?: number;
  floors?: number;
  roof: RoofHints;
  facade: FacadeHints;
}): string[] {
  const missing: string[] = [];
  if (!fields.footprint || fields.footprint.length < 3) missing.push("footprint");
  if (!fields.heightM) missing.push("heightM");
  if (!fields.floors) missing.push("floors");
  if (!fields.roof.shape) missing.push("roof.shape");
  if (!fields.facade.material && !fields.facade.color) missing.push("facade");
  return missing;
}

export function confidenceFromSource(source: BuildingGeometrySource): GeometryConfidence {
  if (source === "overture") return "high";
  if (source === "osm_overpass") return "medium";
  if (source === "existing_project") return "low";
  if (source === "arnis_derived") return "medium";
  return "manual";
}
