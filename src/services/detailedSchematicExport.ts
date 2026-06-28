import type { SchematicModel } from "../domain/schematicModel";
import { checkAxiomPlacement } from "./axiomAcceptance";
import { gzipBytes } from "./gzip";
import { writeSpongeV2Schematic } from "./spongeSchematic";

export interface DetailedSchematicExport {
  fileName: string;
  provenanceFileName: string;
  provenanceJson: string;
  bytes: Uint8Array;
  width: number;
  height: number;
  length: number;
  paletteSize: number;
  nonAirBlocks: number;
}

export function prepareDetailedSchematicExport(
  model: SchematicModel
): DetailedSchematicExport {
  validateDetailedModel(model);
  const bytes = gzipBytes(writeSpongeV2Schematic(model));
  const provenanceFileName = `${model.name}.provenance.json`;

  return {
    fileName: `${model.name}.schem`,
    provenanceFileName,
    provenanceJson: JSON.stringify(
      {
        schemaVersion: "0.1.0",
        schematic: {
          fileName: `${model.name}.schem`,
          name: model.name,
          generator: model.metadata.generator,
          sourceBuilding: model.metadata.sourceBuilding,
          dimensions: {
            width: model.width,
            height: model.height,
            length: model.length
          },
          palette: model.palette,
          generationReport: model.metadata.generationReport,
          axiomPlacement: checkAxiomPlacement(model),
          axiomAcceptance: model.metadata.provenance?.axiomAcceptance ?? null
        },
        provenance: model.metadata.provenance
      },
      null,
      2
    ),
    bytes,
    width: model.width,
    height: model.height,
    length: model.length,
    paletteSize: model.palette.length,
    nonAirBlocks: countNonAirBlocks(model)
  };
}

function validateDetailedModel(model: SchematicModel) {
  if (model.metadata.generator !== "building-geometry-to-schematic") {
    throw new Error("Detailed export requires a Building Geometry schematic model.");
  }
  if (model.width <= 0 || model.height <= 0 || model.length <= 0) {
    throw new Error("Detailed export requires positive schematic dimensions.");
  }
  if (model.blockData.length !== model.width * model.height * model.length) {
    throw new Error("Detailed export block data does not match schematic dimensions.");
  }
  if (model.palette[0] !== "minecraft:air") {
    throw new Error("Detailed export requires air at palette index zero.");
  }
  if (!model.metadata.provenance) {
    throw new Error("Detailed export requires Building Geometry provenance.");
  }
}

function countNonAirBlocks(model: SchematicModel) {
  let count = 0;
  for (const paletteIndex of model.blockData) {
    if (paletteIndex !== 0) count += 1;
  }
  return count;
}
