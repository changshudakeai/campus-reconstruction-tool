import { useEffect, useRef, useState, type ReactNode } from "react";
import type { MapFeatureKind } from "../domain/foundationManifest";
import type { LngLatPoint } from "../domain/buildingGeometry";
import type { GeometryType } from "../domain/mapCandidate";
import type { SourceConfidence } from "../domain/foundationManifest";
import { loadConfiguredGaodeJsApi } from "../services/liveMapProviders";
import type { Translation } from "../i18n";

interface LngLatValue { getLng?(): number; getLat?(): number; lng?: number; lat?: number; }
interface MapShape {
  getPath?(): LngLatValue[];
  on?(event: "dragend" | "click", handler: (event: { lnglat?: LngLatValue }) => void): void;
}
interface MapMarker {
  getPosition?(): LngLatValue;
  on?(event: "dragend" | "click", handler: (event: { target?: MapMarker }) => void): void;
}
interface GaodeMapBounds { getSouthWest?(): LngLatValue; getNorthEast?(): LngLatValue; southWest?: LngLatValue; northEast?: LngLatValue; }
interface GaodeMap {
  destroy(): void;
  on?(event: string, handler: (event: { lnglat?: LngLatValue }) => void): void;
  add?(item: unknown | unknown[]): void;
  remove?(item: unknown | unknown[]): void;
  setPitch?(pitch: number): void;
  setRotation?(rotation: number): void;
  setFeatures?(features: string[]): void;
  setBounds?(bounds: unknown): void;
  getCenter?(): LngLatValue;
  getZoom?(): number;
  getPitch?(): number;
  getRotation?(): number;
  getFeatures?(): string[];
  getBounds?(): GaodeMapBounds;
  setCenter?(center: [number, number]): void;
  setZoom?(zoom: number): void;
}
interface GaodeApi {
  Map: new (container: HTMLElement, options: Record<string, unknown>) => GaodeMap;
  Polygon?: new (options: Record<string, unknown>) => MapShape;
  Polyline?: new (options: Record<string, unknown>) => MapShape;
  Marker?: new (options: Record<string, unknown>) => MapMarker;
  Bounds?: new (southWest: [number, number], northEast: [number, number]) => unknown;
}

export interface GaodeMapCaptureRequest { id: string; southWest?: LngLatPoint; northEast?: LngLatPoint; }

export interface GaodeMapView { center: LngLatPoint; zoom: number; pitch: number; rotation: number; }
export interface GaodeMapBoundsInput { southWest: LngLatPoint; northEast: LngLatPoint; }

export interface ReviewMapOverlay {
  id: string;
  kind: MapFeatureKind;
  geometry: { type: GeometryType; points: LngLatPoint[] };
  status?: "accepted" | "pending" | "rejected" | "manual";
  confidence?: SourceConfidence;
}

