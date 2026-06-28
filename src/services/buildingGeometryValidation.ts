import type {
  BuildingGenerationAssumption,
  BuildingGeometry,
  BuildingGeometryValidation,
  FootprintComponent
} from "../domain/buildingGeometry";
import { measureFootprintComponents } from "./buildingObservation";

const MIN_FLOOR_SPACING_METERS = 2.4;
const MAX_FLOOR_SPACING_METERS = 8;
const MAX_BUILDING_SPAN_METERS = 500;

export function synchronizeBuildingGeometryDerivedState(geometry: BuildingGeometry) {
  const components = normalizedComponents(geometry);
  geometry.footprintComponents = components;
  geometry.footprint = components[0]?.exterior.map((point) => ({ ...point })) ?? [];

  const metrics = measureFootprintComponents(components);
  geometry.orientationDegrees = metrics.orientationDegrees;
  geometry.scale = {
    areaSquareMeters: metrics.areaSquareMeters,
    widthMeters: metrics.widthMeters,
    lengthMeters: metrics.lengthMeters
  };
  geometry.floorSpacingMeters = geometry.heightM && geometry.floors
    ? geometry.heightM / geometry.floors
    : null;
  geometry.validation = validateBuildingGeometry(geometry);
  geometry.provenance.generationAssumptions = deriveGenerationAssumptions(geometry);
  return geometry;
}

export function validateBuildingGeometry(geometry: BuildingGeometry): BuildingGeometryValidation {
  const errors: string[] = [];
  const warnings: string[] = [];
  const components = geometry.footprintComponents;
  const metrics = measureFootprintComponents(components);
  const floorSpacingMeters = geometry.heightM && geometry.floors
    ? geometry.heightM / geometry.floors
    : null;

  if (!components.length) errors.push("Building Geometry requires at least one footprint component.");
  for (const [index, component] of components.entries()) {
    if (component.exterior.length < 3) {
      errors.push(`Footprint component ${index + 1} requires at least three exterior points.`);
    }
    if (component.interiorRings.some((ring) => ring.length < 3)) {
      errors.push(`Footprint component ${index + 1} contains an invalid interior ring.`);
    }
    if (![component.exterior, ...component.interiorRings].flat().every((point) =>
      Number.isFinite(point.lng) && Number.isFinite(point.lat) &&
      point.lng >= -180 && point.lng <= 180 && point.lat >= -90 && point.lat <= 90
    )) {
      errors.push(`Footprint component ${index + 1} contains invalid coordinates.`);
    }
  }
  if (components.length && metrics.areaSquareMeters <= 1) {
    errors.push("Building footprint area must exceed one square meter.");
  }
  if (metrics.widthMeters > MAX_BUILDING_SPAN_METERS || metrics.lengthMeters > MAX_BUILDING_SPAN_METERS) {
    errors.push(`Building footprint span must not exceed ${MAX_BUILDING_SPAN_METERS} meters.`);
  }
  if (geometry.heightM !== null && (geometry.heightM < 3 || geometry.heightM > 300)) {
    errors.push("Building height must be between 3 and 300 meters.");
  }
  if (geometry.floors !== null && (!Number.isInteger(geometry.floors) || geometry.floors < 1 || geometry.floors > 100)) {
    errors.push("Building floors must be an integer between 1 and 100.");
  }
  if (floorSpacingMeters !== null && (
    floorSpacingMeters < MIN_FLOOR_SPACING_METERS || floorSpacingMeters > MAX_FLOOR_SPACING_METERS
  )) {
    errors.push(
      `Floor spacing must be between ${MIN_FLOOR_SPACING_METERS} and ${MAX_FLOOR_SPACING_METERS} meters.`
    );
  }
  if (geometry.heightM === null) warnings.push("Height is missing; generation requires an explicit fallback assumption.");
  if (geometry.floors === null) warnings.push("Floor count is missing; generation requires an explicit fallback assumption.");

  return {
    valid: errors.length === 0,
    errors,
    warnings,
    componentCount: components.length,
    orientationDegrees: metrics.orientationDegrees,
    scale: {
      areaSquareMeters: metrics.areaSquareMeters,
      widthMeters: metrics.widthMeters,
      lengthMeters: metrics.lengthMeters
    },
    floorSpacingMeters
  };
}

function normalizedComponents(geometry: BuildingGeometry): FootprintComponent[] {
  const components = geometry.footprintComponents.length
    ? geometry.footprintComponents
    : geometry.footprint.length >= 3
      ? [{ exterior: geometry.footprint, interiorRings: [] }]
      : [];
  return components.map((component) => ({
    exterior: component.exterior.map((point) => ({ ...point })),
    interiorRings: component.interiorRings.map((ring) => ring.map((point) => ({ ...point })))
  }));
}

function deriveGenerationAssumptions(geometry: BuildingGeometry): BuildingGenerationAssumption[] {
  const assumptions: BuildingGenerationAssumption[] = geometry.provenance.arnisRuleDecisions.flatMap((rule) => {
    const decision = geometry.provenance.fieldDecisions.find((item) =>
      item.field === rule.field && item.ruleId === rule.ruleId && item.source === "arnis_derived"
    );
    return decision ? [{
      field: decision.field,
      value: decision.value,
      reason: decision.explanation,
      ruleId: rule.ruleId
    }] : [];
  });

  if (geometry.heightM === null) {
    assumptions.push({
      field: "heightM",
      value: 12,
      reason: "No observed or corrected height is available; schematic generation uses a 12-block fallback.",
      ruleId: "generator-default-height"
    });
  }
  if (geometry.floors === null) {
    assumptions.push({
      field: "floors",
      value: 1,
      reason: "No observed or corrected floor count is available; schematic generation uses one floor.",
      ruleId: "generator-default-floors"
    });
  }
  if (!geometry.roof.shape) {
    assumptions.push({
      field: "roof.shape",
      value: "flat",
      reason: "No accepted roof shape is available; schematic generation uses a flat cap.",
      ruleId: "generator-default-roof"
    });
  }
  if (geometry.floorSpacingMeters !== null) {
    assumptions.push({
      field: "floorSpacingMeters",
      value: geometry.floorSpacingMeters,
      reason: `Reconciled ${geometry.heightM} meters across ${geometry.floors} floors.`,
      ruleId: "height-floor-reconciliation"
    });
  }
  return assumptions;
}
