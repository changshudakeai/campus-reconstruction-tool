import type {
  CandidateSource,
  MapCandidate,
  OnlineMapQueryTarget
} from "../domain/mapCandidate";
import type { CandidateProvider } from "./onlineMapQuery";
import {
  pointCandidate,
  polygonCandidate,
  polylineCandidate
} from "./mapCandidateFactory";
import { assembleBoundaryRings } from "./campusBoundary";

interface GaodePoi {
  id?: string;
  name?: string;
  type?: string;
  location?: string;
  address?: string;
}

interface GaodePoiResponse {
  status?: string;
  info?: string;
  pois?: GaodePoi[];
}

interface GaodeJsPoi {
  id?: string;
  name?: string;
  type?: string;
  address?: string;
  location?: string | { lng?: number; lat?: number };
}

interface GaodeJsSearchRequest {
  mode: "text" | "nearby";
  pageIndex: number;
  center?: { lng: number; lat: number };
  radiusM?: number;
}

type GaodeJsSearch = (query: string, request: GaodeJsSearchRequest) => Promise<GaodeJsPoi[]>;

interface OverpassElement {
  type: "node" | "way" | "relation";
  id: number;
  tags?: Record<string, string>;
  lat?: number;
  lon?: number;
  geometry?: Array<{ lat: number; lon: number }>;
  members?: Array<{ role?: string; geometry?: Array<{ lat: number; lon: number }> }>;
  center?: { lat: number; lon: number };
}

interface OverpassResponse {
  elements?: OverpassElement[];
}

interface OvertureFeatureCollection {
  features?: Array<{
    id?: string;
    properties?: Record<string, unknown>;
    geometry?: {
      type?: "Polygon" | "MultiPolygon";
      coordinates?: unknown;
    };
  }>;
}

const OVERPASS_ENDPOINTS = [
  "/overpass-api/api/interpreter",
  "/overpass-kumi/api/interpreter",
  "/overpass-nchc/api/interpreter"
];
const overpassResponseCache = new Map<string, MapCandidate[]>();
const overtureCampusResponseCache = new Map<string, MapCandidate[]>();
const GAODE_CONFIG_STORAGE_KEY = "campus-reconstruction:gaode-config:v1";

export interface SavedGaodeConfig {
  webServiceKey?: string;
  jsApiKey?: string;
  securityJsCode?: string;
}

export function loadSavedGaodeConfig(): SavedGaodeConfig {
  try {
    const raw = globalThis.localStorage?.getItem(GAODE_CONFIG_STORAGE_KEY);
    return raw ? JSON.parse(raw) as SavedGaodeConfig : {};
  } catch {
    return {};
  }
}

export function saveSavedGaodeConfig(config: SavedGaodeConfig) {
  const cleaned: SavedGaodeConfig = {
    webServiceKey: config.webServiceKey?.trim() || undefined,
    jsApiKey: config.jsApiKey?.trim() || config.webServiceKey?.trim() || undefined,
    securityJsCode: config.securityJsCode?.trim() || undefined
  };
  globalThis.localStorage?.setItem(GAODE_CONFIG_STORAGE_KEY, JSON.stringify(cleaned));
  return cleaned;
}

export interface GaodeReverseGeocodeResult {
  name: string | null;
  formattedAddress: string;
  candidates?: Array<{ name: string; distanceM: number; type: string }>;
}

