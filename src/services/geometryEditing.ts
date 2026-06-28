import type { MapFeature, MapFeatureKind } from "../domain/foundationManifest";
import type { CandidatePoint, MapCandidate, OnlineMapQueryTarget } from "../domain/mapCandidate";
import { manualInputToMapFeature } from "./candidateReview";

export interface GeometryDraft {
  id: string;
  name: string;
  kind: MapFeatureKind;
  block: string;
  points: CandidatePoint[];
}

export const MAX_CAMPUS_BOUNDARY_POINTS = 50;

export function createEmptyGeometryDraft(target: OnlineMapQueryTarget): GeometryDraft {
  return {
    id: "manual-draft-putuo-boundary",
    name: "Manual edited boundary",
    kind: "campus",
    block: "grass_block",
    points: [
      { lng: target.center.lng - 0.001, lat: target.center.lat + 0.001 },
      { lng: target.center.lng + 0.001, lat: target.center.lat + 0.001 },
      { lng: target.center.lng + 0.001, lat: target.center.lat - 0.001 }
    ]
  };
}

export function geometryDraftFromCandidate(candidate: MapCandidate): GeometryDraft {
  const points = candidate.geometry.points.map((point) => ({ ...point }));
  if (candidate.geometry.type === "polygon" && points.length > 3) {
    const first = points[0];
    const last = points[points.length - 1];
    if (Math.abs(first.lng - last.lng) < 1e-9 && Math.abs(first.lat - last.lat) < 1e-9) points.pop();
  }
  return {
    id: `draft-${candidate.id}`,
    name: `${candidate.name} edited`,
    kind: candidate.kind,
    block: defaultBlockForKind(candidate.kind),
    points
  };
}

export function boundaryDraftFromCandidate(candidate: MapCandidate): GeometryDraft {
  return limitCampusBoundaryDraft(geometryDraftFromCandidate(candidate));
}

export function limitCampusBoundaryDraft(draft: GeometryDraft): GeometryDraft {
  if (draft.kind !== "campus" || draft.points.length <= MAX_CAMPUS_BOUNDARY_POINTS) {
    return { ...draft, points: draft.points.map((point) => ({ ...point })) };
  }
  return { ...draft, points: simplifyClosedPolygon(draft.points, MAX_CAMPUS_BOUNDARY_POINTS) };
}

export function simplifyClosedPolygon(points: CandidatePoint[], maxPoints = MAX_CAMPUS_BOUNDARY_POINTS): CandidatePoint[] {
  const limitedMax = Math.max(3, Math.floor(maxPoints));
  const remaining = points.map((point) => ({ ...point }));
  while (remaining.length > limitedMax) {
    let smallestArea = Number.POSITIVE_INFINITY;
    let removeIndex = 0;
    for (let index = 0; index < remaining.length; index += 1) {
      const previous = remaining[(index - 1 + remaining.length) % remaining.length];
      const current = remaining[index];
      const next = remaining[(index + 1) % remaining.length];
      const area = Math.abs(
        (current.lng - previous.lng) * (next.lat - previous.lat)
        - (current.lat - previous.lat) * (next.lng - previous.lng)
      );
      if (area < smallestArea) {
        smallestArea = area;
        removeIndex = index;
      }
    }
    remaining.splice(removeIndex, 1);
  }
  return remaining;
}

export function addPointToDraft(draft: GeometryDraft, point: CandidatePoint): GeometryDraft {
  return {
    ...draft,
    points: [...draft.points, point]
  };
}

export function moveDraftPoint(
  draft: GeometryDraft,
  pointIndex: number,
  delta: CandidatePoint
): GeometryDraft {
  return {
    ...draft,
    points: draft.points.map((point, index) =>
      index === pointIndex
        ? {
            lng: point.lng + delta.lng,
            lat: point.lat + delta.lat
          }
        : point
    )
  };
}

export function replaceDraftPoint(
  draft: GeometryDraft,
  pointIndex: number,
  point: CandidatePoint
): GeometryDraft {
  return {
    ...draft,
    points: draft.points.map((current, index) => index === pointIndex ? { ...point } : current)
  };
}

export function replaceDraftPoints(draft: GeometryDraft, points: CandidatePoint[]): GeometryDraft {
  return { ...draft, points: points.map((point) => ({ ...point })) };
}

export function insertDraftPoint(draft: GeometryDraft, pointIndex: number, point: CandidatePoint): GeometryDraft {
  const points = [...draft.points];
  points.splice(Math.max(0, Math.min(pointIndex, points.length)), 0, { ...point });
  return { ...draft, points };
}

export function removeDraftPoint(draft: GeometryDraft, pointIndex: number): GeometryDraft {
  return { ...draft, points: draft.points.filter((_, index) => index !== pointIndex) };
}

export function moveDraftGeometry(draft: GeometryDraft, delta: CandidatePoint): GeometryDraft {
  return {
    ...draft,
    points: draft.points.map((point) => ({ lng: point.lng + delta.lng, lat: point.lat + delta.lat }))
  };
}

export function removeLastDraftPoint(draft: GeometryDraft): GeometryDraft {
  return {
    ...draft,
    points: draft.points.slice(0, -1)
  };
}

export function draftCanClose(draft: GeometryDraft) {
  return draft.points.length >= 3;
}

export function draftCanCommit(draft: GeometryDraft) {
  return draft.kind === "road" ? draft.points.length >= 2 : draft.points.length >= 3;
}

export function draftToManualFeature(draft: GeometryDraft, index: number): MapFeature {
  if (!draftCanCommit(draft)) {
    throw new Error(draft.kind === "road" ? "A manual road needs at least two points." : "A manual feature needs at least three points.");
  }

  return manualInputToMapFeature({
    id: `feature-manual-geometry-${index}`,
    name: draft.name,
    kind: draft.kind,
    block: draft.block,
    geometry: {
      type: draft.kind === "road" ? "polyline" : "polygon",
      points: draft.points.map((point) => ({ ...point }))
    },
    provenance: {
      source: "manual_drawing",
      sourceLabel: "Manual drawing",
      query: "Gaode feature review map",
      rawId: draft.id,
      notes: ["User-created or adjusted geometry from the Foundation Feature Review Map."]
    }
  });
}

function defaultBlockForKind(kind: MapFeatureKind) {
  return {
    campus: "grass_block",
    building: "quartz_block",
    road: "gray_concrete",
    vegetation: "moss_block",
    water: "water",
    sports: "orange_concrete"
  }[kind];
}
