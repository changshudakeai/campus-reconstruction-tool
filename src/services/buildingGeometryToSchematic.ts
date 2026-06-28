import type {
  BuildingGeometry,
  FootprintComponent,
  LngLatPoint
} from "../domain/buildingGeometry";
import {
  countBlocks,
  createEmptySchematicModel,
  type MinecraftBlockName,
  type SchematicGenerationReport,
  type SchematicModel,
  setBlock
} from "../domain/schematicModel";

interface LocalPoint {
  x: number;
  z: number;
}

interface LocalComponent {
  exterior: LocalPoint[];
  interiorRings: LocalPoint[][];
}

export interface BuildingMaterialStyle {
  foundation: MinecraftBlockName;
  wall: MinecraftBlockName;
  glass: MinecraftBlockName;
  roof: MinecraftBlockName;
  floor: MinecraftBlockName;
  entrance: MinecraftBlockName;
  accent: MinecraftBlockName;
}

export interface SchematicGenerationOptions {
  blocksPerMeter?: number;
  paddingBlocks?: number;
  fallbackHeightBlocks?: number;
  materialStyle?: Partial<BuildingMaterialStyle>;
}

export const DEFAULT_BUILDING_MATERIAL_STYLE: BuildingMaterialStyle = {
  foundation: "minecraft:smooth_stone",
  wall: "minecraft:stone_bricks",
  glass: "minecraft:glass",
  roof: "minecraft:dark_oak_slab",
  floor: "minecraft:oak_planks",
  entrance: "minecraft:dark_oak_door",
  accent: "minecraft:polished_andesite"
};

const AIR = 0;
const FOUNDATION = 1;
const WALL = 2;
const GLASS = 3;
const ROOF = 4;
const FLOOR = 5;
const ENTRANCE = 6;
const ACCENT = 7;
const MAX_SCHEMATIC_SPAN = 512;
const SUPPORTED_ROOFS = new Set([
  "flat",
  "hipped",
  "gabled",
  "skillion",
  "pyramidal",
  "dome",
  "cone",
  "onion"
]);

export function generateSchematicFromBuildingGeometry(
  geometry: BuildingGeometry,
  options: SchematicGenerationOptions = {}
): SchematicModel {
  validateGenerationInput(geometry, options);

  const blocksPerMeter = options.blocksPerMeter ?? 1;
  const paddingBlocks = options.paddingBlocks ?? 3;
  const fallbackHeightBlocks = options.fallbackHeightBlocks ?? 12;
  const materialStyle = { ...DEFAULT_BUILDING_MATERIAL_STYLE, ...(options.materialStyle ?? {}) };
  const palette = makePalette(materialStyle);
  const components = toLocalComponents(geometry.footprintComponents, blocksPerMeter);
  const acceptedBounds = localBounds(components.flatMap(componentPoints));
  const shiftedComponents = shiftComponents(components, acceptedBounds, paddingBlocks);
  const width = Math.max(8, Math.ceil(acceptedBounds.maxX - acceptedBounds.minX) + paddingBlocks * 2 + 1);
  const length = Math.max(8, Math.ceil(acceptedBounds.maxZ - acceptedBounds.minZ) + paddingBlocks * 2 + 1);
  const wallHeight = Math.max(
    5,
    Math.ceil((geometry.heightM ?? fallbackHeightBlocks) * blocksPerMeter)
  );
  const floorCount = Math.max(1, geometry.floors ?? 1);
  const floorSpacingBlocks = Math.max(3, Math.round(wallHeight / floorCount));
  const roofShape = normalizedRoofShape(geometry.roof.shape);
  const roofHeight = roofHeightForShape(roofShape, width, length);
  const height = wallHeight + roofHeight + 2;
  if (width > MAX_SCHEMATIC_SPAN || length > MAX_SCHEMATIC_SPAN || height > MAX_SCHEMATIC_SPAN) {
    throw new Error(`Generated schematic dimensions must not exceed ${MAX_SCHEMATIC_SPAN} blocks.`);
  }

  const mask = buildFootprintMask(width, length, shiftedComponents);
  if (!mask.some((row) => row.some(Boolean))) {
    throw new Error("Building Geometry footprint did not occupy any voxel cells at the requested scale.");
  }
  const boundaryDistance = buildBoundaryDistance(mask, width, length);
  const entranceCells = selectEntranceCells(mask, 5);
  const model = createEmptySchematicModel({
    name: safeSchematicName(geometry.buildingName),
    width,
    height,
    length,
    palette,
    sourceBuilding: geometry.buildingName,
    nonRectangularFootprint: hasNonRectangularFootprint(mask),
    roofShape,
    provenance: buildingGeometryProvenance(geometry)
  });

  for (let z = 0; z < length; z += 1) {
    for (let x = 0; x < width; x += 1) {
      if (!mask[z][x]) continue;
      setBlock(model, x, 0, z, FOUNDATION);
      addInteriorFloors(model, x, z, wallHeight, floorSpacingBlocks);
      if (isBoundaryCell(mask, x, z)) {
        addFacadeColumn(
          model,
          x,
          z,
          wallHeight,
          floorSpacingBlocks,
          entranceCells.has(cellKey(x, z))
        );
      }
    }
  }

  addRoof(model, mask, boundaryDistance, wallHeight, roofHeight, roofShape);
  model.metadata.generationReport = createGenerationReport({
    geometry,
    model,
    mask,
    components: shiftedComponents,
    acceptedBounds,
    blocksPerMeter,
    paddingBlocks,
    floorCount,
    floorSpacingBlocks,
    roofShape,
    roofHeight,
    entranceWidth: entranceCells.size
  });
  return model;
}