export async function reverseGeocodeGaode(point: { lng: number; lat: number }): Promise<GaodeReverseGeocodeResult> {
  if (gaodeSecurityJsCode()) {
    const api = await loadConfiguredGaodeJsApi() as {
      plugin(names: string[], callback: () => void): void;
      Geocoder?: new (options: Record<string, unknown>) => {
        getAddress(position: [number, number], callback: (status: string, result: unknown) => void): void;
      };
    };
    if (!api.Geocoder) await new Promise<void>((resolve) => api.plugin(["AMap.Geocoder"], resolve));
    const Geocoder = api.Geocoder;
    if (!Geocoder) throw new Error("Gaode Geocoder plugin did not load.");
    return new Promise((resolve, reject) => {
      new Geocoder({ radius: 120, extensions: "all" }).getAddress([point.lng, point.lat], (status, raw) => {
        if (status !== "complete") {
          reject(new Error(`Gaode reverse geocoding failed: ${status}`));
          return;
        }
        const result = raw as { regeocode?: { formattedAddress?: string; pois?: Array<{ name?: string; distance?: number | string; type?: string }> } };
        const candidates = usableReverseGeocodePois(result.regeocode?.pois ?? []);
        resolve({
          name: candidates[0]?.name ?? null,
          formattedAddress: result.regeocode?.formattedAddress ?? "",
          candidates
        });
      });
    });
  }

  const key = gaodeWebServiceKey();
  if (!key) throw new Error("Gaode Web Service key is required for reverse geocoding.");
  const endpoint = viteEnv.VITE_GAODE_REGEOCODE_ENDPOINT ?? "https://restapi.amap.com/v3/geocode/regeo";
  const url = new URL(endpoint);
  url.searchParams.set("key", key);
  url.searchParams.set("location", `${point.lng},${point.lat}`);
  url.searchParams.set("radius", "120");
  url.searchParams.set("extensions", "all");
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Gaode reverse geocoding returned HTTP ${response.status}`);
  const payload = await response.json() as { status?: string; info?: string; regeocode?: { formatted_address?: string; pois?: Array<{ name?: string; distance?: string; type?: string }> } };
  if (payload.status !== "1") throw new Error(`Gaode reverse geocoding was rejected: ${payload.info ?? "unknown error"}`);
  const candidates = usableReverseGeocodePois(payload.regeocode?.pois ?? []);
  return {
    name: candidates[0]?.name ?? null,
    formattedAddress: payload.regeocode?.formatted_address ?? "",
    candidates
  };
}

function usableReverseGeocodePois(
  pois: Array<{ name?: string; distance?: number | string; type?: string }>
) {
  return pois
    .filter((poi) =>
      Number(poi.distance ?? 999) <= 120 &&
      poi.name &&
      !/停车|出入口|道路|公交|地铁/.test(poi.name)
    )
    .map((poi) => ({
      name: poi.name!.trim(),
      distanceM: Number(poi.distance ?? 999),
      type: poi.type ?? ""
    }))
    .sort((left, right) => left.distanceM - right.distanceM);
}

const viteEnv = (import.meta as ImportMeta & {
  env?: Record<string, string | undefined>;
}).env ?? {};

function gaodeWebServiceKey() {
  const saved = loadSavedGaodeConfig();
  return saved.webServiceKey || saved.jsApiKey || viteEnv.VITE_GAODE_WEB_SERVICE_KEY;
}

function gaodeJsApiKey() {
  const saved = loadSavedGaodeConfig();
  return saved.jsApiKey || saved.webServiceKey || viteEnv.VITE_GAODE_WEB_SERVICE_KEY;
}

function gaodeSecurityJsCode() {
  return loadSavedGaodeConfig().securityJsCode || viteEnv.VITE_GAODE_SECURITY_JS_CODE;
}

export async function loadConfiguredGaodeJsApi(): Promise<unknown> {
  const apiKey = viteEnv.VITE_GAODE_WEB_SERVICE_KEY;
  const securityJsCode = viteEnv.VITE_GAODE_SECURITY_JS_CODE;
  if (!apiKey || !securityJsCode) {
    throw new Error("Gaode JS API key and security code are required.");
  }
  const browserWindow = window as typeof window & {
    _AMapSecurityConfig?: { securityJsCode: string };
    AMap?: unknown;
  };
  browserWindow._AMapSecurityConfig = { securityJsCode };
  await loadGaodeScript(apiKey);
  if (!browserWindow.AMap) throw new Error("Gaode JS API did not initialize.");
  return browserWindow.AMap;
}

export function createDefaultCandidateProviders(
  fixtureProviders: CandidateProvider[]
): CandidateProvider[] {
  const fixtureBySource = new Map(
    fixtureProviders.map((provider) => [provider.source, provider])
  );
  const useFixtures = viteEnv.VITE_BUILDING_GEOMETRY_OFFLINE_FIXTURE === "true";
  const rawOverpassProvider = useFixtures ? withFixtureFallback(
    new OverpassCandidateProvider({ endpoint: viteEnv.VITE_OVERPASS_ENDPOINT }),
    fixtureBySource.get("osm_overpass")
  ) : new OverpassCandidateProvider({ endpoint: viteEnv.VITE_OVERPASS_ENDPOINT });
  const overpassProvider: CandidateProvider = {
    source: "osm_overpass",
    async query(target) {
      return (await rawOverpassProvider.query(target)).filter((candidate) => candidate.kind !== "building");
    }
  };
  const gaodeLiveProvider = createLiveGaodePoiProvider();
  const campusBuildingProvider = useFixtures
    ? fixtureBySource.get("overture")
    : createCampusBuildingProvider();

  const gaodePoiProvider = useFixtures ? withFixtureFallback(
    gaodeLiveProvider,
    fixtureBySource.get("gaode_poi")
  ) : gaodeLiveProvider;

  return [
    useFixtures ? fixtureBySource.get("arnis_open_geodata") : undefined,
    campusBuildingProvider,
    overpassProvider,
    gaodePoiProvider,
    useFixtures ? fixtureBySource.get("gaode_aoi") : undefined
  ].filter((provider): provider is CandidateProvider => Boolean(provider));
}

export function createLiveGaodePoiProvider(): CandidateProvider {
  return viteEnv.VITE_GAODE_PROVIDER === "js_api"
    ? new GaodeJsPoiCandidateProvider({
        apiKey: viteEnv.VITE_GAODE_WEB_SERVICE_KEY,
        securityJsCode: viteEnv.VITE_GAODE_SECURITY_JS_CODE
      })
    : new GaodePoiCandidateProvider({
        apiKey: viteEnv.VITE_GAODE_WEB_SERVICE_KEY,
        endpoint: viteEnv.VITE_GAODE_POI_ENDPOINT
      });
}

export class GaodeJsPoiCandidateProvider implements CandidateProvider {
  readonly source = "gaode_poi" as const;

  constructor(
    private readonly options: {
      apiKey?: string;
      securityJsCode?: string;
      searchPoi?: GaodeJsSearch;
    } = {}
  ) {}

  async query(target: OnlineMapQueryTarget): Promise<MapCandidate[]> {
    if (!this.options.apiKey || !this.options.securityJsCode) return [];
    const searchPoi = this.options.searchPoi ?? await loadGaodeJsSearch(
      this.options.apiKey,
      this.options.securityJsCode
    );
    const queries = gaodeSearchQueries(target.query);
    const requests: Array<Promise<GaodeJsPoi[]>> = [];
    for (const [queryIndex, query] of queries.entries()) {
      const textPages = queryIndex === 0 ? [1, 2, 3] : [1];
      for (const pageIndex of textPages) {
        requests.push(searchPoi(query, { mode: "text", pageIndex }));
      }
      requests.push(searchPoi(query, {
        mode: "nearby",
        pageIndex: 1,
        center: target.gaodeCenter ?? target.center,
        radiusM: Math.max(1_500, target.radiusM)
      }));
    }
    const settled = await Promise.allSettled(requests);
    const pois = settled.flatMap((result) => result.status === "fulfilled" ? result.value : []);
    if (!pois.length && settled.every((result) => result.status === "rejected")) {
      throw new Error("All Gaode text and nearby search strategies failed.");
    }
    return deduplicateGaodeCandidates(pois
      .map((poi, index) => gaodeJsPoiToCandidate(poi, target, index))
      .filter((candidate): candidate is MapCandidate => Boolean(candidate)), target);
  }
}

export function gaodeSearchQueries(query: string) {
  const normalized = query.trim();
  if (!normalized) return [];
  const afterCampus = normalized.includes("校区")
    ? normalized.slice(normalized.lastIndexOf("校区") + 2).trim()
    : "";
  return Array.from(new Set([normalized, afterCampus].filter(Boolean)));
}

export class GaodePoiCandidateProvider implements CandidateProvider {
  readonly source = "gaode_poi" as const;

  constructor(
    private readonly options: {
      apiKey?: string;
      endpoint?: string;
      fetchJson?: typeof fetch;
    } = {}
  ) {}

  async query(target: OnlineMapQueryTarget): Promise<MapCandidate[]> {
    if (!this.options.apiKey) return [];

    const endpoint = this.options.endpoint ?? "https://restapi.amap.com/v3/place/text";
    const url = new URL(endpoint);
    url.searchParams.set("key", this.options.apiKey);
    url.searchParams.set("keywords", target.query);
    url.searchParams.set("city", "310000");
    url.searchParams.set("citylimit", "true");
    url.searchParams.set("extensions", "all");
    url.searchParams.set("offset", "20");
    url.searchParams.set("page", "1");
    url.searchParams.set("output", "json");

    const fetchJson = this.options.fetchJson ?? fetch;
    const response = await fetchJson(url);
    if (!response.ok) {
      throw new Error(`Gaode POI request failed: ${response.status}`);
    }

    const payload = (await response.json()) as GaodePoiResponse;
    if (payload.status !== "1") {
      throw new Error(`Gaode POI request rejected: ${payload.info ?? "unknown error"}`);
    }

    return (payload.pois ?? [])
      .map((poi, index) => gaodePoiToCandidate(poi, target, index))
      .filter((candidate): candidate is MapCandidate => Boolean(candidate));
  }
}

export class OverpassCandidateProvider implements CandidateProvider {
  readonly source = "osm_overpass" as const;

  constructor(
    private readonly options: {
      endpoint?: string;
      fetchJson?: typeof fetch;
      buildingsOnly?: boolean;
    } = {}
  ) {}

  async query(target: OnlineMapQueryTarget): Promise<MapCandidate[]> {
    const cacheKey = `${this.options.buildingsOnly ? "buildings" : "all"}:${target.center.lng.toFixed(5)}:${target.center.lat.toFixed(5)}:${target.radiusM}:${target.query}`;
    const cached = overpassResponseCache.get(cacheKey);
    if (cached) return cached;
    const fetchJson = this.options.fetchJson ?? fetch;
    const endpoints = this.options.endpoint
      ? [this.options.endpoint, ...OVERPASS_ENDPOINTS.filter((endpoint) => endpoint !== this.options.endpoint)]
      : OVERPASS_ENDPOINTS;
    const queries = this.options.buildingsOnly
      ? [buildOverpassBuildingQuery(target)]
      : buildOverpassFeatureQueries(target);
    const settled = await Promise.allSettled(queries.map((query) => queryOverpassLayer(query, endpoints, fetchJson)));
    const elements = settled.flatMap((result) => result.status === "fulfilled" ? result.value : []);
    if (!elements.length && settled.every((result) => result.status === "rejected")) {
      throw new Error(`All Overpass layers failed (${settled.map((result) => result.status === "rejected" ? String(result.reason) : "").filter(Boolean).join(" | ")})`);
    }
    const byId = new Map<string, MapCandidate>();
    for (const candidate of elements.flatMap((element) => overpassElementToCandidates(element, target))) {
      if (!byId.has(candidate.provenance.rawId)) byId.set(candidate.provenance.rawId, candidate);
    }
    const candidates = Array.from(byId.values());
    overpassResponseCache.set(cacheKey, candidates);
    return candidates;
  }
}

async function queryOverpassLayer(query: string, endpoints: string[], fetchJson: typeof fetch) {
  const errors: string[] = [];
  for (const endpoint of endpoints) {
    try {
      const response = await fetchJson(endpoint, {
        method: "POST",
        headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8" },
        body: new URLSearchParams({ data: query }),
        signal: AbortSignal.timeout(35_000)
      });
      if (!response.ok) { errors.push(`${endpointHost(endpoint)}: HTTP ${response.status}`); continue; }
      return ((await response.json()) as OverpassResponse).elements ?? [];
    } catch (error) {
      errors.push(`${endpointHost(endpoint)}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  throw new Error(errors.join(" | "));
}