export function GaodeCampusBoundaryEditor({
  center,
  points,
  geometryType = "polygon",
  maxPoints,
  overlays = [],
  visibleKinds,
  selectedPointIndex = 0,
  shapeSelected = false,
  onAddPoint,
  onChangePoints,
  onSelectPoint,
  onSelectShape,
  onDeselect,
  onActivate,
  overlayInteractive = false,
  onOverlayClick,
  selectedOverlayIds = [],
  popup,
  viewStorageKey,
  captureMode = false,
  captureFitBounds,
  captureRequest,
  onCapture,
  t
}: {
  center: LngLatPoint;
  points: LngLatPoint[];
  geometryType?: "polygon" | "polyline";
  maxPoints?: number;
  overlays?: ReviewMapOverlay[];
  visibleKinds?: MapFeatureKind[];
  selectedPointIndex?: number;
  shapeSelected?: boolean;
  onAddPoint: (point: LngLatPoint) => void;
  onChangePoints?: (points: LngLatPoint[]) => void;
  onSelectPoint?: (index: number) => void;
  onSelectShape?: () => void;
  onDeselect?: () => void;
  onActivate?: () => void;
  overlayInteractive?: boolean;
  onOverlayClick?: (overlayId: string, point: LngLatPoint | null) => void;
  selectedOverlayIds?: string[];
  popup?: ReactNode;
  viewStorageKey?: string;
  captureMode?: boolean;
  captureFitBounds?: GaodeMapBoundsInput | null;
  captureRequest?: GaodeMapCaptureRequest | null;
  onCapture?: (result: { imageDataUrl: string; request: GaodeMapCaptureRequest }) => void;
  t: Translation;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const mapRef = useRef<GaodeMap | null>(null);
  const apiRef = useRef<GaodeApi | null>(null);
  const renderedRef = useRef<unknown[]>([]);
  const addRef = useRef(onAddPoint);
  const changeRef = useRef(onChangePoints);
  const selectRef = useRef(onSelectPoint);
  const selectShapeRef = useRef(onSelectShape);
  const deselectRef = useRef(onDeselect);
  const overlayClickRef = useRef(onOverlayClick);
  const shapeSelectedRef = useRef(shapeSelected);
  const captureRef = useRef(onCapture);
  const captureModeRef = useRef(captureMode);
  const captureSessionRef = useRef<{ view: GaodeMapView; features: string[] } | null>(null);
  const selectedOverlayIdSetRef = useRef(new Set(selectedOverlayIds));
  const pointsRef = useRef(points);
  const suppressMapClickUntilRef = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const [mapReady, setMapReady] = useState(false);
  const editable = Boolean(onChangePoints);
  useEffect(() => { addRef.current = onAddPoint; }, [onAddPoint]);
  useEffect(() => { changeRef.current = onChangePoints; }, [onChangePoints]);
  useEffect(() => { selectRef.current = onSelectPoint; }, [onSelectPoint]);
  useEffect(() => { selectShapeRef.current = onSelectShape; }, [onSelectShape]);
  useEffect(() => { deselectRef.current = onDeselect; }, [onDeselect]);
  useEffect(() => { overlayClickRef.current = onOverlayClick; }, [onOverlayClick]);
  useEffect(() => { shapeSelectedRef.current = shapeSelected; }, [shapeSelected]);
  useEffect(() => { captureRef.current = onCapture; }, [onCapture]);
  useEffect(() => { captureModeRef.current = captureMode; }, [captureMode]);
  useEffect(() => { selectedOverlayIdSetRef.current = new Set(selectedOverlayIds); }, [selectedOverlayIds]);
  useEffect(() => { pointsRef.current = points; }, [points]);

  useEffect(() => {
    let disposed = false;
    void (async () => {
      try {
        if (!hostRef.current) return;
        const api = await loadConfiguredGaodeJsApi() as GaodeApi;
        if (disposed || !hostRef.current) return;
        apiRef.current = api;
        const origGetCtx = HTMLCanvasElement.prototype.getContext as any;
        (HTMLCanvasElement.prototype as any).getContext = function(type: string, attrs?: any) {
          if (type === "webgl" || type === "webgl2" || type === "experimental-webgl") {
            attrs = { ...attrs, preserveDrawingBuffer: true };
          }
          return origGetCtx.call(this, type, attrs);
        };
        const initialView = loadGaodeMapView(viewStorageKey, { center, zoom: 17, pitch: 42, rotation: 0 });
        const map = new api.Map(hostRef.current, {
          viewMode: "3D", center: [initialView.center.lng, initialView.center.lat], zoom: initialView.zoom, pitch: initialView.pitch, rotation: initialView.rotation,
          showBuildingBlock: true, features: ["bg", "road", "building", "point"]
        });
        (HTMLCanvasElement.prototype as any).getContext = origGetCtx;
        map.on?.("click", (event) => {
          if (Date.now() < suppressMapClickUntilRef.current) return;
          if (shapeSelectedRef.current) {
            deselectRef.current?.();
            return;
          }
          const point = lngLatValue(event.lnglat);
          if (point) addRef.current(point);
        });
        const rememberView = () => {
          if (captureModeRef.current) return;
          const view = readMapView(map);
          if (view) saveGaodeMapView(viewStorageKey, view);
        };
        for (const event of ["moveend", "zoomend", "pitchend", "rotateend"]) map.on?.(event, rememberView);
        mapRef.current = map;
        setMapReady(true);
        setError(null);
      } catch (reason) {
        if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
      }
    })();
    return () => {
      disposed = true;
      const map = mapRef.current;
      if (!map) return;
      disposeGaodeMap(map, renderedRef.current);
      renderedRef.current = [];
      mapRef.current = null;
      captureSessionRef.current = null;
      setMapReady(false);
    };
  }, [center.lat, center.lng, viewStorageKey]);

  useEffect(() => {
    const map = mapRef.current, api = apiRef.current;
    if (!mapReady || !map || !api) return;
    if (captureMode) {
      if (!captureSessionRef.current) {
        const view = readMapView(map);
        if (view) captureSessionRef.current = { view, features: map.getFeatures?.() ?? ["bg", "road", "building", "point"] };
      }
      map.setPitch?.(0);
      map.setRotation?.(0);
      map.setFeatures?.(["bg", "road", "building"]);
      if (captureFitBounds && api.Bounds && map.setBounds) map.setBounds(new api.Bounds(
        [captureFitBounds.southWest.lng, captureFitBounds.southWest.lat],
        [captureFitBounds.northEast.lng, captureFitBounds.northEast.lat]
      ));
      return;
    }
    const session = captureSessionRef.current;
    if (!session) return;
    map.setFeatures?.(session.features);
    map.setPitch?.(session.view.pitch);
    map.setRotation?.(session.view.rotation);
    map.setCenter?.([session.view.center.lng, session.view.center.lat]);
    map.setZoom?.(session.view.zoom);
    captureSessionRef.current = null;
  }, [captureFitBounds, captureMode, mapReady]);

  useEffect(() => {
    const map = mapRef.current, host = hostRef.current;
    if (!captureRequest || !mapReady || !map || !host || !captureRef.current) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      try {
        const canvas = Array.from(host.querySelectorAll("canvas")).sort((left, right) => right.width * right.height - left.width * left.height)[0];
        if (!canvas) throw new Error("Gaode capture canvas is unavailable.");
        const imageDataUrl = canvas.toDataURL("image/png");
        if (!imageDataUrl || imageDataUrl.length < 100) throw new Error("Screenshot capture failed.");
        const bounds = readMapBounds(map) ?? (captureRequest.southWest && captureRequest.northEast
          ? { southWest: captureRequest.southWest, northEast: captureRequest.northEast }
          : null);
        if (!bounds) throw new Error("Unable to georeference the current Gaode viewport.");
        if (!cancelled) captureRef.current?.({ imageDataUrl, request: { ...captureRequest, ...bounds } });
      } catch (reason) {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      }
    }, 450);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [captureRequest, mapReady]);

  useEffect(() => {
    const map = mapRef.current, api = apiRef.current;
    if (!map || !api) return;
    if (renderedRef.current.length) safeRemoveMapItems(map, renderedRef.current);
    const rendered: unknown[] = [];
    const captureActive = captureMode || Boolean(captureRequest);
    for (const overlay of overlays) {
      if (captureActive) continue;
      if (visibleKinds && !visibleKinds.includes(overlay.kind)) continue;
      const selected = selectedOverlayIdSetRef.current.has(overlay.id);
      const path = overlay.geometry.points.map((point) => [point.lng, point.lat]);
      const Shape = overlay.geometry.type === "polyline" ? api.Polyline : api.Polygon;
      if (!Shape || path.length < (overlay.geometry.type === "polyline" ? 2 : 3)) continue;
      const color = FEATURE_COLORS[overlay.kind];
      const shape = new Shape({
        path,
        strokeColor: selected ? "#ffcf48" : color,
        strokeWeight: selected ? 7 : 3,
        strokeStyle: overlay.status === "pending" ? "dashed" : "solid",
        strokeOpacity: selected ? 1 : overlay.status === "rejected" ? 0.25 : 0.9,
        fillColor: color,
        fillOpacity: selected ? 0.38 : overlay.status === "rejected" ? 0.03 : 0.2,
        zIndex: selected ? 95 : overlay.kind === "building" ? 35 : 25,
        bubble: true,
        cursor: overlayInteractive ? "pointer" : "crosshair"
      });
      if (overlayInteractive) shape.on?.("click", (event) => {
        suppressMapClickUntilRef.current = Date.now() + 250;
        overlayClickRef.current?.(overlay.id, lngLatValue(event.lnglat));
      });
      rendered.push(shape);
    }
    const draftPath = points.map((point) => [point.lng, point.lat]);
    const DraftShape = geometryType === "polyline" ? api.Polyline : points.length >= 3 ? api.Polygon : api.Polyline;
    if (!captureActive && DraftShape && draftPath.length >= 2) {
      const draftShape = new DraftShape({
        path: draftPath,
        strokeColor: "#1e63d5", strokeWeight: 5, strokeOpacity: 0.98,
        fillColor: "#7db8f4", fillOpacity: geometryType === "polygon" ? 0.18 : 0,
        draggable: editable && shapeSelected, cursor: editable && shapeSelected ? "move" : "pointer", zIndex: 60, bubble: true
      });
      draftShape.on?.("click", () => {
        suppressMapClickUntilRef.current = Date.now() + 250;
        selectShapeRef.current?.();
      });
      draftShape.on?.("dragend", () => {
        suppressMapClickUntilRef.current = Date.now() + 250;
        const next = draftShape.getPath?.().map(lngLatValue).filter((point): point is LngLatPoint => Boolean(point)) ?? [];
        if (next.length) changeRef.current?.(next);
      });
      rendered.push(draftShape);
    }
    const Marker = api.Marker;
    if (!captureActive && Marker && editable) {
      points.forEach((point, index) => {
        const marker = new Marker({
          position: [point.lng, point.lat], draggable: shapeSelected, zIndex: 90,
          content: `<button class="map-vertex${index === selectedPointIndex ? " selected" : ""}" aria-label="vertex ${index + 1}">${index + 1}</button>`,
          anchor: "center"
        });
        marker.on?.("click", () => { suppressMapClickUntilRef.current = Date.now() + 250; selectRef.current?.(index); });
        marker.on?.("dragend", (event) => {
          suppressMapClickUntilRef.current = Date.now() + 250;
          const moved = lngLatValue(event.target?.getPosition?.() ?? marker.getPosition?.());
          if (!moved) return;
          changeRef.current?.(pointsRef.current.map((current, pointIndex) => pointIndex === index ? moved : current));
          selectRef.current?.(index);
        });
        rendered.push(marker);
      });
      for (const index of shapeSelected ? editableMidpointEdgeIndexes(points.length, selectedPointIndex, geometryType, maxPoints) : []) {
        const nextIndex = (index + 1) % points.length;
        const midpoint = { lng: (points[index].lng + points[nextIndex].lng) / 2, lat: (points[index].lat + points[nextIndex].lat) / 2 };
        const marker = new Marker({
          position: [midpoint.lng, midpoint.lat], zIndex: 85,
          content: `<button class="map-midpoint" aria-label="insert vertex">+</button>`, anchor: "center"
        });
        marker.on?.("click", () => {
          suppressMapClickUntilRef.current = Date.now() + 250;
          const next = [...pointsRef.current];
          next.splice(index + 1, 0, midpoint);
          changeRef.current?.(next);
          selectRef.current?.(index + 1);
        });
        rendered.push(marker);
      }
    }
    if (rendered.length) map.add?.(rendered);
    renderedRef.current = rendered;
    return () => {
      if (mapRef.current === map && rendered.length) safeRemoveMapItems(map, rendered);
    };
  }, [captureMode, captureRequest, editable, geometryType, maxPoints, overlayInteractive, overlays, points, selectedOverlayIds, selectedPointIndex, shapeSelected, visibleKinds]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !selectedOverlayIds.length) return;
    const selected = overlays.find((overlay) => selectedOverlayIds.includes(overlay.id));
    if (!selected?.geometry.points.length) return;
    const center = selected.geometry.points.reduce((sum, point) => ({
      lng: sum.lng + point.lng / selected.geometry.points.length,
      lat: sum.lat + point.lat / selected.geometry.points.length
    }), { lng: 0, lat: 0 });
    map.setCenter?.([center.lng, center.lat]);
    map.setZoom?.(19);
  }, [overlays, selectedOverlayIds]);

  return <section className="gaode-boundary-editor" aria-label={t.campusBoundaryEditor}>
    <div className="candidate-heading"><p className="mini-label">{t.gaodeCampusBaseMap}</p><strong>{points.length} {t.points}</strong></div>
    <div className={shapeSelected ? "gaode-boundary-map-frame shape-selected" : "gaode-boundary-map-frame"} onMouseDown={onActivate}>
      <div ref={hostRef} className="gaode-boundary-map" />
      {popup ? <div className="map-candidate-popup">{popup}</div> : null}
    </div>
    <small>{t.gaodeBoundaryDrawingHelp}</small>
    {error ? <p className="schematic-error">{error}</p> : null}
  </section>;
}