function validateGenerationInput(
  geometry: BuildingGeometry,
  options: SchematicGenerationOptions
) {
  if (!geometry.footprintComponents.length || geometry.footprint.length < 3) {
    throw new Error("Building Geometry must include at least one polygon footprint component.");
  }
  if (!geometry.validation.valid) {
    throw new Error(`Building Geometry validation failed: ${geometry.validation.errors.join(" ")}`);
  }
  const blocksPerMeter = options.blocksPerMeter ?? 1;
  if (!Number.isFinite(blocksPerMeter) || blocksPerMeter < 0.25 || blocksPerMeter > 4) {
    throw new Error("blocksPerMeter must be between 0.25 and 4.");
  }
  const paddingBlocks = options.paddingBlocks ?? 3;
  if (!Number.isInteger(paddingBlocks) || paddingBlocks < 1 || paddingBlocks > 64) {
    throw new Error("paddingBlocks must be an integer between 1 and 64.");
  }
  const fallbackHeightBlocks = options.fallbackHeightBlocks ?? 12;
  if (!Number.isFinite(fallbackHeightBlocks) || fallbackHeightBlocks < 3 || fallbackHeightBlocks > 300) {
    throw new Error("fallbackHeightBlocks must be between 3 and 300.");
  }
  if (geometry.roof.shape && !SUPPORTED_ROOFS.has(geometry.roof.shape.toLowerCase())) {
    throw new Error(`Unsupported roof shape: ${geometry.roof.shape}.`);
  }
  makePalette({ ...DEFAULT_BUILDING_MATERIAL_STYLE, ...(options.materialStyle ?? {}) });
}

function makePalette(style: BuildingMaterialStyle): MinecraftBlockName[] {
  const blocks = [
    style.foundation,
    style.wall,
    style.glass,
    style.roof,
    style.floor,
    style.entrance,
    style.accent
  ];
  if (blocks.some((block) => !/^minecraft:[a-z0-9_./-]+(?:\[.+\])?$/.test(block))) {
    throw new Error("Material style contains an invalid Minecraft block identifier.");
  }
  if (new Set(blocks).size !== blocks.length || blocks.includes("minecraft:air")) {
    throw new Error("Material style blocks must be unique non-air blocks.");
  }
  return ["minecraft:air", ...blocks];
}