export function clearCampusBuildingProviderCache() {
  overtureCampusResponseCache.clear();
  overpassResponseCache.clear();
}
export function createCampusBuildingProvider() {
  const overpass = new OverpassCandidateProvider({ endpoint: viteEnv.VITE_OVERPASS_ENDPOINT, buildingsOnly: true });
  return {
    source: "overture" as const,
    async query(target: OnlineMapQueryTarget) {
      const [overtureResult, osmResult] = await Promise.allSettled([
        queryOvertureCampusBuildings(target),
        overpass.query(target)
      ]);
      const overture = overtureResult.status === "fulfilled" ? overtureResult.value : [];
      const osm = osmResult.status === "fulfilled" ? osmResult.value : [];
      if (!overture.length && !osm.length) {
        throw new Error(`Overture: ${overtureResult.status === "rejected" ? String(overtureResult.reason) : "no buildings"} | OSM: ${osmResult.status === "rejected" ? String(osmResult.reason) : "no buildings"}`);
      }
      return mergeCampusBuildingCandidates(overture, osm);
    }
  } satisfies CandidateProvider;
}

export function mergeCampusBuildingCandidates(preferred: MapCandidate[], additional: MapCandidate[]) {
  const merged = [...preferred];
  for (const candidate of additional) {
    if (merged.some((existing) => sameBuildingGeometry(existing, candidate))) continue;
    merged.push(candidate);
  }
  return merged;
}

