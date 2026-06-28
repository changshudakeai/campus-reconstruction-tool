import type { BuildingTarget, LngLatPoint } from "../domain/buildingGeometry";
import type { MapCandidate } from "../domain/mapCandidate";

export interface GaodeLocationAnchor {
  poiId: string;
  name: string;
  query: string;
  coordinateSystem: "GCJ-02";
  acquisition: "poi_search" | "map_click";
  position: LngLatPoint;
}

export interface OpenGeodataQueryAnchor {
  derivedFromPoiId: string;
  coordinateSystem: "WGS-84";
  position: LngLatPoint;
  transformation: "gcj02-to-wgs84-iterative-v1";
}

const PI = Math.PI;
const EARTH_SEMI_MAJOR_AXIS = 6_378_245;
const ECCENTRICITY_SQUARED = 0.006693421622965943;

export function gcj02ToWgs84(point: LngLatPoint): LngLatPoint {
  if (outsideChina(point)) return { ...point };

  let lng = point.lng;
  let lat = point.lat;
  for (let iteration = 0; iteration < 8; iteration += 1) {
    const projected = wgs84ToGcj02({ lng, lat });
    lng += point.lng - projected.lng;
    lat += point.lat - projected.lat;
  }
  return { lng, lat };
}

export function gaodeCandidateToLocationAnchor(candidate: MapCandidate): GaodeLocationAnchor {
  if (candidate.source !== "gaode_poi") {
    throw new Error("A Gaode Location Anchor requires a Gaode POI candidate.");
  }
  if (candidate.geometry.type !== "point" || candidate.geometry.points.length !== 1) {
    throw new Error("A Gaode Location Anchor requires exactly one POI point.");
  }
  if (candidate.provenance.coordinateSystem !== "GCJ-02") {
    throw new Error("Gaode POI coordinate lineage is missing or invalid.");
  }
  return {
    poiId: candidate.provenance.rawId,
    name: candidate.name,
    query: candidate.provenance.query,
    coordinateSystem: "GCJ-02",
    acquisition: "poi_search",
    position: { ...candidate.geometry.points[0] }
  };
}

export function gaodeMapClickToLocationAnchor(input: {
  name: string;
  query: string;
  point: LngLatPoint;
}): GaodeLocationAnchor {
  const name = input.name.trim() || input.query.trim();
  const query = input.query.trim();
  if (!name || !query) throw new Error("A building name is required for a map-confirmed anchor.");
  if (!Number.isFinite(input.point.lng) || !Number.isFinite(input.point.lat)) {
    throw new Error("The selected Gaode map coordinate is invalid.");
  }
  return {
    poiId: `gaode:map-click:${input.point.lng.toFixed(6)},${input.point.lat.toFixed(6)}`,
    name,
    query,
    coordinateSystem: "GCJ-02",
    acquisition: "map_click",
    position: { ...input.point }
  };
}

export function openGeodataAnchorFromGaode(
  anchor: GaodeLocationAnchor
): OpenGeodataQueryAnchor {
  return {
    derivedFromPoiId: anchor.poiId,
    coordinateSystem: "WGS-84",
    position: gcj02ToWgs84(anchor.position),
    transformation: "gcj02-to-wgs84-iterative-v1"
  };
}

export function buildingTargetFromLocationAnchors(
  gaode: GaodeLocationAnchor,
  open: OpenGeodataQueryAnchor,
  campusName = "ECNU Putuo Campus"
): BuildingTarget {
  return {
    name: gaode.name,
    campus: campusName,
    aliases: Array.from(new Set([
      gaode.name,
      gaode.query
    ].map((alias) => alias.trim()).filter(Boolean))),
    approximateCenter: { ...open.position },
    locationAnchor: {
      gaodePoiId: gaode.poiId,
      gaodeName: gaode.name,
      acquisition: gaode.acquisition,
      gcj02: { ...gaode.position },
      wgs84: { ...open.position },
      transformation: open.transformation
    }
  };
}

export function wgs84ToGcj02(point: LngLatPoint): LngLatPoint {
  if (outsideChina(point)) return { ...point };
  let latOffset = transformLatitude(point.lng - 105, point.lat - 35);
  let lngOffset = transformLongitude(point.lng - 105, point.lat - 35);
  const latitudeRadians = point.lat / 180 * PI;
  let magic = Math.sin(latitudeRadians);
  magic = 1 - ECCENTRICITY_SQUARED * magic * magic;
  const sqrtMagic = Math.sqrt(magic);
  latOffset = latOffset * 180 /
    ((EARTH_SEMI_MAJOR_AXIS * (1 - ECCENTRICITY_SQUARED)) / (magic * sqrtMagic) * PI);
  lngOffset = lngOffset * 180 /
    (EARTH_SEMI_MAJOR_AXIS / sqrtMagic * Math.cos(latitudeRadians) * PI);
  return { lng: point.lng + lngOffset, lat: point.lat + latOffset };
}

function outsideChina(point: LngLatPoint) {
  return point.lng < 72.004 || point.lng > 137.8347 || point.lat < 0.8293 || point.lat > 55.8271;
}

function transformLatitude(x: number, y: number) {
  let result = -100 + 2 * x + 3 * y + 0.2 * y * y + 0.1 * x * y
    + 0.2 * Math.sqrt(Math.abs(x));
  result += (20 * Math.sin(6 * x * PI) + 20 * Math.sin(2 * x * PI)) * 2 / 3;
  result += (20 * Math.sin(y * PI) + 40 * Math.sin(y / 3 * PI)) * 2 / 3;
  result += (160 * Math.sin(y / 12 * PI) + 320 * Math.sin(y * PI / 30)) * 2 / 3;
  return result;
}

function transformLongitude(x: number, y: number) {
  let result = 300 + x + 2 * y + 0.1 * x * x + 0.1 * x * y
    + 0.1 * Math.sqrt(Math.abs(x));
  result += (20 * Math.sin(6 * x * PI) + 20 * Math.sin(2 * x * PI)) * 2 / 3;
  result += (20 * Math.sin(x * PI) + 40 * Math.sin(x / 3 * PI)) * 2 / 3;
  result += (150 * Math.sin(x / 12 * PI) + 300 * Math.sin(x / 30 * PI)) * 2 / 3;
  return result;
}