function toLocalComponents(components: FootprintComponent[], blocksPerMeter: number) {
  const origin = components[0].exterior[0];
  return components.map((component) => ({
    exterior: component.exterior.map((point) => toLocalPoint(point, origin, blocksPerMeter)),
    interiorRings: component.interiorRings.map((ring) =>
      ring.map((point) => toLocalPoint(point, origin, blocksPerMeter))
    )
  }));
}

function toLocalPoint(point: LngLatPoint, origin: LngLatPoint, blocksPerMeter: number): LocalPoint {
  const metersPerDegreeLat = 111_320;
  const metersPerDegreeLng = metersPerDegreeLat * Math.cos(origin.lat * Math.PI / 180);
  return {
    x: (point.lng - origin.lng) * metersPerDegreeLng * blocksPerMeter,
    z: (point.lat - origin.lat) * metersPerDegreeLat * blocksPerMeter
  };
}

function shiftComponents(components: LocalComponent[], bounds: ReturnType<typeof localBounds>, padding: number) {
  return components.map((component) => ({
    exterior: component.exterior.map((point) => ({
      x: point.x - bounds.minX + padding,
      z: point.z - bounds.minZ + padding
    })),
    interiorRings: component.interiorRings.map((ring) => ring.map((point) => ({
      x: point.x - bounds.minX + padding,
      z: point.z - bounds.minZ + padding
    })))
  }));
}

function componentPoints(component: LocalComponent) {
  return [component.exterior, ...component.interiorRings].flat();
}

function localBounds(points: LocalPoint[]) {
  return points.reduce((bounds, point) => ({
    minX: Math.min(bounds.minX, point.x),
    maxX: Math.max(bounds.maxX, point.x),
    minZ: Math.min(bounds.minZ, point.z),
    maxZ: Math.max(bounds.maxZ, point.z)
  }), {
    minX: Number.POSITIVE_INFINITY,
    maxX: Number.NEGATIVE_INFINITY,
    minZ: Number.POSITIVE_INFINITY,
    maxZ: Number.NEGATIVE_INFINITY
  });
}

function buildFootprintMask(width: number, length: number, components: LocalComponent[]) {
  return Array.from({ length }, (_, z) =>
    Array.from({ length: width }, (_, x) => pointInComponents({ x: x + 0.5, z: z + 0.5 }, components))
  );
}

function pointInComponents(point: LocalPoint, components: LocalComponent[]) {
  return components.some((component) =>
    pointInPolygon(point, component.exterior) &&
    !component.interiorRings.some((ring) => pointInPolygon(point, ring))
  );
}

function pointInPolygon(point: LocalPoint, polygon: LocalPoint[]) {
  let inside = false;
  for (let index = 0, previous = polygon.length - 1; index < polygon.length; previous = index, index += 1) {
    const current = polygon[index];
    const prior = polygon[previous];
    const crosses = current.z > point.z !== prior.z > point.z &&
      point.x < ((prior.x - current.x) * (point.z - current.z)) / (prior.z - current.z) + current.x;
    if (crosses) inside = !inside;
  }
  return inside;
}

function addInteriorFloors(
  model: SchematicModel,
  x: number,
  z: number,
  wallHeight: number,
  floorSpacing: number
) {
  for (let y = floorSpacing; y < wallHeight; y += floorSpacing) setBlock(model, x, y, z, FLOOR);
}

function addFacadeColumn(
  model: SchematicModel,
  x: number,
  z: number,
  wallHeight: number,
  floorSpacing: number,
  entrance: boolean
) {
  for (let y = 1; y <= wallHeight; y += 1) {
    if (entrance && y <= Math.min(3, wallHeight)) {
      setBlock(model, x, y, z, ENTRANCE);
      continue;
    }
    if (y < wallHeight && y % floorSpacing === 0) {
      setBlock(model, x, y, z, ACCENT);
      continue;
    }
    const withinFloor = y % floorSpacing;
    const windowBay = (x + z) % 4 === 1 || (x + z) % 4 === 2;
    const windowHeight = withinFloor >= 2 && withinFloor <= Math.max(2, floorSpacing - 1);
    setBlock(model, x, y, z, windowBay && windowHeight ? GLASS : WALL);
  }
}