function sameBuildingGeometry(left: MapCandidate, right: MapCandidate) {
  if (left.provenance.rawId === right.provenance.rawId) return true;
  if (left.geometry.type !== "polygon" || right.geometry.type !== "polygon") return false;
  const leftCenter = geometryCenter(left.geometry.points);
  const rightCenter = geometryCenter(right.geometry.points);
  return distanceMetersWgs84(leftCenter, rightCenter) <= 8 ||
    pointInsideRing(leftCenter, right.geometry.points) ||
    pointInsideRing(rightCenter, left.geometry.points);
}

function geometryCenter(points: Array<{ lng: number; lat: number }>) {
  return points.reduce((sum, point) => ({ lng: sum.lng + point.lng / points.length, lat: sum.lat + point.lat / points.length }), { lng: 0, lat: 0 });
}

function distanceMetersWgs84(left: { lng: number; lat: number }, right: { lng: number; lat: number }) {
  const latScale = 111_320;
  const lngScale = latScale * Math.cos(((left.lat + right.lat) / 2) * Math.PI / 180);
  return Math.hypot((left.lng - right.lng) * lngScale, (left.lat - right.lat) * latScale);
}

function pointInsideRing(point: { lng: number; lat: number }, ring: Array<{ lng: number; lat: number }>) {
  let inside = false;
  for (let index = 0, previous = ring.length - 1; index < ring.length; previous = index++) {
    const left = ring[index];
    const right = ring[previous];
    if (((left.lat > point.lat) !== (right.lat > point.lat)) &&
      point.lng < (right.lng - left.lng) * (point.lat - left.lat) / ((right.lat - left.lat) || Number.EPSILON) + left.lng) inside = !inside;
  }
  return inside;
}

