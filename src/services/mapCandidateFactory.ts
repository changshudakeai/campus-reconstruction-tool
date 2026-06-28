import type { CandidateSource, MapCandidate } from "../domain/mapCandidate";

export function sourceLabel(source: CandidateSource) {
  return {
    arnis_open_geodata: "Arnis-style open geodata",
    overture: "Overture building data",
    osm_overpass: "OSM / Overpass",
    gaode_poi: "Gaode POI",
    gaode_aoi: "Gaode AOI",
    screenshot_analysis: "Screenshot analysis",
    manual_drawing: "Manual drawing"
  }[source];
}

export function polygonCandidate(input: {
  id: string;
  name: string;
  kind: MapCandidate["kind"];
  source: CandidateSource;
  confidence: MapCandidate["confidence"];
  query: string;
  rawId: string;
  notes: string[];
  points: Array<[number, number]>;
}): MapCandidate {
  return makeCandidate(input, "polygon", input.points);
}

export function polylineCandidate(input: {
  id: string;
  name: string;
  kind: MapCandidate["kind"];
  source: CandidateSource;
  confidence: MapCandidate["confidence"];
  query: string;
  rawId: string;
  notes: string[];
  points: Array<[number, number]>;
}): MapCandidate {
  return makeCandidate(input, "polyline", input.points);
}

export function pointCandidate(input: {
  id: string;
  name: string;
  kind: MapCandidate["kind"];
  source: CandidateSource;
  confidence: MapCandidate["confidence"];
  query: string;
  rawId: string;
  notes: string[];
  coordinateSystem?: "GCJ-02" | "WGS-84";
  point: [number, number];
}): MapCandidate {
  return makeCandidate(input, "point", [input.point]);
}

export function makeCandidate(
  input: {
    id: string;
    name: string;
    kind: MapCandidate["kind"];
    source: CandidateSource;
    confidence: MapCandidate["confidence"];
    query: string;
    rawId: string;
    notes: string[];
    coordinateSystem?: "GCJ-02" | "WGS-84";
  },
  type: MapCandidate["geometry"]["type"],
  points: Array<[number, number]>
): MapCandidate {
  return {
    id: input.id,
    name: input.name,
    kind: input.kind,
    source: input.source,
    confidence: input.confidence,
    geometry: {
      type,
      points: points.map(([lng, lat]) => ({ lng, lat }))
    },
    provenance: {
      source: input.source,
      sourceLabel: sourceLabel(input.source),
      query: input.query,
      rawId: input.rawId,
      notes: input.notes,
      coordinateSystem: input.coordinateSystem
    },
    editable: true,
    accepted: false
  };
}


export function isValidCandidateGeometry(candidate: MapCandidate): boolean {
  const { type, points } = candidate.geometry;
  if (type === 'polygon') return points.length >= 3;
  if (type === 'polyline') return points.length >= 2;
  if (type === 'point') return false; // single-point geometry cannot render on map
  return false;
}
