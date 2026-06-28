import { invokeDesktop } from "../services/tauriInvoke";
import type { BuildingGeometry, BuildingPartGeometry, BuildingTarget, FootprintComponent } from "../domain/buildingGeometry";
import { createEmptySchematicModel, type MinecraftBlockName, type SchematicModel } from "../domain/schematicModel";
import { mergeBuildingGeometry } from "./minimalArnisAdapter";
import { createBuildingGeometryObservation } from "../services/buildingObservation";
import { buildingGeometryProvenance } from "../services/buildingGeometryToSchematic";

export interface ArnisBuildingCandidate {
  id: string;
  source: "overture" | "osm_overpass";
  name: string | null;
  tags: Record<string, string>;
  components: FootprintComponent[];
  heightM: number | null;
  floors: number | null;
  roofShape: string | null;
  identityConfidence: "high" | "medium" | "low";
  distanceM: number;
  widthM: number;
  lengthM: number;
  parts: BuildingPartGeometry[];
}

export interface ArnisCandidateQueryResult {
  candidates: ArnisBuildingCandidate[];
  warnings: string[];
}

interface RustGeneratedBuilding {
  width: number;
  height: number;
  length: number;
  palette: MinecraftBlockName[];
  blockRuns: Array<{ paletteIndex: number; runLength: number }>;
  report: {
    candidateId: string;
    source: string;
    generator: string;
    blocksPerMeter: number;
    floorCount: number;
    roofShape: string;
    nonAirBlocks: number;
    deterministicSeed: number;
    correctionNotes: string[];
    buildingPartCount: number;
  };
}

export async function queryArnisBuildingCandidates(
  target: BuildingTarget,
  scale = 1
): Promise<ArnisCandidateQueryResult> {
  if (!target.locationAnchor) {
    throw new Error("Confirm a live Gaode building location before querying Arnis.");
  }
  return invokeDesktop("query_building_candidates", {
    request: {
      name: target.name,
      aliases: target.aliases,
      lng: target.approximateCenter.lng,
      lat: target.approximateCenter.lat,
      radiusM: 250,
      scale,
      coordinateSystem: "WGS-84",
      gaodePoiId: target.locationAnchor.gaodePoiId,
      gaodeLng: target.locationAnchor.gcj02.lng,
      gaodeLat: target.locationAnchor.gcj02.lat,
      transformation: target.locationAnchor.transformation
    }
  });
}

export function buildingGeometryFromArnisCandidate(
  target: BuildingTarget,
  candidate: ArnisBuildingCandidate
): BuildingGeometry {
  const observation = createBuildingGeometryObservation({
    id: candidate.id,
    source: candidate.source,
    sourceFeatureId: candidate.id,
    name: candidate.name,
    tags: candidate.tags,
    components: candidate.components,
    normalizationNotes: ["Normalized by the vendored Arnis Rust Core Adapter."]
  });
  const geometry = mergeBuildingGeometry(target, [candidate.source], [{
    source: candidate.source,
    geometry: {
      footprint: candidate.components[0].exterior,
      heightM: candidate.heightM,
      floors: candidate.floors,
      roof: { shape: candidate.roofShape },
      observations: [observation],
      notes: [`Selected live Arnis candidate ${candidate.id}.`]
    }
  }]);
  geometry.buildingParts = candidate.parts;
  geometry.provenance.notes.push(
    `Preserved ${candidate.parts.length} Arnis building part(s) for massing generation.`
  );
  return geometry;
}

export async function generateSchematicWithArnisCore(
  geometry: BuildingGeometry,
  candidateId: string,
  source: string
): Promise<SchematicModel> {
  const generated = await invokeDesktop<RustGeneratedBuilding>("generate_building", {
    request: {
      candidateId,
      source,
      components: geometry.footprintComponents,
      heightM: geometry.heightM,
      floors: geometry.floors,
      roofShape: geometry.roof.shape,
      blocksPerMeter: 1,
      seed: stableSeed(candidateId),
      materials: {},
      correctionNotes: geometry.provenance.corrections.map((record) => record.reason),
      parts: geometry.buildingParts ?? []
    }
  });
  const blockData = decodeBlockRuns(generated);
  const model = createEmptySchematicModel({
    name: "putuo_campus_library",
    width: generated.width,
    height: generated.height,
    length: generated.length,
    palette: generated.palette,
    sourceBuilding: geometry.buildingName,
    nonRectangularFootprint: true,
    roofShape: generated.report.roofShape,
    provenance: buildingGeometryProvenance(geometry)
  });
  model.blockData = blockData;
  model.metadata.provenance?.notes.push(
    `${generated.report.generator}; candidate=${candidateId}; seed=${generated.report.deterministicSeed}; buildingParts=${generated.report.buildingPartCount}`
  );
  return model;
}

export function decodeBlockRuns(generated: Pick<RustGeneratedBuilding, "width" | "height" | "length" | "palette" | "blockRuns">) {
  const expected = generated.width * generated.height * generated.length;
  const data = new Uint16Array(expected);
  let offset = 0;
  for (const run of generated.blockRuns) {
    if (!Number.isInteger(run.runLength) || run.runLength <= 0) throw new Error("Arnis Core returned an invalid RLE run.");
    if (run.paletteIndex < 0 || run.paletteIndex >= generated.palette.length) throw new Error("Arnis Core returned an invalid palette index.");
    if (offset + run.runLength > expected) throw new Error("Arnis Core RLE exceeded the declared dimensions.");
    data.fill(run.paletteIndex, offset, offset + run.runLength);
    offset += run.runLength;
  }
  if (offset !== expected) throw new Error("Arnis Core RLE did not fill the declared dimensions.");
  return data;
}

function stableSeed(value: string) {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}