async function queryOvertureCampusBuildings(target: OnlineMapQueryTarget): Promise<MapCandidate[]> {
  const endpoint = viteEnv.VITE_OVERTURE_BUILDING_ENDPOINT || "http://127.0.0.1:8765/overture/buildings";
  const cacheKey = `${target.center.lng.toFixed(5)}:${target.center.lat.toFixed(5)}:${target.radiusM}`;
  const cached = overtureCampusResponseCache.get(cacheKey);
  if (cached) return cached;
  const latDegrees = target.radiusM / 111_320;
  const lngDegrees = target.radiusM / (111_320 * Math.max(0.2, Math.cos(target.center.lat * Math.PI / 180)));
  const features = await queryOvertureTile(endpoint, {
    west: target.center.lng - lngDegrees,
    south: target.center.lat - latDegrees,
    east: target.center.lng + lngDegrees,
    north: target.center.lat + latDegrees
  });
  const uniqueFeatures = Array.from(new Map(features.map((feature, index) => [feature.id ?? JSON.stringify(feature.geometry) ?? `anonymous-${index}`, feature])).values());
  const candidates = uniqueFeatures
    .map((feature, index) => overtureFeatureToCandidate(feature, target, index))
    .filter((candidate): candidate is MapCandidate => Boolean(candidate));
  overtureCampusResponseCache.set(cacheKey, candidates);
  return candidates;
}

interface QueryBbox { west: number; south: number; east: number; north: number; }

export async function queryOvertureTile(endpoint: string, bbox: QueryBbox, depth = 0, fetchJson: typeof fetch = fetch): Promise<NonNullable<OvertureFeatureCollection["features"]>> {
  if (Math.max(bbox.east - bbox.west, bbox.north - bbox.south) > 0.018 && depth < 4) return splitOvertureTile(endpoint, bbox, depth, fetchJson);
  const url = new URL(endpoint);
  url.searchParams.set("bbox", [bbox.west, bbox.south, bbox.east, bbox.north].join(","));
  url.searchParams.set("limit", "200");
  const response = await fetchJson(url, { signal: AbortSignal.timeout(180_000) });
  if (!response.ok) throw new Error(`Local Overture building request failed: HTTP ${response.status}`);
  const payload = await response.json() as OvertureFeatureCollection;
  const features = payload.features ?? [];
  if (features.length < 200 || depth >= 4 || Math.max(bbox.east - bbox.west, bbox.north - bbox.south) < 0.001) return features;
  return splitOvertureTile(endpoint, bbox, depth, fetchJson);
}

async function splitOvertureTile(endpoint: string, bbox: QueryBbox, depth: number, fetchJson: typeof fetch) {
  const midLng = (bbox.west + bbox.east) / 2, midLat = (bbox.south + bbox.north) / 2;
  return (await Promise.all([
    queryOvertureTile(endpoint, { west: bbox.west, south: bbox.south, east: midLng, north: midLat }, depth + 1, fetchJson),
    queryOvertureTile(endpoint, { west: midLng, south: bbox.south, east: bbox.east, north: midLat }, depth + 1, fetchJson),
    queryOvertureTile(endpoint, { west: bbox.west, south: midLat, east: midLng, north: bbox.north }, depth + 1, fetchJson),
    queryOvertureTile(endpoint, { west: midLng, south: midLat, east: bbox.east, north: bbox.north }, depth + 1, fetchJson)
  ])).flat();
}

function overtureFeatureToCandidate(
  feature: NonNullable<OvertureFeatureCollection["features"]>[number],
  target: OnlineMapQueryTarget,
  index: number
): MapCandidate | null {
  const polygon = largestOvertureExterior(feature.geometry);
  if (!polygon || polygon.length < 3) return null;
  const id = feature.id ?? `feature-${index}`;
  const rawName = feature.properties?.name;
  const name = typeof rawName === "string" && rawName.trim() ? rawName.trim() : `Overture building ${id.slice(0, 8)}`;
  const notes = Object.entries(feature.properties ?? {})
    .filter(([, value]) => value !== null && value !== undefined)
    .slice(0, 8)
    .map(([key, value]) => `${key}=${String(value)}`);
  return polygonCandidate({
    id: `candidate-overture-${candidateSafeId(id)}`,
    name,
    kind: "building",
    source: "overture",
    confidence: "medium",
    query: target.query,
    rawId: `overture:${id}`,
    notes: ["Local Overture Maps building footprint.", ...notes],
    points: polygon
  });
}