function addRoof(
  model: SchematicModel,
  mask: boolean[][],
  boundaryDistance: number[][],
  wallHeight: number,
  roofHeight: number,
  roofShape: string
) {
  const centerX = (model.width - 1) / 2;
  const centerZ = (model.length - 1) / 2;
  const shortAxisIsX = model.width <= model.length;
  const shortCenter = shortAxisIsX ? centerX : centerZ;
  const shortRadius = Math.max(1, (shortAxisIsX ? model.width : model.length) / 2);

  for (let z = 0; z < model.length; z += 1) {
    for (let x = 0; x < model.width; x += 1) {
      if (!mask[z][x]) continue;
      let rise = 1;
      if (roofShape === "hipped" || roofShape === "pyramidal") {
        rise = Math.min(Math.max(boundaryDistance[z][x], 1), roofHeight);
      } else if (roofShape === "gabled") {
        const short = shortAxisIsX ? x : z;
        rise = 1 + Math.round((1 - Math.min(1, Math.abs(short - shortCenter) / shortRadius)) * (roofHeight - 1));
      } else if (roofShape === "skillion") {
        rise = 1 + Math.round((x / Math.max(1, model.width - 1)) * (roofHeight - 1));
      } else if (["dome", "cone", "onion"].includes(roofShape)) {
        const normalizedDistance = Math.hypot(
          (x - centerX) / Math.max(1, model.width / 2),
          (z - centerZ) / Math.max(1, model.length / 2)
        );
        rise = 1 + Math.round(Math.max(0, 1 - normalizedDistance) * (roofHeight - 1));
      }
      setBlock(model, x, wallHeight + Math.min(roofHeight, rise), z, ROOF);
    }
  }
}

function roofHeightForShape(shape: string, width: number, length: number) {
  if (shape === "flat") return 2;
  const shortSpan = Math.min(width, length);
  return Math.max(3, Math.min(8, Math.round(shortSpan * 0.12)));
}

function normalizedRoofShape(shape: string | null) {
  return shape?.toLowerCase() || "flat";
}

function selectEntranceCells(mask: boolean[][], requestedWidth: number) {
  const boundary: Array<{ x: number; z: number }> = [];
  for (let z = 0; z < mask.length; z += 1) {
    for (let x = 0; x < mask[z].length; x += 1) {
      if (mask[z][x] && isBoundaryCell(mask, x, z)) boundary.push({ x, z });
    }
  }
  const south = Math.max(...boundary.map((cell) => cell.z));
  const centerX = (mask[0].length - 1) / 2;
  return new Set(
    boundary
      .filter((cell) => cell.z >= south - 1)
      .sort((left, right) => Math.abs(left.x - centerX) - Math.abs(right.x - centerX))
      .slice(0, requestedWidth)
      .map((cell) => cellKey(cell.x, cell.z))
  );
}

