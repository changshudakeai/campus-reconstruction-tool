import type { FoundationManifest, MapFeature, MapFeatureKind } from "../domain/foundationManifest";
import type { CandidatePoint } from "../domain/mapCandidate";
import { DEFAULT_ARNIS_FOUNDATION_STYLE_PACK, stylePackBlocks, type FoundationStylePack } from "./foundationFeatureGenerators";
import {
  createEmptySchematicModel,
  type MinecraftBlockName,
  type SchematicModel,
  setBlock
} from "../domain/schematicModel";

interface LocalPoint {
  x: number;
  z: number;
}

export interface FoundationSchematicOptions {
  blocksPerMeter?: number;
  paddingBlocks?: number;
  roadWidthBlocks?: number;
  orientationDegrees?: number;
  stylePack?: FoundationStylePack;
  featureHeights?: Partial<Record<MapFeatureKind, number>>;
}

export interface FoundationSchematicExport {
  model: SchematicModel;
  featureCount: number;
  paletteByBlock: Map<MinecraftBlockName, number>;
}

export type FoundationExportSizeRisk = "ready" | "large" | "very_large";

export interface FoundationSchematicPreview {
  width: number;
  height: number;
  length: number;
  totalBlocks: number;
  estimatedNonAirBlocks: number;
  reviewedFeatureCount: number;
  paletteSize: number;
  risk: FoundationExportSizeRisk;
}

interface ProjectedFeature {
  feature: MapFeature;
  points: LocalPoint[];
}

interface FoundationSchematicLayout {
  mergedOptions: Required<FoundationSchematicOptions>;
  features: MapFeature[];
  shiftedFeatures: ProjectedFeature[];
  width: number;
  height: number;
  length: number;
  palette: MinecraftBlockName[];
}

const AIR: MinecraftBlockName = "minecraft:air";
const DEFAULT_OPTIONS: Required<FoundationSchematicOptions> = {
  blocksPerMeter: 0.45,
  paddingBlocks: 8,
  roadWidthBlocks: 4,
  orientationDegrees: 0,
  stylePack: DEFAULT_ARNIS_FOUNDATION_STYLE_PACK,
  featureHeights: {
    campus: 1,
    building: 3,
    road: 1,
    vegetation: 1,
    water: 1,
    sports: 1
  }
};

const DEFAULT_BLOCKS: Record<MapFeatureKind, MinecraftBlockName> = {
  campus: "minecraft:grass_block",
  building: "minecraft:quartz_block",
  road: "minecraft:gray_concrete",
  vegetation: "minecraft:moss_block",
  water: "minecraft:water",
  sports: "minecraft:orange_concrete"
};