function largestOvertureExterior(
  geometry: { type?: "Polygon" | "MultiPolygon"; coordinates?: unknown } | undefined
): Array<[number, number]> | null {
  if (!geometry || typeof geometry !== "object" || !("type" in geometry) || !("coordinates" in geometry)) return null;
  const typed = geometry as { type?: string; coordinates?: unknown };
  const coordinates = typed.coordinates;
  const polygons = typed.type === "Polygon" ? [coordinates] : typed.type === "MultiPolygon" && Array.isArray(coordinates) ? coordinates : [];
  const exteriors = polygons
    .map((polygon) => Array.isArray(polygon) ? polygon[0] : null)
    .filter((ring): ring is unknown[] => Array.isArray(ring))
    .map((ring) => ring
      .filter((point): point is [number, number] => Array.isArray(point) && Number.isFinite(point[0]) && Number.isFinite(point[1]))
      .map(([lng, lat]) => [lng, lat] as [number, number]))
    .filter((ring) => ring.length >= 3);
  return exteriors.sort((left, right) => Math.abs(ringArea(right)) - Math.abs(ringArea(left)))[0] ?? null;
}

function ringArea(points: Array<[number, number]>) {
  return points.reduce((sum, point, index) => {
    const next = points[(index + 1) % points.length];
    return sum + point[0] * next[1] - next[0] * point[1];
  }, 0) / 2;
}


function withFixtureFallback(
  liveProvider: CandidateProvider,
  fixtureProvider?: CandidateProvider
): CandidateProvider {
  return {
    source: liveProvider.source,
    async query(target) {
      try {
        const liveCandidates = await liveProvider.query(target);
        if (liveCandidates.length > 0) return liveCandidates;
      } catch (error) {
        console.warn(`Live ${liveProvider.source} provider failed; using fixture fallback.`, error);
      }

      return fixtureProvider?.query(target) ?? [];
    }
  };
}

function gaodePoiToCandidate(
  poi: GaodePoi,
  target: OnlineMapQueryTarget,
  index: number
): MapCandidate | null {
  const point = parseGaodeLocation(poi.location);
  if (!point) return null;

  return pointCandidate({
    id: `candidate-gaode-poi-${poi.id ?? index}`,
    name: poi.name ?? "Gaode POI",
    kind: gaodeKindFromType(poi.type),
    source: "gaode_poi",
    confidence: "medium",
    query: target.query,
    rawId: `gaode:poi:${poi.id ?? index}`,
    notes: [
      "Live Gaode POI result used for naming and positioning support.",
      poi.address ? `Address: ${poi.address}` : "Address unavailable."
    ],
    coordinateSystem: "GCJ-02",
    point
  });
}

function gaodeJsPoiToCandidate(
  poi: GaodeJsPoi,
  target: OnlineMapQueryTarget,
  index: number
): MapCandidate | null {
  const point = parseGaodeJsLocation(poi.location);
  if (!point) return null;

  return pointCandidate({
    id: `candidate-gaode-js-poi-${poi.id ?? index}`,
    name: poi.name ?? "Gaode POI",
    kind: gaodeKindFromType(poi.type),
    source: "gaode_poi",
    confidence: "medium",
    query: target.query,
    rawId: `gaode:js-poi:${poi.id ?? index}`,
    notes: [
      "Live Gaode JS API result used for naming and positioning support.",
      poi.address ? `Address: ${poi.address}` : "Address unavailable."
    ],
    coordinateSystem: "GCJ-02",
    point
  });
}

function parseGaodeJsLocation(location: GaodeJsPoi["location"]): [number, number] | null {
  if (typeof location === "string") return parseGaodeLocation(location);
  const lng = Number(location?.lng);
  const lat = Number(location?.lat);
  return Number.isFinite(lng) && Number.isFinite(lat) ? [lng, lat] : null;
}

async function loadGaodeJsSearch(apiKey: string, securityJsCode: string): Promise<GaodeJsSearch> {
  const browserWindow = window as typeof window & {
    _AMapSecurityConfig?: { securityJsCode: string };
    AMap?: {
      PlaceSearch: new (options: Record<string, unknown>) => {
        search(query: string, callback: (status: string, result: unknown) => void): void;
        searchNearBy?(query: string, center: [number, number], radius: number, callback: (status: string, result: unknown) => void): void;
      };
    };
  };
  browserWindow._AMapSecurityConfig = { securityJsCode };

  if (!browserWindow.AMap?.PlaceSearch) {
    await loadGaodeScript(apiKey);
  }
  const PlaceSearch = browserWindow.AMap?.PlaceSearch;
  if (!PlaceSearch) throw new Error("Gaode JS API PlaceSearch plugin did not load.");

  return (query, request) => new Promise((resolve, reject) => {
    const search = new PlaceSearch(gaodePlaceSearchOptions(request.pageIndex));
    const callback = (status: string, rawResult: unknown) => {
      if (status !== "complete") {
        reject(new Error(`Gaode JS POI search failed: ${status}`));
        return;
      }
      const result = rawResult as { poiList?: { pois?: GaodeJsPoi[] } };
      resolve(result.poiList?.pois ?? []);
    };
    if (request.mode === "nearby" && request.center && search.searchNearBy) {
      search.searchNearBy(
        query,
        [request.center.lng, request.center.lat],
        request.radiusM ?? 1_500,
        callback
      );
    } else {
      search.search(query, callback);
    }
  });
}