function createGenerationReport(input: {
  geometry: BuildingGeometry;
  model: SchematicModel;
  mask: boolean[][];
  components: LocalComponent[];
  acceptedBounds: ReturnType<typeof localBounds>;
  blocksPerMeter: number;
  paddingBlocks: number;
  floorCount: number;
  floorSpacingBlocks: number;
  roofShape: string;
  roofHeight: number;
  entranceWidth: number;
}): SchematicGenerationReport {
  const fidelity = measureRasterizationFidelity(
    input.mask,
    input.components,
    input.geometry,
    input.blocksPerMeter
  );
  const blockCounts = Object.fromEntries(
    input.model.palette.map((block, index) => [block, countBlocks(input.model, index)])
  );
  const roofAssumption = input.geometry.provenance.generationAssumptions.find((assumption) =>
    assumption.field === "roof.shape"
  );
  return {
    blocksPerMeter: input.blocksPerMeter,
    paddingBlocks: input.paddingBlocks,
    dimensions: {
      widthBlocks: input.model.width,
      heightBlocks: input.model.height,
      lengthBlocks: input.model.length,
      footprintWidthMeters: (input.acceptedBounds.maxX - input.acceptedBounds.minX) / input.blocksPerMeter,
      footprintLengthMeters: (input.acceptedBounds.maxZ - input.acceptedBounds.minZ) / input.blocksPerMeter
    },
    orientationDegrees: input.geometry.orientationDegrees,
    floorCount: input.floorCount,
    floorSpacingBlocks: input.floorSpacingBlocks,
    roof: {
      shape: input.roofShape,
      heightBlocks: input.roofHeight,
      assumption: roofAssumption?.reason ?? null
    },
    facadeRhythm: "institutional-window-bays-with-floor-bands",
    entrance: { side: "south", widthBlocks: input.entranceWidth },
    blockCounts,
    semanticBlockCounts: {
      foundation: countBlocks(input.model, FOUNDATION),
      walls: countBlocks(input.model, WALL),
      windows: countBlocks(input.model, GLASS),
      roof: countBlocks(input.model, ROOF),
      floors: countBlocks(input.model, FLOOR),
      entrance: countBlocks(input.model, ENTRANCE),
      accents: countBlocks(input.model, ACCENT)
    },
    fidelity,
    footprintOverlay: input.components.map((component) => ({
      exterior: component.exterior.map(({ x, z }) => ({ x, z })),
      interiorRings: component.interiorRings.map((ring) =>
        ring.map(({ x, z }) => ({ x, z }))
      )
    }))
  };
}

function measureRasterizationFidelity(
  mask: boolean[][],
  components: LocalComponent[],
  geometry: BuildingGeometry,
  blocksPerMeter: number
) {
  let referenceArea = 0;
  let voxelArea = 0;
  let intersection = 0;
  const samples = 4;
  for (let z = 0; z < mask.length; z += 1) {
    for (let x = 0; x < mask[z].length; x += 1) {
      let insideSamples = 0;
      for (let sampleZ = 0; sampleZ < samples; sampleZ += 1) {
        for (let sampleX = 0; sampleX < samples; sampleX += 1) {
          if (pointInComponents({
            x: x + (sampleX + 0.5) / samples,
            z: z + (sampleZ + 0.5) / samples
          }, components)) insideSamples += 1;
        }
      }
      const fraction = insideSamples / (samples * samples);
      referenceArea += fraction;
      if (mask[z][x]) {
        voxelArea += 1;
        intersection += fraction;
      }
    }
  }
  const union = referenceArea + voxelArea - intersection;
  const voxelDimensions = orientedMaskDimensions(mask, geometry.orientationDegrees, blocksPerMeter);
  return {
    footprintIoU: union ? intersection / union : 0,
    areaErrorPercent: referenceArea ? Math.abs(voxelArea - referenceArea) / referenceArea * 100 : 100,
    widthErrorMeters: Math.abs(voxelDimensions.widthMeters - geometry.scale.widthMeters),
    lengthErrorMeters: Math.abs(voxelDimensions.lengthMeters - geometry.scale.lengthMeters),
    orientationErrorDegrees: 0
  };
}

function orientedMaskDimensions(mask: boolean[][], orientationDegrees: number, blocksPerMeter: number) {
  const angle = -orientationDegrees * Math.PI / 180;
  const rotatedCorners: LocalPoint[] = [];
  for (let z = 0; z < mask.length; z += 1) {
    for (let x = 0; x < mask[z].length; x += 1) {
      if (!mask[z][x]) continue;
      for (const corner of [
        { x, z },
        { x: x + 1, z },
        { x, z: z + 1 },
        { x: x + 1, z: z + 1 }
      ]) {
        rotatedCorners.push({
          x: corner.x * Math.cos(angle) - corner.z * Math.sin(angle),
          z: corner.x * Math.sin(angle) + corner.z * Math.cos(angle)
        });
      }
    }
  }
  const bounds = localBounds(rotatedCorners);
  const spans = [
    (bounds.maxX - bounds.minX) / blocksPerMeter,
    (bounds.maxZ - bounds.minZ) / blocksPerMeter
  ].sort((left, right) => left - right);
  return { widthMeters: spans[0], lengthMeters: spans[1] };
}

