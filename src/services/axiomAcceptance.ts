import type {
  PreviewCameraView,
  SchematicModel,
  AxiomAcceptanceRecord,
  AxiomImportResult,
  AxiomPlacementCheck
} from "../domain/schematicModel";
import { cloneSchematicProvenance } from "../domain/schematicModel";

export function checkAxiomPlacement(model: SchematicModel, toleranceBlocks = 4): AxiomPlacementCheck {
  const report = model.metadata.generationReport;
  const handoff = model.metadata.provenance?.handoff ?? null;
  const blocksPerMeter = report?.blocksPerMeter ?? null;
  const orientationDegrees = report?.orientationDegrees ?? null;
  const expectedWidthBlocks = handoff && blocksPerMeter !== null
    ? Math.round(handoff.approximateWidthMeters * blocksPerMeter)
    : null;
  const expectedLengthBlocks = handoff && blocksPerMeter !== null
    ? Math.round(handoff.approximateLengthMeters * blocksPerMeter)
    : null;
  const footprintWidthBlocks = report ? Math.round(report.dimensions.footprintWidthMeters * report.blocksPerMeter) : null;
  const footprintLengthBlocks = report ? Math.round(report.dimensions.footprintLengthMeters * report.blocksPerMeter) : null;
  const widthDeltaBlocks = expectedWidthBlocks !== null && footprintWidthBlocks !== null
    ? footprintWidthBlocks - expectedWidthBlocks
    : null;
  const lengthDeltaBlocks = expectedLengthBlocks !== null && footprintLengthBlocks !== null
    ? footprintLengthBlocks - expectedLengthBlocks
    : null;
  const status = widthDeltaBlocks === null || lengthDeltaBlocks === null
    ? "unknown"
    : Math.abs(widthDeltaBlocks) <= toleranceBlocks && Math.abs(lengthDeltaBlocks) <= toleranceBlocks
      ? "fits"
      : "exceeds";

  return {
    origin: { x: 0, y: 0, z: 0 },
    orientationDegrees,
    blocksPerMeter,
    schematicDimensions: {
      widthBlocks: model.width,
      heightBlocks: model.height,
      lengthBlocks: model.length
    },
    footprintDimensions: {
      widthBlocks: footprintWidthBlocks,
      lengthBlocks: footprintLengthBlocks
    },
    expectedSlotDimensions: {
      widthBlocks: expectedWidthBlocks,
      lengthBlocks: expectedLengthBlocks
    },
    widthDeltaBlocks,
    lengthDeltaBlocks,
    toleranceBlocks,
    status,
    notes: placementNotes(status, widthDeltaBlocks, lengthDeltaBlocks, toleranceBlocks)
  };
}

export function recordAxiomAcceptance(
  model: SchematicModel,
  input: {
    minecraftVersion: string;
    axiomVersion: string;
    importResult: AxiomImportResult;
    orientationCheck: AxiomAcceptanceRecord["checks"]["orientation"];
    scaleCheck: AxiomAcceptanceRecord["checks"]["scale"];
    paletteCheck: AxiomAcceptanceRecord["checks"]["palette"];
    blockPlacementCheck: AxiomAcceptanceRecord["checks"]["blockPlacement"];
    screenshots: Array<{ view: PreviewCameraView | "axiom"; uri: string; note: string }>;
    correctionNotes: string[];
    testedAt?: string;
  }
): SchematicModel {
  const minecraftVersion = input.minecraftVersion.trim();
  const axiomVersion = input.axiomVersion.trim();
  const correctionNotes = normalizeLines(input.correctionNotes);
  const screenshots = input.screenshots
    .map((screenshot) => ({
      view: screenshot.view,
      uri: screenshot.uri.trim(),
      note: screenshot.note.trim()
    }))
    .filter((screenshot) => screenshot.uri);

  if (input.importResult === "succeeded") {
    if (!minecraftVersion) throw new Error("Successful Axiom acceptance requires the Minecraft version.");
    if (!axiomVersion) throw new Error("Successful Axiom acceptance requires the Axiom version.");
    if (screenshots.length === 0) throw new Error("Successful Axiom acceptance requires at least one screenshot reference.");
  }

  if (input.importResult === "failed" && correctionNotes.length === 0) {
    throw new Error("Failed Axiom import requires actionable correction notes.");
  }

  const provenance = cloneSchematicProvenance(model.metadata.provenance);
  if (!provenance) throw new Error("Axiom acceptance requires schematic provenance.");

  provenance.axiomAcceptance = {
    testedAt: input.testedAt ?? new Date().toISOString(),
    minecraftVersion,
    axiomVersion,
    importResult: input.importResult,
    placement: checkAxiomPlacement(model),
    checks: {
      orientation: input.orientationCheck,
      scale: input.scaleCheck,
      palette: input.paletteCheck,
      blockPlacement: input.blockPlacementCheck
    },
    screenshots,
    correctionNotes
  };

  return { ...model, metadata: { ...model.metadata, provenance } };
}

function placementNotes(
  status: AxiomPlacementCheck["status"],
  widthDeltaBlocks: number | null,
  lengthDeltaBlocks: number | null,
  toleranceBlocks: number
) {
  if (status === "unknown") {
    return ["Building Slot dimensions or generation scale were missing, so placement fit needs human review."];
  }
  if (status === "fits") {
    return [
      `Generated footprint is within ±${toleranceBlocks} blocks of the reviewed Building Slot dimensions.`
    ];
  }
  return [
    `Generated footprint exceeds the reviewed Building Slot tolerance: width delta ${widthDeltaBlocks} blocks, length delta ${lengthDeltaBlocks} blocks.`
  ];
}

function normalizeLines(lines: string[]) {
  return lines.map((line) => line.trim()).filter(Boolean);
}