export function gaodePlaceSearchOptions(pageIndex: number) {
  return {
    city: "上海",
    citylimit: true,
    pageSize: 20,
    pageIndex,
    extensions: "all"
  };
}

function deduplicateGaodeCandidates(candidates: MapCandidate[], target: OnlineMapQueryTarget) {
  const unique = new Map<string, MapCandidate>();
  for (const candidate of candidates) {
    const point = candidate.geometry.points[0];
    const key = candidate.provenance.rawId || `${candidate.name}:${point.lng.toFixed(6)},${point.lat.toFixed(6)}`;
    if (!unique.has(key)) unique.set(key, candidate);
  }
  const query = target.query.replace(/\s+/g, "").toLowerCase();
  return Array.from(unique.values()).sort((left, right) => {
    const leftName = left.name.replace(/\s+/g, "").toLowerCase();
    const rightName = right.name.replace(/\s+/g, "").toLowerCase();
    const leftMatch = leftName === query ? 2 : leftName.includes(query) || query.includes(leftName) ? 1 : 0;
    const rightMatch = rightName === query ? 2 : rightName.includes(query) || query.includes(rightName) ? 1 : 0;
    return rightMatch - leftMatch
      || gaodeDistanceSquared(left.geometry.points[0], target.gaodeCenter ?? target.center) - gaodeDistanceSquared(right.geometry.points[0], target.gaodeCenter ?? target.center);
  }).slice(0, 60);
}

function gaodeDistanceSquared(left: { lng: number; lat: number }, right: { lng: number; lat: number }) {
  const lng = left.lng - right.lng;
  const lat = left.lat - right.lat;
  return lng * lng + lat * lat;
}

function loadGaodeScript(apiKey: string) {
  const existing = document.querySelector<HTMLScriptElement>('script[data-gaode-js-api="true"]');
  if (existing) {
    return new Promise<void>((resolve, reject) => {
      if ((window as typeof window & { AMap?: unknown }).AMap) {
        resolve();
        return;
      }
      existing.addEventListener("load", () => resolve(), { once: true });
      existing.addEventListener("error", () => reject(new Error("Gaode JS API failed to load.")), { once: true });
    });
  }

  return new Promise<void>((resolve, reject) => {
    const script = document.createElement("script");
    script.dataset.gaodeJsApi = "true";
    script.src = `https://webapi.amap.com/maps?v=2.0&key=${encodeURIComponent(apiKey)}&plugin=AMap.PlaceSearch`;
    script.addEventListener("load", () => resolve(), { once: true });
    script.addEventListener("error", () => reject(new Error("Gaode JS API failed to load.")), { once: true });
    document.head.appendChild(script);
  });
}

function parseGaodeLocation(location: string | undefined): [number, number] | null {
  if (!location) return null;
  const [lng, lat] = location.split(",").map(Number);
  if (!Number.isFinite(lng) || !Number.isFinite(lat)) return null;
  return [lng, lat];
}

function gaodeKindFromType(type: string | undefined): MapCandidate["kind"] {
  if (!type) return "building";
  if (type.includes("学校") || type.includes("科教文化")) return "building";
  if (type.includes("道路")) return "road";
  if (type.includes("体育")) return "sports";
  return "building";
}

function buildOverpassFeatureQueries(target: OnlineMapQueryTarget): string[] {
  const { lat, lng } = target.center;
  const radius = Math.round(target.radiusM);
  const wrap = (body: string) => `
[out:json][timeout:25];
(
${body}
);
out body geom center;
`.trim();
  return [
    wrap(`  way(around:${radius},${lat},${lng})["highway"];\n  way(around:${radius},${lat},${lng})["area:highway"];\n  way(around:${radius},${lat},${lng})["amenity"="parking"];\n  relation(around:${radius},${lat},${lng})["area:highway"];`),
    wrap(`  way(around:${radius},${lat},${lng})["natural"~"water|bay"];\n  relation(around:${radius},${lat},${lng})["natural"~"water|bay"];\n  way(around:${radius},${lat},${lng})["water"~"pond|lake|river"];\n  relation(around:${radius},${lat},${lng})["water"~"pond|lake|river"];\n  way(around:${radius},${lat},${lng})["waterway"="river"];`),
    wrap(`  way(around:${radius},${lat},${lng})["landuse"~"grass|forest|meadow|orchard|recreation_ground"];\n  relation(around:${radius},${lat},${lng})["landuse"~"grass|forest|meadow|orchard|recreation_ground"];\n  way(around:${radius},${lat},${lng})["natural"~"wood|grassland|scrub|tree_row"];\n  relation(around:${radius},${lat},${lng})["natural"~"wood|grassland|scrub"];\n  way(around:${radius},${lat},${lng})["leisure"~"park|garden"];\n  relation(around:${radius},${lat},${lng})["leisure"~"park|garden"];\n  node(around:${radius},${lat},${lng})["natural"="tree"];`),
    wrap(`  way(around:${radius},${lat},${lng})["leisure"~"pitch|track"];\n  relation(around:${radius},${lat},${lng})["leisure"~"pitch|track"];\n  way(around:${radius},${lat},${lng})["sport"];\n  relation(around:${radius},${lat},${lng})["sport"];`)
  ];
}