export function generateFoundationSchematicFromManifest(
  manifest: FoundationManifest,
  options: FoundationSchematicOptions = {}
): FoundationSchematicExport {
  const layout = prepareFoundationSchematicLayout(manifest, options);
  const paletteByBlock = new Map(layout.palette.map((block, index) => [block, index]));
  const model = createEmptySchematicModel({
    name: "ecnu_putuo_foundation",
    width: layout.width,
    height: layout.height,
    length: layout.length,
    palette: layout.palette,
    generator: "foundation-manifest-to-schematic",
    sourceBuilding: manifest.target.campus,
    nonRectangularFootprint: true,
    roofShape: null
  });

  for (const { feature, points } of layout.shiftedFeatures) {
    const block = normalizeMinecraftBlock(feature.block, feature.kind);
    const paletteIndex = paletteByBlock.get(block) ?? 0;
    const featureHeight = layout.mergedOptions.featureHeights[feature.kind] ?? 1;

    const generatorStyle = layout.mergedOptions.stylePack.features[feature.kind];
    if (feature.kind === "road" && feature.geometry.type === "polyline" && generatorStyle?.generator === "arnis:road/v1") {
      const edgeBlock = normalizeMinecraftBlock(generatorStyle.blocks[1] ?? feature.block, feature.kind);
      const edgeIndex = paletteByBlock.get(edgeBlock) ?? paletteIndex;
      const width = generatorStyle.width ?? layout.mergedOptions.roadWidthBlocks;
      rasterizePolyline(model, points, edgeIndex, Math.max(1, width + 2), featureHeight);
      rasterizePolyline(model, points, paletteIndex, Math.max(1, width), featureHeight);
    } else if (feature.geometry.type === "polygon" && (generatorStyle?.generator === "arnis:water/v1" || generatorStyle?.generator === "arnis:sports/v1")) {
      const borderBlock = normalizeMinecraftBlock(generatorStyle.blocks[1] ?? feature.block, feature.kind);
      const borderIndex = paletteByBlock.get(borderBlock) ?? paletteIndex;
      rasterizePolygonWithBorder(model, points, paletteIndex, borderIndex, featureHeight);
    } else if (feature.geometry.type === "polygon") {
      rasterizePolygon(model, points, paletteIndex, featureHeight);
      if (feature.kind === "vegetation" && generatorStyle?.generator === "arnis:vegetation/v1") {
        const logIndex = paletteByBlock.get(normalizeMinecraftBlock(generatorStyle.blocks[1] ?? "oak_log", feature.kind)) ?? paletteIndex;
        const leavesIndex = paletteByBlock.get(normalizeMinecraftBlock(generatorStyle.blocks[2] ?? "oak_leaves", feature.kind)) ?? paletteIndex;
        rasterizeVegetationTrees(model, points, logIndex, leavesIndex, generatorStyle.density ?? 0.035, generatorStyle.seed ?? 1);
      }
    } else if (feature.geometry.type === "polyline") {
      rasterizePolyline(
        model,
        points,
        paletteIndex,
        Math.max(1, layout.mergedOptions.roadWidthBlocks),
        featureHeight
      );
    } else {
      rasterizePoint(model, points[0], paletteIndex, featureHeight);
    }
  }

  return {
    model,
    featureCount: layout.features.length,
    paletteByBlock
  };
}

export function previewFoundationSchematicExport(
  manifest: FoundationManifest,
  options: FoundationSchematicOptions = {}
): FoundationSchematicPreview {
  const layout = prepareFoundationSchematicLayout(manifest, options);
  const totalBlocks = layout.width * layout.height * layout.length;
  const estimatedNonAirBlocks = estimateNonAirBlocks(layout);

  return {
    width: layout.width,
    height: layout.height,
    length: layout.length,
    totalBlocks,
    estimatedNonAirBlocks,
    reviewedFeatureCount: layout.features.length,
    paletteSize: layout.palette.length,
    risk: sizeRisk(totalBlocks)
  };
}

export function previewGeneratedFoundationSchematic(
  generated: FoundationSchematicExport
): FoundationSchematicPreview {
  const { model, featureCount } = generated;
  const totalBlocks = model.blockData.length;
  return {
    width: model.width,
    height: model.height,
    length: model.length,
    totalBlocks,
    estimatedNonAirBlocks: model.blockData.reduce(
      (count, paletteIndex) => count + (paletteIndex === 0 ? 0 : 1),
      0
    ),
    reviewedFeatureCount: featureCount,
    paletteSize: model.palette.length,
    risk: sizeRisk(totalBlocks)
  };
}

function mergeOptions(manifest: FoundationManifest, options: FoundationSchematicOptions): Required<FoundationSchematicOptions> {
  return {
    ...DEFAULT_OPTIONS,
    blocksPerMeter: manifest.target.blocksPerMeter ?? DEFAULT_OPTIONS.blocksPerMeter,
    orientationDegrees: manifest.target.orientationDegrees ?? DEFAULT_OPTIONS.orientationDegrees,
    ...options,
    featureHeights: {
      ...DEFAULT_OPTIONS.featureHeights,
      ...(options.featureHeights ?? {})
    }
  };
}