function maskBounds(mask: boolean[][]) {
  const cells: Array<{ x: number; z: number }> = [];
  for (let z = 0; z < mask.length; z += 1) {
    for (let x = 0; x < mask[z].length; x += 1) if (mask[z][x]) cells.push({ x, z });
  }
  const xs = cells.map((cell) => cell.x);
  const zs = cells.map((cell) => cell.z);
  return {
    width: Math.max(...xs) - Math.min(...xs) + 1,
    length: Math.max(...zs) - Math.min(...zs) + 1
  };
}

export function buildingGeometryProvenance(geometry: BuildingGeometry) {
  return {
    sourcePriority: [...geometry.provenance.sourcePriority],
    usedSources: [...geometry.provenance.usedSources],
    missingFields: [...geometry.provenance.missingFields],
    notes: [...geometry.provenance.notes],
    locationAnchor: geometry.target.locationAnchor
      ? structuredClone(geometry.target.locationAnchor)
      : undefined,
    buildingParts: geometry.buildingParts?.map((part) => structuredClone(part)),
    handoff: geometry.provenance.handoff ? { ...geometry.provenance.handoff } : null,
    sourceRecords: geometry.provenance.sourceRecords.map((record) => structuredClone(record)),
    observations: geometry.provenance.observations.map((observation) => structuredClone(observation)),
    identityResolution: structuredClone(geometry.provenance.identityResolution),
    observationReviews: { ...geometry.provenance.observationReviews },
    fieldDecisions: geometry.provenance.fieldDecisions.map((decision) => structuredClone(decision)),
    contradictions: geometry.provenance.contradictions.map((contradiction) => structuredClone(contradiction)),
    arnisRuleDecisions: geometry.provenance.arnisRuleDecisions.map((decision) => structuredClone(decision)),
    generationAssumptions: geometry.provenance.generationAssumptions.map((assumption) => structuredClone(assumption)),
    corrections: geometry.provenance.corrections.map((correction) => structuredClone(correction)),
    geometryValidation: structuredClone(geometry.validation),
    blockReplacements: []
  };
}

function isBoundaryCell(mask: boolean[][], x: number, z: number) {
  return neighborDirections.some(([dx, dz]) => !mask[z + dz]?.[x + dx]);
}

function buildBoundaryDistance(mask: boolean[][], width: number, length: number) {
  const distances = Array.from({ length }, () => Array.from({ length: width }, () => 0));
  for (let z = 0; z < length; z += 1) {
    for (let x = 0; x < width; x += 1) {
      if (mask[z][x]) distances[z][x] = distanceToOutside(mask, x, z, Math.max(width, length));
    }
  }
  return distances;
}

function distanceToOutside(mask: boolean[][], startX: number, startZ: number, maxDistance: number) {
  for (let distance = 1; distance <= maxDistance; distance += 1) {
    for (let dz = -distance; dz <= distance; dz += 1) {
      for (let dx = -distance; dx <= distance; dx += 1) {
        if (Math.abs(dx) + Math.abs(dz) === distance && !mask[startZ + dz]?.[startX + dx]) return distance;
      }
    }
  }
  return maxDistance;
}

function hasNonRectangularFootprint(mask: boolean[][]) {
  const filledCells = mask.flat().filter(Boolean).length;
  const bounds = maskBounds(mask);
  return filledCells < bounds.width * bounds.length * 0.92;
}

function safeSchematicName(name: string) {
  return name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "");
}

function cellKey(x: number, z: number) {
  return `${x}:${z}`;
}

const neighborDirections = [[1, 0], [-1, 0], [0, 1], [0, -1]] as const;

export const schematicPalette = ["minecraft:air", ...Object.values(DEFAULT_BUILDING_MATERIAL_STYLE)] as MinecraftBlockName[];
export const schematicPaletteIndexes = {
  AIR,
  FOUNDATION,
  WALL,
  GLASS,
  ROOF,
  FLOOR,
  ENTRANCE,
  ACCENT
} as const;