function buildOverpassBuildingQuery(target: OnlineMapQueryTarget): string {
  const { lat, lng } = target.center;
  const radius = Math.round(target.radiusM);
  return `
[out:json][timeout:35];
(
  way(around:${radius},${lat},${lng})["building"];
  relation(around:${radius},${lat},${lng})["building"];
);
out body geom center;
`.trim();
}

function overpassElementToCandidates(
  element: OverpassElement,
  target: OnlineMapQueryTarget
): MapCandidate[] {
  if (element.type === "relation" && element.members?.length) {
    const rings = assembleBoundaryRings(element.members
      .filter((member) => !member.role || member.role === "outer")
      .map((member) => (member.geometry ?? []).map((point) => ({ lng: point.lon, lat: point.lat }))));
    return rings.map((ring, index) => overpassGeometryToCandidate(element, target, ring.map((point) => [point.lng, point.lat] as [number, number]), index)).filter((candidate): candidate is MapCandidate => Boolean(candidate));
  }
  const points = element.geometry?.map((point) => [point.lon, point.lat] as [number, number]) ?? [];
  const candidate = overpassGeometryToCandidate(element, target, points, 0);
  return candidate ? [candidate] : [];
}

function overpassGeometryToCandidate(element: OverpassElement, target: OnlineMapQueryTarget, points: Array<[number, number]>, partIndex: number): MapCandidate | null {
  const tags = element.tags ?? {};
  const kind = overpassKindFromTags(tags);
  const name = tags.name ?? defaultOverpassName(kind, element);
  const rawId = `osm:${element.type}:${element.id}${partIndex ? `:outer:${partIndex}` : ""}`;
  const notes = [
    "Live OSM/Overpass result with geometry from out geom.",
    `OSM tags: ${Object.entries(tags)
      .slice(0, 6)
      .map(([key, value]) => `${key}=${value}`)
      .join(", ") || "none"}`
  ];

  if (points.length >= 3 && isClosed(points)) {
    return polygonCandidate({
      id: `candidate-${candidateSafeId(rawId)}`,
      name,
      kind,
      source: "osm_overpass",
      confidence: kind === "building" ? "medium" : "low",
      query: target.query,
      rawId,
      notes,
      points
    });
  }

  if (points.length >= 2) {
    return polylineCandidate({
      id: `candidate-${candidateSafeId(rawId)}`,
      name,
      kind,
      source: "osm_overpass",
      confidence: "medium",
      query: target.query,
      rawId,
      notes,
      points
    });
  }

  const center = element.center ?? (
    typeof element.lon === "number" && typeof element.lat === "number"
      ? { lon: element.lon, lat: element.lat }
      : null
  );
  if (!center) return null;

  return pointCandidate({
    id: `candidate-${candidateSafeId(rawId)}`,
    name,
    kind,
    source: "osm_overpass",
    confidence: "low",
    query: target.query,
    rawId,
    notes,
    point: [center.lon, center.lat]
  });
}

function overpassKindFromTags(tags: Record<string, string>): MapCandidate["kind"] {
  if (tags.building) return "building";
  if (tags.highway || tags["area:highway"] || tags.amenity === "parking") return "road";
  if (["water", "bay"].includes(tags.natural) || ["pond", "lake", "river"].includes(tags.water) || tags.waterway === "river") return "water";
  if (["pitch", "track"].includes(tags.leisure) || tags.sport) return "sports";
  if (tags.landuse || ["park", "garden"].includes(tags.leisure) || ["wood", "grassland", "scrub", "tree_row", "tree"].includes(tags.natural)) return "vegetation";
  return "building";
}

function defaultOverpassName(kind: MapCandidate["kind"], element: OverpassElement) {
  return `${kind} ${element.type}/${element.id}`;
}

function candidateSafeId(value: string) {
  return value.replace(/:/g, "-");
}

function isClosed(points: Array<[number, number]>) {
  const first = points[0];
  const last = points[points.length - 1];
  return first[0] === last[0] && first[1] === last[1];
}

function endpointHost(endpoint: string) {
  try { return new URL(endpoint, typeof location === "undefined" ? "http://localhost" : location.href).host; }
  catch { return endpoint; }
}