function prepareFoundationSchematicLayout(
  manifest: FoundationManifest,
  options: FoundationSchematicOptions
): FoundationSchematicLayout {
  const mergedOptions = mergeOptions(manifest, options);
  const features = manifest.mapFeatures.filter((feature) => feature.reviewed);
  const allPoints = features.flatMap((feature) => feature.geometry.points);
  if (allPoints.length === 0) {
    throw new Error("Foundation Manifest has no reviewed geometry to export.");
  }

  const origin = allPoints[0];
  const projectedFeatures = features.map((feature) => ({
    feature,
    points: feature.geometry.points.map((point) => rotateLocalPoint(
      lngLatToLocal(point, origin, mergedOptions.blocksPerMeter),
      mergedOptions.orientationDegrees
    ))
  }));
  const bounds = localBounds(projectedFeatures.flatMap((entry) => entry.points));
  const width = Math.max(
    8,
    Math.ceil(bounds.maxX - bounds.minX) + mergedOptions.paddingBlocks * 2 + 1
  );
  const length = Math.max(
    8,
    Math.ceil(bounds.maxZ - bounds.minZ) + mergedOptions.paddingBlocks * 2 + 1
  );
  const hasGeneratedTrees = features.some((feature) => feature.kind === "vegetation") && mergedOptions.stylePack.features.vegetation?.generator === "arnis:vegetation/v1";
  const height = Math.max(hasGeneratedTrees ? 8 : 0, Math.max(...features.map((feature) => mergedOptions.featureHeights[feature.kind] ?? 1)) + 2);
  const shiftedFeatures = projectedFeatures.map((entry) => ({
    feature: entry.feature,
    points: entry.points.map((point) => ({
      x: point.x - bounds.minX + mergedOptions.paddingBlocks,
      z: point.z - bounds.minZ + mergedOptions.paddingBlocks
    }))
  }));

  return {
    mergedOptions,
    features,
    shiftedFeatures,
    width,
    height,
    length,
    palette: makePalette(features, mergedOptions.stylePack)
  };
}

function makePalette(features: MapFeature[], stylePack: FoundationStylePack): MinecraftBlockName[] {
  return Array.from(
    new Set<MinecraftBlockName>([
      AIR,
      ...features.map((feature) => normalizeMinecraftBlock(feature.block, feature.kind)),
      ...stylePackBlocks(stylePack) as MinecraftBlockName[]
    ])
  );
}

function rasterizeVegetationTrees(model: SchematicModel, polygon: LocalPoint[], logIndex: number, leavesIndex: number, density: number, seed: number) {
  const bounds = localBounds(polygon);
  const step = Math.max(4, Math.round(1 / Math.sqrt(Math.max(0.005, density))));
  for (let z = Math.ceil(bounds.minZ) + (seed % step); z <= Math.floor(bounds.maxZ); z += step) {
    for (let x = Math.ceil(bounds.minX) + ((seed >> 3) % step); x <= Math.floor(bounds.maxX); x += step) {
      if (!pointInPolygon({ x: x + .5, z: z + .5 }, polygon)) continue;
      for (let y = 1; y <= 4; y += 1) setBlock(model, x, y, z, logIndex);
      for (let dz = -2; dz <= 2; dz += 1) for (let dx = -2; dx <= 2; dx += 1) for (let y = 4; y <= 6; y += 1) {
        if (Math.abs(dx) + Math.abs(dz) + Math.abs(y - 5) <= 4) setBlock(model, x + dx, y, z + dz, leavesIndex);
      }
    }
  }
}

function rasterizePolygonWithBorder(model: SchematicModel, polygon: LocalPoint[], fillIndex: number, borderIndex: number, height: number) {
  const bounds = localBounds(polygon);
  for (let z = Math.max(0, Math.floor(bounds.minZ)); z <= Math.min(model.length - 1, Math.ceil(bounds.maxZ)); z += 1) {
    for (let x = Math.max(0, Math.floor(bounds.minX)); x <= Math.min(model.width - 1, Math.ceil(bounds.maxX)); x += 1) {
      if (!pointInPolygon({ x: x + .5, z: z + .5 }, polygon)) continue;
      const border = [[1,0],[-1,0],[0,1],[0,-1]].some(([dx,dz]) => !pointInPolygon({ x: x + .5 + dx, z: z + .5 + dz }, polygon));
      for (let y = 0; y < height; y += 1) setBlock(model, x, y, z, border ? borderIndex : fillIndex);
    }
  }
}

function normalizeMinecraftBlock(block: string, kind: MapFeatureKind): MinecraftBlockName {
  if (block.includes(":")) return block as MinecraftBlockName;
  return `minecraft:${block || DEFAULT_BLOCKS[kind].replace("minecraft:", "")}`;
}

