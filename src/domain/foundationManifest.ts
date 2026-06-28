import { CandidateGeometry, CandidateProvenance } from "./mapCandidate";

export type MapFeatureKind =
  | "campus"
  | "building"
  | "road"
  | "vegetation"
  | "water"
  | "sports";

export type SourceConfidence = "high" | "medium" | "low" | "manual";

export interface CoordinateBounds {
  minLng: number;
  minLat: number;
  maxLng: number;
  maxLat: number;
}

export interface GeometryDimensions {
  pointCount: number;
  bounds: CoordinateBounds;
  approximateWidthMeters: number;
  approximateLengthMeters: number;
}

export interface MapFeature {
  id: string;
  kind: MapFeatureKind;
  name: string;
  source: string;
  confidence: SourceConfidence;
  block: string;
  reviewed: boolean;
  geometry: CandidateGeometry;
  dimensions: GeometryDimensions;
  provenance: CandidateProvenance;
  replacementPolicy: "replace" | "overlay" | "foundation-only";
}

export interface BuildingSlot {
  id: string;
  name: string;
  sourceFeatureId: string;
  geometryRole: "representative-building" | "foundation-only";
  replacementPolicy: "replace" | "overlay";
  confidence: SourceConfidence;
  selectedBlock: string;
  geometry: CandidateGeometry;
  dimensions: GeometryDimensions;
  provenance: CandidateProvenance;
  currentRefinementId?: string;
  currentRefinementVersion?: number;
  refinementStatus?: "unstarted" | "draft" | "refined" | "insufficient-data" | "deferred";
}

export interface FoundationManifest {
  schemaVersion: "0.1.0";
  target: {
    campus: string;
    coordinateSystem: "WGS84/GCJ-02 pending source";
    blocksPerMeter?: number;
    orientationDegrees?: number;
  };
  mapFeatures: MapFeature[];
  buildingSlots: BuildingSlot[];
  representativeBuildingSlotId: string | null;
}

export function createEmptyFoundationManifest(campus: string, settings: { blocksPerMeter?: number; orientationDegrees?: number } = {}): FoundationManifest {
  return {
    schemaVersion: "0.1.0",
    target: { campus, coordinateSystem: "WGS84/GCJ-02 pending source", blocksPerMeter: settings.blocksPerMeter ?? 1, orientationDegrees: settings.orientationDegrees ?? 0 },
    mapFeatures: [],
    buildingSlots: [],
    representativeBuildingSlotId: null
  };
}

export function isPutuoLibraryName(name: string) {
  const normalizedName = name.toLowerCase();
  return normalizedName.includes("library") || name.includes("图书馆");
}

export function chooseRepresentativeBuildingSlot(buildingSlots: BuildingSlot[]) {
  return (
    buildingSlots.find((slot) => isPutuoLibraryName(slot.name)) ??
    buildingSlots.find((slot) => slot.geometryRole === "representative-building") ??
    buildingSlots[0] ??
    null
  );
}

export function selectRepresentativeBuildingSlot(manifest: FoundationManifest) {
  return (
    manifest.buildingSlots.find((slot) => slot.id === manifest.representativeBuildingSlotId) ??
    chooseRepresentativeBuildingSlot(manifest.buildingSlots)
  );
}

export function summarizeGeometry(geometry: CandidateGeometry): GeometryDimensions {
  const safePoints = geometry.points.length
    ? geometry.points
    : [{ lng: 0, lat: 0 }];
  const minLng = Math.min(...safePoints.map((point) => point.lng));
  const maxLng = Math.max(...safePoints.map((point) => point.lng));
  const minLat = Math.min(...safePoints.map((point) => point.lat));
  const maxLat = Math.max(...safePoints.map((point) => point.lat));
  const centerLatRadians = (((minLat + maxLat) / 2) * Math.PI) / 180;
  const metersPerDegreeLat = 111_320;
  const metersPerDegreeLng = 111_320 * Math.cos(centerLatRadians);

  return {
    pointCount: geometry.points.length,
    bounds: {
      minLng,
      minLat,
      maxLng,
      maxLat
    },
    approximateWidthMeters: Math.round((maxLng - minLng) * metersPerDegreeLng),
    approximateLengthMeters: Math.round((maxLat - minLat) * metersPerDegreeLat)
  };
}

