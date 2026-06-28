import {
  CandidateSource,
  MAP_CANDIDATE_SOURCE_PRIORITY,
  MapCandidate,
  OnlineMapQueryTarget,
  PUTUO_ONLINE_QUERY_TARGET
} from "../domain/mapCandidate";
import { createDefaultCandidateProviders } from "./liveMapProviders";
import {
  isValidCandidateGeometry,
  pointCandidate,
  polygonCandidate,
  polylineCandidate
} from "./mapCandidateFactory";

export interface CandidateProvider {
  source: CandidateSource;
  query(target: OnlineMapQueryTarget): Promise<MapCandidate[]>;
}

export type ProviderCacheStatus = "miss" | "hit";

type ProviderRole = "preferred_geometry" | "search_and_naming" | "fallback";

export interface ProviderDebugEntry {
  source: CandidateSource;
  role: ProviderRole;
  cacheStatus: ProviderCacheStatus;
  count: number;
  candidateIds: string[];
  rawIds: string[];
  notesPreview: string[];
  error?: string;
}

export interface OnlineMapQueryResult {
  target: OnlineMapQueryTarget;
  candidates: MapCandidate[];
  sourceOrder: CandidateSource[];
  sourceSummaries: Array<{
    source: CandidateSource;
    count: number;
    role: ProviderRole;
    cacheStatus: ProviderCacheStatus;
  }>;
  providerDebug: ProviderDebugEntry[];
}

export class OnlineMapQueryService {
  private readonly cache = new Map<string, MapCandidate[]>();

  constructor(
    private readonly providers: CandidateProvider[],
    private readonly sourceOrder = MAP_CANDIDATE_SOURCE_PRIORITY
  ) {}

  clearCache() {
    this.cache.clear();
  }

  async queryPutuoCampus(target = PUTUO_ONLINE_QUERY_TARGET): Promise<OnlineMapQueryResult> {
    return this.queryCampus(target);
  }

  async queryCampus(target: OnlineMapQueryTarget): Promise<OnlineMapQueryResult> {
    const orderedProviders = this.sourceOrder
      .map((source) => this.providers.find((provider) => provider.source === source))
      .filter((provider): provider is CandidateProvider => Boolean(provider));

    const settledProviders = await Promise.allSettled(
      orderedProviders.map(async (provider) => {
        const cacheKey = providerCacheKey(provider.source, target);
        const cachedCandidates = this.cache.get(cacheKey);

        if (cachedCandidates) {
          return {
            source: provider.source,
            candidates: cachedCandidates,
            cacheStatus: "hit" as const,
            error: undefined
          };
        }

        const candidates = await provider.query(target);
        this.cache.set(cacheKey, candidates);

        return {
          source: provider.source,
          candidates,
          cacheStatus: "miss" as const,
          error: undefined
        };
      })
    );
    const candidatesBySource = settledProviders.map((result, index) => result.status === "fulfilled"
      ? result.value
      : {
          source: orderedProviders[index].source,
          candidates: [] as MapCandidate[],
          cacheStatus: "miss" as const,
          error: result.reason instanceof Error ? result.reason.message : String(result.reason)
        });

    const candidates = candidatesBySource.flatMap((entry) => entry.candidates).filter(isValidCandidateGeometry);
    const providerDebug = candidatesBySource.map((entry) =>
      makeProviderDebugEntry(entry.source, entry.candidates, entry.cacheStatus, entry.error)
    );

    return {
      target,
      candidates,
      sourceOrder: this.sourceOrder,
      sourceSummaries: candidatesBySource.map((entry) => ({
        source: entry.source,
        count: entry.candidates.length,
        role: sourceRole(entry.source),
        cacheStatus: entry.cacheStatus
      })),
      providerDebug
    };
  }
}