function sizeRisk(totalBlocks: number): FoundationExportSizeRisk {
  if (totalBlocks >= 5_000_000) return "very_large";
  if (totalBlocks >= 1_000_000) return "large";
  return "ready";
}

function estimateNonAirBlocks(layout: FoundationSchematicLayout) {
  const filledBlocks = new Set<number>();

  for (const { feature, points } of layout.shiftedFeatures) {
    const featureHeight = layout.mergedOptions.featureHeights[feature.kind] ?? 1;

    if (feature.geometry.type === "polygon") {
      estimatePolygonBlocks(layout, filledBlocks, points, featureHeight);
    } else if (feature.geometry.type === "polyline") {
      estimatePolylineBlocks(
        layout,
        filledBlocks,
        points,
        Math.max(1, layout.mergedOptions.roadWidthBlocks),
        featureHeight
      );
    } else {
      estimatePointBlocks(layout, filledBlocks, points[0], featureHeight);
    }
  }

  return filledBlocks.size;
}

function estimatePolygonBlocks(
  layout: FoundationSchematicLayout,
  filledBlocks: Set<number>,
  polygon: LocalPoint[],
  featureHeight: number
) {
  const bounds = localBounds(polygon);
  const minX = Math.max(0, Math.floor(bounds.minX));
  const maxX = Math.min(layout.width - 1, Math.ceil(bounds.maxX));
  const minZ = Math.max(0, Math.floor(bounds.minZ));
  const maxZ = Math.min(layout.length - 1, Math.ceil(bounds.maxZ));

  for (let z = minZ; z <= maxZ; z += 1) {
    for (let x = minX; x <= maxX; x += 1) {
      if (!pointInPolygon({ x: x + 0.5, z: z + 0.5 }, polygon)) continue;
      estimateColumn(layout, filledBlocks, x, z, featureHeight);
    }
  }
}

function estimatePolylineBlocks(
  layout: FoundationSchematicLayout,
  filledBlocks: Set<number>,
  points: LocalPoint[],
  width: number,
  featureHeight: number
) {
  for (let index = 0; index < points.length - 1; index += 1) {
    estimateSegmentBlocks(
      layout,
      filledBlocks,
      points[index],
      points[index + 1],
      width,
      featureHeight
    );
  }
}

function estimateSegmentBlocks(
  layout: FoundationSchematicLayout,
  filledBlocks: Set<number>,
  start: LocalPoint,
  end: LocalPoint,
  width: number,
  featureHeight: number
) {
  const dx = end.x - start.x;
  const dz = end.z - start.z;
  const steps = Math.max(1, Math.ceil(Math.hypot(dx, dz) * 2));

  for (let step = 0; step <= steps; step += 1) {
    const t = step / steps;
    estimatePointBlocks(
      layout,
      filledBlocks,
      {
        x: start.x + dx * t,
        z: start.z + dz * t
      },
      featureHeight,
      Math.ceil(width / 2)
    );
  }
}

function estimatePointBlocks(
  layout: FoundationSchematicLayout,
  filledBlocks: Set<number>,
  point: LocalPoint,
  featureHeight: number,
  radius = 1
) {
  const centerX = Math.round(point.x);
  const centerZ = Math.round(point.z);

  for (let z = centerZ - radius; z <= centerZ + radius; z += 1) {
    for (let x = centerX - radius; x <= centerX + radius; x += 1) {
      if (Math.hypot(x - centerX, z - centerZ) > radius + 0.25) continue;
      estimateColumn(layout, filledBlocks, x, z, featureHeight);
    }
  }
}

function estimateColumn(
  layout: FoundationSchematicLayout,
  filledBlocks: Set<number>,
  x: number,
  z: number,
  featureHeight: number
) {
  if (x < 0 || z < 0 || x >= layout.width || z >= layout.length) return;

  for (let y = 0; y < featureHeight; y += 1) {
    filledBlocks.add(y * layout.width * layout.length + z * layout.width + x);
  }
}

function lngLatToLocal(
  point: CandidatePoint,
  origin: CandidatePoint,
  blocksPerMeter: number
): LocalPoint {
  const latRadians = (origin.lat * Math.PI) / 180;
  const metersPerDegreeLat = 111_320;
  const metersPerDegreeLng = 111_320 * Math.cos(latRadians);

  return {
    x: (point.lng - origin.lng) * metersPerDegreeLng * blocksPerMeter,
    z: (point.lat - origin.lat) * metersPerDegreeLat * blocksPerMeter
  };
}

