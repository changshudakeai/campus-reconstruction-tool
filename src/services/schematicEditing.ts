import {
  blockIndex,
  cloneSchematicProvenance,
  type BlockReplacementRecord,
  type MinecraftBlockName,
  type SchematicModel
} from "../domain/schematicModel";

export interface BlockInspection {
  x: number;
  y: number;
  z: number;
  paletteIndex: number;
  block: MinecraftBlockName;
}

export interface BatchReplacementResult {
  model: SchematicModel;
  replacedCount: number;
  sourceBlock: MinecraftBlockName;
  replacementBlock: MinecraftBlockName;
}

export function inspectBlock(
  model: SchematicModel,
  x: number,
  y: number,
  z: number
): BlockInspection | null {
  if (
    !Number.isInteger(x) ||
    !Number.isInteger(y) ||
    !Number.isInteger(z) ||
    x < 0 ||
    y < 0 ||
    z < 0 ||
    x >= model.width ||
    y >= model.height ||
    z >= model.length
  ) {
    return null;
  }

  const paletteIndex = model.blockData[blockIndex(x, y, z, model.width, model.length)];
  const block = model.palette[paletteIndex];
  if (!block) return null;

  return {
    x,
    y,
    z,
    paletteIndex,
    block
  };
}

export function replaceAllMatchingBlocks(
  model: SchematicModel,
  sourceBlock: MinecraftBlockName,
  replacementBlock: MinecraftBlockName
): BatchReplacementResult {
  if (sourceBlock === "minecraft:air" || sourceBlock === replacementBlock) {
    return {
      model: cloneSchematicModel(model),
      replacedCount: 0,
      sourceBlock,
      replacementBlock
    };
  }

  const sourcePaletteIndex = model.palette.indexOf(sourceBlock);
  if (sourcePaletteIndex < 0) {
    return {
      model: cloneSchematicModel(model),
      replacedCount: 0,
      sourceBlock,
      replacementBlock
    };
  }

  const palette = [...model.palette];
  let replacementPaletteIndex = palette.indexOf(replacementBlock);
  if (replacementPaletteIndex < 0) {
    palette.push(replacementBlock);
    replacementPaletteIndex = palette.length - 1;
  }

  const blockData = new Uint16Array(model.blockData);
  let replacedCount = 0;
  for (let index = 0; index < blockData.length; index += 1) {
    if (blockData[index] !== sourcePaletteIndex) continue;
    blockData[index] = replacementPaletteIndex;
    replacedCount += 1;
  }

  return {
    model: {
      ...model,
      palette,
      blockData,
      metadata: {
        ...model.metadata,
        provenance: appendReplacementRecord(model, {
          sourceBlock,
          replacementBlock,
          replacedCount
        })
      }
    },
    replacedCount,
    sourceBlock,
    replacementBlock
  };
}

export function countMatchingBlocks(
  model: SchematicModel,
  block: MinecraftBlockName
): number {
  if (block === "minecraft:air") return 0;
  const paletteIndex = model.palette.indexOf(block);
  if (paletteIndex < 0) return 0;

  let count = 0;
  for (const current of model.blockData) {
    if (current === paletteIndex) count += 1;
  }
  return count;
}

export function cloneSchematicModel(model: SchematicModel): SchematicModel {
  return {
    ...model,
    palette: [...model.palette],
    blockData: new Uint16Array(model.blockData),
    metadata: {
      ...model.metadata,
      generationReport: model.metadata.generationReport
        ? structuredClone(model.metadata.generationReport)
        : null,
      provenance: cloneSchematicProvenance(model.metadata.provenance)
    }
  };
}

function appendReplacementRecord(
  model: SchematicModel,
  record: BlockReplacementRecord
) {
  const provenance = cloneSchematicProvenance(model.metadata.provenance);
  if (!provenance || record.replacedCount === 0) return provenance;
  provenance.blockReplacements.push({
    sourceBlock: record.sourceBlock,
    replacementBlock: record.replacementBlock,
    replacedCount: record.replacedCount
  });
  provenance.notes.push(
    `Batch replacement: ${record.replacedCount} ${record.sourceBlock} -> ${record.replacementBlock}.`
  );
  return provenance;
}

export function listInspectableBlocks(model: SchematicModel): BlockInspection[] {
  const blocks: BlockInspection[] = [];
  for (let y = 0; y < model.height; y += 1) {
    for (let z = 0; z < model.length; z += 1) {
      for (let x = 0; x < model.width; x += 1) {
        const block = inspectBlock(model, x, y, z);
        if (block && block.block !== "minecraft:air") blocks.push(block);
      }
    }
  }
  return blocks;
}
