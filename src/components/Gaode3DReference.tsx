import { useEffect, useRef, useState } from "react";
import type { LngLatPoint } from "../domain/buildingGeometry";
import { loadConfiguredGaodeJsApi } from "../services/liveMapProviders";
import type { Translation } from "../i18n";

interface GaodeMapInstance {
  destroy(): void;
  on?(event: string, handler: (event: { lnglat?: { getLng?(): number; getLat?(): number; lng?: number; lat?: number } }) => void): void;
  getCenter?(): { getLng?(): number; getLat?(): number; lng?: number; lat?: number };
  getZoom?(): number;
  getPitch?(): number;
  getRotation?(): number;
  add?(item: unknown): void;
  remove?(item: unknown): void;
}

interface GaodeApi {
  Map: new (container: HTMLElement, options: Record<string, unknown>) => GaodeMapInstance;
  Marker?: new (options: Record<string, unknown>) => unknown;
}

interface ReferenceView { center: LngLatPoint; zoom: number; pitch: number; rotation: number; }

function loadReferenceView(key: string, fallbackCenter: LngLatPoint): ReferenceView {
  const fallback = { center: fallbackCenter, zoom: 19, pitch: 58, rotation: 0 };
  try {
    const saved = JSON.parse(localStorage.getItem(key) ?? "null") as ReferenceView | null;
    return saved?.center && [saved.center.lng, saved.center.lat, saved.zoom, saved.pitch, saved.rotation].every(Number.isFinite) ? saved : fallback;
  } catch { return fallback; }
}

function saveReferenceView(key: string, map: GaodeMapInstance) {
  const value = map.getCenter?.(), lng = value?.getLng?.() ?? value?.lng, lat = value?.getLat?.() ?? value?.lat;
  const zoom = map.getZoom?.(), pitch = map.getPitch?.(), rotation = map.getRotation?.();
  if (![lng, lat, zoom, pitch, rotation].every(Number.isFinite)) return;
  localStorage.setItem(key, JSON.stringify({ center: { lng, lat }, zoom, pitch, rotation }));
}

export function Gaode3DReference({
  center,
  t,
  mode = "reference",
  onPickLocation
}: {
  center: LngLatPoint;
  t: Translation;
  mode?: "reference" | "picker";
  onPickLocation?: (point: LngLatPoint) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const pickRef = useRef(onPickLocation);
  useEffect(() => { pickRef.current = onPickLocation; }, [onPickLocation]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let map: GaodeMapInstance | null = null;
    let marker: unknown = null;

    void (async () => {
      try {
        const container = containerRef.current;
        if (!container) return;
        const api = await loadConfiguredGaodeJsApi() as GaodeApi;
        if (disposed) return;
        const storageKey = `campus-reconstruction:gaode-reference-view:v1:${mode}:${center.lng.toFixed(5)},${center.lat.toFixed(5)}`;
        const saved = loadReferenceView(storageKey, center);
        map = new api.Map(container, {
          viewMode: "3D",
          center: [saved.center.lng, saved.center.lat],
          zoom: saved.zoom,
          pitch: saved.pitch,
          rotation: saved.rotation,
          buildingAnimation: false,
          showBuildingBlock: true,
          features: ["bg", "road", "building", "point"]
        });
        if (api.Marker) {
          marker = new api.Marker({ position: [center.lng, center.lat] });
          map.add?.(marker);
        }
        const rememberView = () => { if (map) saveReferenceView(storageKey, map); };
        for (const event of ["moveend", "zoomend", "pitchend", "rotateend"]) map.on?.(event, rememberView);
        if (mode === "picker" && pickRef.current) {
          map.on?.("click", (event) => {
            const lng = event.lnglat?.getLng?.() ?? event.lnglat?.lng;
            const lat = event.lnglat?.getLat?.() ?? event.lnglat?.lat;
            if (!Number.isFinite(lng) || !Number.isFinite(lat)) return;
            const point = { lng: Number(lng), lat: Number(lat) };
            if (marker) map?.remove?.(marker);
            marker = api.Marker ? new api.Marker({ position: [point.lng, point.lat] }) : null;
            if (marker) map?.add?.(marker);
            pickRef.current?.(point);
          });
        }
        setError(null);
      } catch (reason) {
        if (!disposed) setError(reason instanceof Error ? reason.message : String(reason));
      }
    })();

    return () => {
      disposed = true;
      map?.destroy();
    };
  }, [center.lat, center.lng, mode]);

  const label = mode === "picker" ? t.gaodeMapPicker : t.gaode3dReference;

  return (
    <section className="gaode-3d-reference" aria-label={label}>
      <div className="candidate-heading">
        <p className="mini-label">{label}</p>
        <strong>{center.lng.toFixed(6)}, {center.lat.toFixed(6)} · GCJ-02</strong>
      </div>
      <div ref={containerRef} className="gaode-3d-map" />
      {error ? <p className="schematic-error">{error}</p> : null}
      <small>{mode === "picker" ? t.gaodeMapPickerHelp : t.gaode3dEvidenceOnly}</small>
    </section>
  );
}