export function loadGaodeMapView(key: string | undefined, fallback: GaodeMapView, storage: Pick<Storage, "getItem"> | undefined = typeof localStorage === "undefined" ? undefined : localStorage): GaodeMapView {
  if (!key || !storage) return fallback;
  try {
    const parsed = JSON.parse(storage.getItem(`campus-reconstruction:gaode-view:v1:${key}`) ?? "null") as Partial<GaodeMapView> | null;
    if (!parsed?.center || !Number.isFinite(parsed.center.lng) || !Number.isFinite(parsed.center.lat) || !Number.isFinite(parsed.zoom) || !Number.isFinite(parsed.pitch) || !Number.isFinite(parsed.rotation)) return fallback;
    return parsed as GaodeMapView;
  } catch { return fallback; }
}

export function saveGaodeMapView(key: string | undefined, view: GaodeMapView, storage: Pick<Storage, "setItem"> | undefined = typeof localStorage === "undefined" ? undefined : localStorage) {
  if (!key || !storage) return;
  storage.setItem(`campus-reconstruction:gaode-view:v1:${key}`, JSON.stringify(view));
}

function readMapView(map: GaodeMap): GaodeMapView | null {
  const center = lngLatValue(map.getCenter?.());
  const zoom = map.getZoom?.(), pitch = map.getPitch?.(), rotation = map.getRotation?.();
  return center && Number.isFinite(zoom) && Number.isFinite(pitch) && Number.isFinite(rotation)
    ? { center, zoom: Number(zoom), pitch: Number(pitch), rotation: Number(rotation) }
    : null;
}

