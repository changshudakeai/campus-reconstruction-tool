import type { SemanticFeatureAnnotation, SemanticFeaturePreservationRecord } from "../domain/semanticFeature";
import type { MinecraftBlockName, SchematicModel } from "../domain/schematicModel";
import { blockIndex, cloneSchematicProvenance } from "../domain/schematicModel";

export function applySemanticFeatureAnnotations(
  model: SchematicModel,
  annotations: SemanticFeatureAnnotation[],
  appliedAt = new Date().toISOString()
): SchematicModel {
  if (!annotations.length) return model;
  const provenance = cloneSchematicProvenance(model.metadata.provenance);
  if (!provenance) throw new Error("Semantic feature preservation requires schematic provenance.");

  const next: SchematicModel = {
    ...model,
    blockData: new Uint16Array(model.blockData),
    metadata: {
      ...model.metadata,
      generationReport: model.metadata.generationReport ? structuredClone(model.metadata.generationReport) : null,
      provenance
    }
  };

  const records: SemanticFeaturePreservationRecord[] = [];
  for (const annotation of annotations) {
    validateAnnotation(annotation);
    const block = blockForAnnotation(next, annotation);
    const paletteIndex = ensurePaletteBlock(next, block);
    const cells = targetCells(next, annotation);
    let affectedBlocks = 0;
    for (const cell of cells) {
      const index = blockIndex(cell.x, cell.y, cell.z, next.width, next.length);
      if (next.blockData[index] !== paletteIndex) {
        next.blockData[index] = paletteIndex;
        affectedBlocks += 1;
      }
    }
    records.push({ annotation: structuredClone(annotation), appliedAt, affectedBlocks, block, envelopeChanged: false });
  }

  next.metadata.provenance!.semanticFeatures = [
    ...(next.metadata.provenance!.semanticFeatures ?? []),
    ...records
  ];
  next.metadata.provenance!.notes.push(
    `Applied ${records.length} semantic feature preservation annotation(s).`
  );
  return next;
}

function validateAnnotation(annotation: SemanticFeatureAnnotation) {
  if (!annotation.id.trim()) throw new Error("Semantic feature annotation requires an id.");
  if (!annotation.label.trim()) throw new Error("Semantic feature annotation requires a label.");
  if (!annotation.reason.trim()) throw new Error("Semantic feature annotation requires a reason.");
}

function blockForAnnotation(model: SchematicModel, annotation: SemanticFeatureAnnotation): MinecraftBlockName {
  const preferred: MinecraftBlockName[] = annotation.kind === "window_band"
    ? ["minecraft:glass"]
    : annotation.kind === "entrance_emphasis"
      ? ["minecraft:dark_oak_door", "minecraft:polished_andesite"]
      : annotation.kind === "roof_ridge"
        ? [model.metadata.roofShape === "flat" ? "minecraft:polished_andesite" : "minecraft:dark_oak_slab"]
        : ["minecraft:polished_andesite"];
  return preferred.find((block) => model.palette.includes(block)) ?? preferred[0];
}

function ensurePaletteBlock(model: SchematicModel, block: MinecraftBlockName) {
  const existing = model.palette.indexOf(block);
  if (existing >= 0) return existing;
  model.palette = [...model.palette, block];
  return model.palette.length - 1;
}

function targetCells(model: SchematicModel, annotation: SemanticFeatureAnnotation) {
  const bounds = occupiedBounds(model);
  const width = annotation.strength === "strong" ? 7 : annotation.strength === "visible" ? 5 : 3;
  const y = heightBandY(model, annotation.heightBand, bounds);
  if (annotation.kind === "roof_ridge") {
    const z = Math.round((bounds.minZ + bounds.maxZ) / 2);
    return range(bounds.minX, bounds.maxX).map((x) => ({ x, y: bounds.maxY, z }));
  }
  if (annotation.side === "east" || annotation.side === "west") {
    const x = annotation.side === "east" ? bounds.maxX : bounds.minX;
    const centerZ = Math.round((bounds.minZ + bounds.maxZ) / 2);
    return range(centerZ - Math.floor(width / 2), centerZ + Math.floor(width / 2))
      .filter((z) => z >= bounds.minZ && z <= bounds.maxZ)
      .flatMap((z) => verticalCells(x, z, y, annotation));
  }
  const z = annotation.side === "north" ? bounds.minZ : annotation.side === "south" ? bounds.maxZ : Math.round((bounds.minZ + bounds.maxZ) / 2);
  const centerX = Math.round((bounds.minX + bounds.maxX) / 2);
  return range(centerX - Math.floor(width / 2), centerX + Math.floor(width / 2))
    .filter((x) => x >= bounds.minX && x <= bounds.maxX)
    .flatMap((x) => verticalCells(x, z, y, annotation));
}

function verticalCells(x: number, z: number, y: number, annotation: SemanticFeatureAnnotation) {
  if (annotation.kind === "entrance_emphasis") {
    return [0, 1, 2].map((offset) => ({ x, y: Math.max(1, y + offset), z }));
  }
  if (annotation.kind === "window_band") {
    return [-1, 0, 1].map((offset) => ({ x, y: Math.max(1, y + offset), z }));
  }
  return [{ x, y, z }];
}

function heightBandY(model: SchematicModel, band: SemanticFeatureAnnotation["heightBand"], bounds: ReturnType<typeof occupiedBounds>) {
  if (band === "roof") return bounds.maxY;
  if (band === "lower") return Math.max(1, Math.round(bounds.maxY * 0.25));
  if (band === "upper") return Math.max(1, Math.round(bounds.maxY * 0.72));
  return Math.max(1, Math.round(bounds.maxY * 0.5));
}

function occupiedBounds(model: SchematicModel) {
  const bounds = {
    minX: model.width,
    maxX: 0,
    minY: model.height,
    maxY: 0,
    minZ: model.length,
    maxZ: 0
  };
  for (let y = 0; y < model.height; y += 1) {
    for (let z = 0; z < model.length; z += 1) {
      for (let x = 0; x < model.width; x += 1) {
        if (model.blockData[blockIndex(x, y, z, model.width, model.length)] === 0) continue;
        bounds.minX = Math.min(bounds.minX, x);
        bounds.maxX = Math.max(bounds.maxX, x);
        bounds.minY = Math.min(bounds.minY, y);
        bounds.maxY = Math.max(bounds.maxY, y);
        bounds.minZ = Math.min(bounds.minZ, z);
        bounds.maxZ = Math.max(bounds.maxZ, z);
      }
    }
  }
  return bounds;
}

function range(start: number, end: number) {
  return Array.from({ length: Math.max(0, end - start + 1) }, (_, index) => start + index);
}