function rotateLocalPoint(point: LocalPoint, degrees: number): LocalPoint {
  if (!degrees) return point;
  const radians = degrees * Math.PI / 180;
  const cos = Math.cos(radians), sin = Math.sin(radians);
  return { x: point.x * cos - point.z * sin, z: point.x * sin + point.z * cos };
}

function localBounds(points: LocalPoint[]) {
  return points.reduce(
    (bounds, point) => ({
      minX: Math.min(bounds.minX, point.x),
      maxX: Math.max(bounds.maxX, point.x),
      minZ: Math.min(bounds.minZ, point.z),
      maxZ: Math.max(bounds.maxZ, point.z)
    }),
    {
      minX: Number.POSITIVE_INFINITY,
      maxX: Number.NEGATIVE_INFINITY,
      minZ: Number.POSITIVE_INFINITY,
      maxZ: Number.NEGATIVE_INFINITY
    }
  );
}

function rasterizePolygon(
  model: SchematicModel,
  polygon: LocalPoint[],
  paletteIndex: number,
  featureHeight: number
) {
  const bounds = localBounds(polygon);
  const minX = Math.max(0, Math.floor(bounds.minX));
  const maxX = Math.min(model.width - 1, Math.ceil(bounds.maxX));
  const minZ = Math.max(0, Math.floor(bounds.minZ));
  const maxZ = Math.min(model.length - 1, Math.ceil(bounds.maxZ));

  for (let z = minZ; z <= maxZ; z += 1) {
    for (let x = minX; x <= maxX; x += 1) {
      if (!pointInPolygon({ x: x + 0.5, z: z + 0.5 }, polygon)) continue;
      fillColumn(model, x, z, paletteIndex, featureHeight);
    }
  }
}

function rasterizePolyline(
  model: SchematicModel,
  points: LocalPoint[],
  paletteIndex: number,
  width: number,
  featureHeight: number
) {
  for (let index = 0; index < points.length - 1; index += 1) {
    rasterizeSegment(model, points[index], points[index + 1], paletteIndex, width, featureHeight);
  }
}

function rasterizeSegment(
  model: SchematicModel,
  start: LocalPoint,
  end: LocalPoint,
  paletteIndex: number,
  width: number,
  featureHeight: number
) {
  const dx = end.x - start.x;
  const dz = end.z - start.z;
  const steps = Math.max(1, Math.ceil(Math.hypot(dx, dz) * 2));

  for (let step = 0; step <= steps; step += 1) {
    const t = step / steps;
    const x = start.x + dx * t;
    const z = start.z + dz * t;
    rasterizePoint(model, { x, z }, paletteIndex, featureHeight, Math.ceil(width / 2));
  }
}

function rasterizePoint(
  model: SchematicModel,
  point: LocalPoint,
  paletteIndex: number,
  featureHeight: number,
  radius = 1
) {
  const centerX = Math.round(point.x);
  const centerZ = Math.round(point.z);

  for (let z = centerZ - radius; z <= centerZ + radius; z += 1) {
    for (let x = centerX - radius; x <= centerX + radius; x += 1) {
      if (Math.hypot(x - centerX, z - centerZ) > radius + 0.25) continue;
      fillColumn(model, x, z, paletteIndex, featureHeight);
    }
  }
}

function fillColumn(
  model: SchematicModel,
  x: number,
  z: number,
  paletteIndex: number,
  featureHeight: number
) {
  for (let y = 0; y < featureHeight; y += 1) {
    setBlock(model, x, y, z, paletteIndex);
  }
}

function pointInPolygon(point: LocalPoint, polygon: LocalPoint[]): boolean {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i, i += 1) {
    const pi = polygon[i];
    const pj = polygon[j];
    const crosses =
      pi.z > point.z !== pj.z > point.z &&
      point.x < ((pj.x - pi.x) * (point.z - pi.z)) / (pj.z - pi.z) + pi.x;
    if (crosses) inside = !inside;
  }
  return inside;
}