function readMapBounds(map: GaodeMap): GaodeMapBoundsInput | null {
  const bounds = map.getBounds?.();
  const southWest = lngLatValue(bounds?.getSouthWest?.() ?? bounds?.southWest);
  const northEast = lngLatValue(bounds?.getNorthEast?.() ?? bounds?.northEast);
  return southWest && northEast ? { southWest, northEast } : null;
}

export function editableMidpointEdgeIndexes(
  pointCount: number,
  selectedPointIndex: number,
  geometryType: "polygon" | "polyline",
  maxPoints = Number.POSITIVE_INFINITY
): number[] {
  if (pointCount >= maxPoints || pointCount < 2 || selectedPointIndex < 0 || selectedPointIndex >= pointCount) return [];
  if (geometryType === "polyline") {
    return [selectedPointIndex - 1, selectedPointIndex].filter((index) => index >= 0 && index < pointCount - 1);
  }
  if (pointCount < 3) return [];
  return Array.from(new Set([(selectedPointIndex - 1 + pointCount) % pointCount, selectedPointIndex]));
}

const FEATURE_COLORS: Record<MapFeatureKind, string> = {
  campus: "#2f7de1", building: "#d96b35", road: "#5d6872",
  vegetation: "#3f8f4f", water: "#2e7f9f", sports: "#d7a52f"
};

function lngLatValue(value: LngLatValue | undefined | null): LngLatPoint | null {
  const lng = value?.getLng?.() ?? value?.lng;
  const lat = value?.getLat?.() ?? value?.lat;
  return Number.isFinite(lng) && Number.isFinite(lat) ? { lng: Number(lng), lat: Number(lat) } : null;
}

export function disposeGaodeMap(map: Pick<GaodeMap, "remove" | "destroy">, items: unknown[]) {
  safeRemoveMapItems(map, items);
  try { map.destroy(); } catch { /* AMap teardown must never unmount the React root. */ }
}

function safeRemoveMapItems(map: Pick<GaodeMap, "remove">, items: unknown[]) {
  if (!items.length) return;
  try { map.remove?.(items); } catch { /* Map may already be disposed during a mode switch. */ }
}