export const putuoFixtureCandidateProviders: CandidateProvider[] = [
  {
    source: "arnis_open_geodata",
    async query(target) {
      return [
        polygonCandidate({
          id: "candidate-campus-boundary-open-geodata",
          name: "Putuo Campus boundary",
          kind: "campus",
          source: "arnis_open_geodata",
          confidence: "medium",
          query: target.query,
          rawId: "open-geodata:ecnu-putuo-campus-envelope",
          notes: ["Preferred geometry source; fixture stands in for Arnis-style open geodata."],
          points: [
            [121.4047, 31.2314],
            [121.4129, 31.2311],
            [121.4141, 31.2256],
            [121.4072, 31.2245],
            [121.4039, 31.2271]
          ]
        })
      ];
    }
  },
  {
    source: "overture",
    async query(target) {
      return [
        polygonCandidate({
          id: "candidate-putuo-library-overture",
          name: "Putuo Campus Library",
          kind: "building",
          source: "overture",
          confidence: "high",
          query: target.query,
          rawId: "overture:building:putuo-library-fixture",
          notes: ["Preferred detailed building geometry candidate for the Representative Building."],
          points: [
            [121.40854, 31.22844],
            [121.40903, 31.22858],
            [121.40938, 31.2283],
            [121.40922, 31.22794],
            [121.40874, 31.22785],
            [121.40847, 31.22812]
          ]
        })
      ];
    }
  },
  {
    source: "osm_overpass",
    async query(target) {
      return [
        polylineCandidate({
          id: "candidate-campus-road-osm",
          name: "Main campus road",
          kind: "road",
          source: "osm_overpass",
          confidence: "medium",
          query: target.query,
          rawId: "osm:way:main-campus-road-fixture",
          notes: ["Road candidate from the open geodata fallback path."],
          points: [
            [121.4068, 31.2289],
            [121.4081, 31.2284],
            [121.4102, 31.2281],
            [121.4121, 31.2273]
          ]
        })
      ];
    }
  },
  {
    source: "gaode_poi",
    async query(target) {
      return [
        pointCandidate({
          id: "candidate-library-name-gaode-poi",
          name: "华东师范大学普陀校区图书馆",
          kind: "building",
          source: "gaode_poi",
          confidence: "medium",
          query: target.query,
          rawId: "gaode:poi:library-fixture",
          notes: ["Gaode POI is used for naming and positioning support, not final geometry."],
          point: [121.409, 31.2282]
        })
      ];
    }
  },
  {
    source: "gaode_aoi",
    async query(target) {
      return [
        polygonCandidate({
          id: "candidate-library-aoi-gaode",
          name: "Library AOI hint",
          kind: "building",
          source: "gaode_aoi",
          confidence: "low",
          query: target.query,
          rawId: "gaode:aoi:library-fixture",
          notes: ["AOI Candidate is a provider-specific Map Candidate, not the general candidate type."],
          points: [
            [121.40865, 31.22849],
            [121.4093, 31.22843],
            [121.40931, 31.22792],
            [121.40861, 31.22793]
          ]
        })
      ];
    }
  }
];

export const defaultOnlineMapQueryService = new OnlineMapQueryService(
  createDefaultCandidateProviders(putuoFixtureCandidateProviders)
);

function sourceRole(source: CandidateSource): OnlineMapQueryResult["sourceSummaries"][number]["role"] {
  if (source === "gaode_poi" || source === "gaode_aoi") return "search_and_naming";
  if (source === "screenshot_analysis" || source === "manual_drawing") return "fallback";
  return "preferred_geometry";
}

function providerCacheKey(source: CandidateSource, target: OnlineMapQueryTarget) {
  return [
    source,
    target.query,
    target.campus,
    target.center.lng.toFixed(6),
    target.center.lat.toFixed(6),
    target.gaodeCenter?.lng.toFixed(6) ?? "",
    target.gaodeCenter?.lat.toFixed(6) ?? "",
    target.radiusM
  ].join(":");
}

function makeProviderDebugEntry(
  source: CandidateSource,
  candidates: MapCandidate[],
  cacheStatus: ProviderCacheStatus,
  error?: string
): ProviderDebugEntry {
  return {
    source,
    role: sourceRole(source),
    cacheStatus,
    count: candidates.length,
    candidateIds: candidates.map((candidate) => candidate.id),
    rawIds: Array.from(
      new Set(candidates.map((candidate) => candidate.provenance.rawId).filter(Boolean))
    ),
    notesPreview: Array.from(
      new Set(candidates.flatMap((candidate) => candidate.provenance.notes).filter(Boolean))
    ).slice(0, 3),
    error
  };
}