export const foundationManifestPlaceholder: FoundationManifest = {
  schemaVersion: "0.1.0",
  target: {
    campus: "ECNU Putuo Campus",
    coordinateSystem: "WGS84/GCJ-02 pending source"
  },
  mapFeatures: [
    {
      id: "feature-campus-putuo",
      kind: "campus",
      name: "Putuo Campus boundary",
      source: "manual review placeholder",
      confidence: "manual",
      block: "grass_block",
      reviewed: true,
      geometry: {
        type: "polygon",
        points: [
          { lng: 121.4047, lat: 31.2314 },
          { lng: 121.4129, lat: 31.2311 },
          { lng: 121.4141, lat: 31.2256 },
          { lng: 121.4072, lat: 31.2245 },
          { lng: 121.4039, lat: 31.2271 }
        ]
      },
      dimensions: summarizeGeometry({
        type: "polygon",
        points: [
          { lng: 121.4047, lat: 31.2314 },
          { lng: 121.4129, lat: 31.2311 },
          { lng: 121.4141, lat: 31.2256 },
          { lng: 121.4072, lat: 31.2245 },
          { lng: 121.4039, lat: 31.2271 }
        ]
      }),
      provenance: {
        source: "manual_drawing",
        sourceLabel: "Manual drawing",
        query: "ECNU Putuo Campus",
        rawId: "manual:putuo-campus-boundary-placeholder",
        notes: ["Placeholder reviewed feature created for the first app shell."]
      },
      replacementPolicy: "foundation-only"
    },
    {
      id: "feature-library-slot",
      kind: "building",
      name: "Putuo Campus Library",
      source: "Map Candidate review placeholder",
      confidence: "medium",
      block: "quartz_block",
      reviewed: true,
      geometry: {
        type: "polygon",
        points: [
          { lng: 121.40854, lat: 31.22844 },
          { lng: 121.40903, lat: 31.22858 },
          { lng: 121.40938, lat: 31.2283 },
          { lng: 121.40922, lat: 31.22794 },
          { lng: 121.40874, lat: 31.22785 },
          { lng: 121.40847, lat: 31.22812 }
        ]
      },
      dimensions: summarizeGeometry({
        type: "polygon",
        points: [
          { lng: 121.40854, lat: 31.22844 },
          { lng: 121.40903, lat: 31.22858 },
          { lng: 121.40938, lat: 31.2283 },
          { lng: 121.40922, lat: 31.22794 },
          { lng: 121.40874, lat: 31.22785 },
          { lng: 121.40847, lat: 31.22812 }
        ]
      }),
      provenance: {
        source: "overture",
        sourceLabel: "Overture building data",
        query: "ECNU Putuo Campus",
        rawId: "placeholder:putuo-library",
        notes: ["Placeholder reviewed Building Slot for the Representative Building."]
      },
      replacementPolicy: "replace"
    }
  ],
  buildingSlots: [
    {
      id: "slot-putuo-library",
      name: "Putuo Campus Library",
      sourceFeatureId: "feature-library-slot",
      geometryRole: "representative-building",
      replacementPolicy: "replace",
      confidence: "medium",
      selectedBlock: "quartz_block",
      geometry: {
        type: "polygon",
        points: [
          { lng: 121.40854, lat: 31.22844 },
          { lng: 121.40903, lat: 31.22858 },
          { lng: 121.40938, lat: 31.2283 },
          { lng: 121.40922, lat: 31.22794 },
          { lng: 121.40874, lat: 31.22785 },
          { lng: 121.40847, lat: 31.22812 }
        ]
      },
      dimensions: summarizeGeometry({
        type: "polygon",
        points: [
          { lng: 121.40854, lat: 31.22844 },
          { lng: 121.40903, lat: 31.22858 },
          { lng: 121.40938, lat: 31.2283 },
          { lng: 121.40922, lat: 31.22794 },
          { lng: 121.40874, lat: 31.22785 },
          { lng: 121.40847, lat: 31.22812 }
        ]
      }),
      provenance: {
        source: "overture",
        sourceLabel: "Overture building data",
        query: "ECNU Putuo Campus",
        rawId: "placeholder:putuo-library",
        notes: ["Placeholder reviewed Building Slot for the Representative Building."]
      }
    }
  ],
  representativeBuildingSlotId: "slot-putuo-library"
};
