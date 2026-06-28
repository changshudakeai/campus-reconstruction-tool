import type { SchematicModel } from "../domain/schematicModel";

const TAG_END = 0;
const TAG_BYTE_ARRAY = 7;
const TAG_STRING = 8;
const TAG_LIST = 9;
const TAG_COMPOUND = 10;
const TAG_INT_ARRAY = 11;
const TAG_SHORT = 2;
const TAG_INT = 3;

export interface SpongeSchematicSummary {
  rootName: "Schematic";
  version: 2;
  dataVersion: number;
  width: number;
  height: number;
  length: number;
  paletteMax: number;
  blockDataBytes: number;
}

export function writeSpongeV2Schematic(model: SchematicModel): Uint8Array {
  const writer = new NbtWriter();

  writer.writeByte(TAG_COMPOUND);
  writer.writeString("Schematic");
  writer.writeNamedInt("Version", 2);
  writer.writeNamedInt("DataVersion", 3955);
  writer.writeNamedShort("Width", model.width);
  writer.writeNamedShort("Height", model.height);
  writer.writeNamedShort("Length", model.length);
  writer.writeNamedIntArray("Offset", [0, 0, 0]);

  writer.writeByte(TAG_COMPOUND);
  writer.writeString("Palette");
  model.palette.forEach((block, index) => writer.writeNamedInt(block, index));
  writer.writeByte(TAG_END);

  const blockData = encodeVarintBlockData(model.blockData);
  writer.writeNamedInt("PaletteMax", model.palette.length);
  writer.writeNamedByteArray("BlockData", blockData);
  writer.writeNamedEmptyList("BlockEntities", TAG_COMPOUND);
  writer.writeNamedEmptyList("Entities", TAG_COMPOUND);

  writer.writeByte(TAG_COMPOUND);
  writer.writeString("Metadata");
  writer.writeNamedString("Name", model.name);
  writer.writeNamedString("Generator", model.metadata.generator);
  writer.writeNamedString("SourceBuilding", model.metadata.sourceBuilding);
  writer.writeNamedString("RoofShape", model.metadata.roofShape ?? "none");
  writeProvenanceMetadata(writer, model);
  writer.writeByte(TAG_END);

  writer.writeByte(TAG_END);
  return writer.toUint8Array();
}

function writeProvenanceMetadata(writer: NbtWriter, model: SchematicModel) {
  const provenance = model.metadata.provenance;
  if (!provenance) return;

  writer.writeNamedString("SourcePriority", safeMetadataString(provenance.sourcePriority.join(",")));
  writer.writeNamedString("UsedSources", safeMetadataString(provenance.usedSources.join(",")));
  writer.writeNamedString("MissingFields", safeMetadataString(provenance.missingFields.join(",")));
  writer.writeNamedString("ProvenanceNotes", safeMetadataString(provenance.notes.join(" | ")));
  writer.writeNamedString(
    "GenerationAssumptions",
    safeMetadataString(JSON.stringify(provenance.generationAssumptions))
  );
  writer.writeNamedString(
    "GeometryCorrections",
    safeMetadataString(JSON.stringify(provenance.corrections))
  );
  writer.writeNamedString(
    "BlockReplacements",
    safeMetadataString(JSON.stringify(provenance.blockReplacements))
  );
  if (provenance.semanticFeatures?.length) {
    writer.writeNamedString(
      "SemanticFeatures",
      safeMetadataString(JSON.stringify(provenance.semanticFeatures))
    );
  }
  if (provenance.sourceConflicts?.length) {
    writer.writeNamedString(
      "SourceConflicts",
      safeMetadataString(JSON.stringify(provenance.sourceConflicts))
    );
  }
  if (provenance.externalModels?.length) {
    writer.writeNamedString(
      "ExternalModels",
      safeMetadataString(JSON.stringify(provenance.externalModels))
    );
  }
  if (provenance.axiomAcceptance) {
    writer.writeNamedString(
      "AxiomAcceptance",
      safeMetadataString(JSON.stringify(provenance.axiomAcceptance))
    );
  }
  if (provenance.handoff) {
    writer.writeNamedString("FoundationSlotId", safeMetadataString(provenance.handoff.foundationSlotId));
    writer.writeNamedString("SourceFeatureId", safeMetadataString(provenance.handoff.sourceFeatureId));
    writer.writeNamedString("SourceRawId", safeMetadataString(provenance.handoff.rawId));
  }
}

function safeMetadataString(value: string) {
  return value.slice(0, 16_000);
}

export function summarizeSpongeV2Schematic(
  model: SchematicModel,
  bytes: Uint8Array
): SpongeSchematicSummary {
  return {
    rootName: "Schematic",
    version: 2,
    dataVersion: 3955,
    width: model.width,
    height: model.height,
    length: model.length,
    paletteMax: model.palette.length,
    blockDataBytes: encodeVarintBlockData(model.blockData).length
  };
}

export function encodeVarintBlockData(blockData: Uint16Array): Uint8Array {
  const bytes: number[] = [];
  for (const paletteIndex of blockData) {
    writeVarint(bytes, paletteIndex);
  }
  return Uint8Array.from(bytes);
}

function writeVarint(bytes: number[], value: number) {
  let remaining = value >>> 0;
  while ((remaining & 0xffffff80) !== 0) {
    bytes.push((remaining & 0x7f) | 0x80);
    remaining >>>= 7;
  }
  bytes.push(remaining & 0x7f);
}

class NbtWriter {
  private readonly bytes: number[] = [];

  writeByte(value: number) {
    this.bytes.push(value & 0xff);
  }

  writeNamedShort(name: string, value: number) {
    this.writeByte(TAG_SHORT);
    this.writeString(name);
    this.writeShort(value);
  }

  writeNamedInt(name: string, value: number) {
    this.writeByte(TAG_INT);
    this.writeString(name);
    this.writeInt(value);
  }

  writeNamedString(name: string, value: string) {
    this.writeByte(TAG_STRING);
    this.writeString(name);
    this.writeString(value);
  }

  writeNamedByteArray(name: string, value: Uint8Array) {
    this.writeByte(TAG_BYTE_ARRAY);
    this.writeString(name);
    this.writeInt(value.length);
    for (const byte of value) this.writeByte(byte);
  }

  writeNamedIntArray(name: string, values: number[]) {
    this.writeByte(TAG_INT_ARRAY);
    this.writeString(name);
    this.writeInt(values.length);
    for (const value of values) this.writeInt(value);
  }

  writeNamedEmptyList(name: string, childType: number) {
    this.writeByte(TAG_LIST);
    this.writeString(name);
    this.writeByte(childType);
    this.writeInt(0);
  }

  writeString(value: string) {
    const encoded = new TextEncoder().encode(value);
    this.writeShort(encoded.length);
    for (const byte of encoded) this.writeByte(byte);
  }

  toUint8Array(): Uint8Array {
    return Uint8Array.from(this.bytes);
  }

  private writeShort(value: number) {
    this.bytes.push((value >> 8) & 0xff, value & 0xff);
  }

  private writeInt(value: number) {
    this.bytes.push(
      (value >> 24) & 0xff,
      (value >> 16) & 0xff,
      (value >> 8) & 0xff,
      value & 0xff
    );
  }
}
