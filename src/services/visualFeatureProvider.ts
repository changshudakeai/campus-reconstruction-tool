import type { MapFeatureKind } from "../domain/foundationManifest";
import type { CandidatePoint, MapCandidate } from "../domain/mapCandidate";
import { polygonCandidate, polylineCandidate } from "./mapCandidateFactory";

interface VisualFeatureResponse {
  model?: string;
  features?: Array<{
    id?: string;
    class?: string;
    confidence?: number;
    geometry?: { type?: "polygon" | "polyline"; coordinates?: Array<[number, number]>; coordinateSpace?: "normalized" | "wgs84" };
  }>;
}

export async function queryVisualFeatureProvider(input: {
  endpoint: string;
  imageDataUrl: string;
  boundary: CandidatePoint[];
  campus: string;
}): Promise<MapCandidate[]> {
  if (!input.endpoint.trim()) throw new Error("Visual Feature Provider endpoint is required.");
  if (!input.imageDataUrl) throw new Error("A georeferenced map screenshot is required.");
  const bounds = geometryBounds(input.boundary);
  const response = await fetch(input.endpoint.trim(), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      schemaVersion: "1.0",
      campus: input.campus,
      imageDataUrl: input.imageDataUrl,
      bounds,
      classes: ["building", "road", "water", "vegetation", "sports"]
    }),
    signal: AbortSignal.timeout(180_000)
  });
  if (!response.ok) throw new Error(`Visual Feature Provider returned HTTP ${response.status}`);
  const payload = await response.json() as VisualFeatureResponse;
  return (payload.features ?? []).flatMap((feature, index) => {
    const kind = visualKind(feature.class);
    const type = feature.geometry?.type;
    const coordinates = feature.geometry?.coordinates;
    if (!kind || !type || !coordinates?.length) return [];
    const points = feature.geometry?.coordinateSpace === "wgs84"
      ? coordinates
      : coordinates.map(([x, y]) => [
          bounds.minLng + Math.max(0, Math.min(1, x)) * (bounds.maxLng - bounds.minLng),
          bounds.maxLat - Math.max(0, Math.min(1, y)) * (bounds.maxLat - bounds.minLat)
        ] as [number, number]);
    const confidence = (feature.confidence ?? 0) >= 0.85 ? "high" : (feature.confidence ?? 0) >= 0.55 ? "medium" : "low";
    const common = {
      id: `candidate-visual-${feature.id ?? index}`,
      name: `Visual ${kind} ${index + 1}`,
      kind,
      source: "screenshot_analysis" as const,
      confidence: confidence as MapCandidate["confidence"],
      query: input.campus,
      rawId: `visual:${payload.model ?? "configured-model"}:${feature.id ?? index}`,
      notes: [`Visual Feature Provider model=${payload.model ?? "unspecified"}`, `score=${feature.confidence ?? 0}`, `coordinateSpace=${feature.geometry?.coordinateSpace ?? "normalized"}`],
      points
    };
    return [type === "polyline" ? polylineCandidate(common) : polygonCandidate(common)];
  });
}

export async function queryDeterministicVisualFeatures(input: {
  imageDataUrl: string;
  boundary: CandidatePoint[];
  campus: string;
}): Promise<MapCandidate[]> {
  if (!input.imageDataUrl) throw new Error("A georeferenced map screenshot is required.");
  const image = await loadImage(input.imageDataUrl);
  const canvas = document.createElement("canvas");
  const maxSide = 1_200;
  const scale = Math.min(1, maxSide / Math.max(image.naturalWidth, image.naturalHeight));
  canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
  canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) throw new Error("Canvas image analysis is unavailable.");
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  return extractDeterministicVisualFeaturesFromPixels(
    context.getImageData(0, 0, canvas.width, canvas.height).data,
    canvas.width,
    canvas.height,
    geometryBounds(input.boundary),
    input.campus
  );
}

interface PixelBounds { minLng: number; minLat: number; maxLng: number; maxLat: number; }
interface PixelPoint { x: number; y: number; }
interface ColorRegion {
  minX: number; minY: number; maxX: number; maxY: number;
  width: number; height: number; pixelCount: number;
  contour: PixelPoint[];
}

export function extractDeterministicVisualFeaturesFromPixels(
  pixels: Uint8ClampedArray,
  width: number,
  height: number,
  bounds: PixelBounds,
  campus: string
): MapCandidate[] {
  if (pixels.length !== width * height * 4) throw new Error("Pixel buffer dimensions do not match.");
  const rules: Array<{
    kind: "water" | "vegetation" | "sports" | "road";
    matches: (r: number, g: number, b: number) => boolean;
    accepts: (region: ColorRegion) => boolean;
  }> = [
    {
      kind: "water",
      matches: (r, g, b) => { const c = colorMetrics(r, g, b); return c.hue > 195 && c.hue <= 235 && c.saturation >= 0.12 && b > r + 10; },
      accepts: (region) => region.pixelCount >= 70 && region.width >= 5 && region.height >= 5
    },
    {
      kind: "vegetation",
      matches: (r, g, b) => { const c = colorMetrics(r, g, b); return c.hue >= 70 && c.hue <= 165 && c.saturation >= 0.1 && g > r + 7 && g > b + 4 && c.lightness < 0.86; },
      accepts: (region) => region.pixelCount >= 70 && region.width >= 5 && region.height >= 5
    },
    {
      kind: "sports",
      matches: (r, g, b) => { const c = colorMetrics(r, g, b); return c.hue >= 155 && c.hue <= 195 && c.saturation >= 0.16 && g > r + 12 && b > r + 8 && c.lightness < 0.82; },
      accepts: (region) => region.pixelCount >= 110 && region.width >= 8 && region.height >= 8 && fillRatio(region) >= 0.35
    },
    {
      kind: "road",
      matches: (r, g, b) => { const c = colorMetrics(r, g, b); return c.saturation < 0.09 && c.lightness >= 0.72 && c.lightness <= 0.97; },
      accepts: (region) => {
        const longSide = Math.max(region.width, region.height), shortSide = Math.max(1, Math.min(region.width, region.height));
        return region.pixelCount >= 120 && (longSide / shortSide >= 1.8 || fillRatio(region) < 0.62);
      }
    }
  ];
  const minimumArea = Math.max(45, Math.round(width * height * 0.000045));
  return rules.flatMap((rule) => connectedColorRegions(pixels, width, height, rule.matches, minimumArea)
    .filter(rule.accepts)
    .filter((region) => region.contour.length >= 3)
    .slice(0, 60)
    .map((region, index) => {
      const simplified = simplifyClosedContour(region.contour, Math.max(1.5, Math.min(region.width, region.height) * 0.018));
      const points = simplified.map((point) => pixelToLngLat(point, width, height, bounds));
      const areaShare = region.pixelCount / (width * height);
      const confidence: MapCandidate["confidence"] = areaShare >= 0.012 && !touchesImageEdge(region, width, height) ? "medium" : "low";
      const signature = [rule.kind, region.minX, region.minY, region.maxX, region.maxY, region.pixelCount].join(":");
      return polygonCandidate({
        id: `candidate-visual-rule-${signature}`,
        name: `Rule-detected ${rule.kind} ${index + 1}`,
        kind: rule.kind,
        source: "screenshot_analysis",
        confidence,
        query: campus,
        rawId: `visual:deterministic-v2:${signature}`,
        notes: [
          "Deterministic label-free map segmentation v2",
          `pixels=${region.pixelCount}`,
          `contourPoints=${points.length}`,
          `fillRatio=${fillRatio(region).toFixed(2)}`,
          "Morphological denoising and contour simplification applied",
          "Requires human review"
        ],
        points
      });
    }));
}

function connectedColorRegions(
  pixels: Uint8ClampedArray,
  width: number,
  height: number,
  matches: (r: number, g: number, b: number) => boolean,
  minimumArea: number
): ColorRegion[] {
  let mask = new Uint8Array(width * height);
  for (let index = 0; index < mask.length; index += 1) {
    const offset = index * 4;
    mask[index] = pixels[offset + 3] > 20 && matches(pixels[offset], pixels[offset + 1], pixels[offset + 2]) ? 1 : 0;
  }
  mask = denoiseMask(mask, width, height);
  const visited = new Uint8Array(mask.length);
  const regions: ColorRegion[] = [];
  for (let start = 0; start < mask.length; start += 1) {
    if (!mask[start] || visited[start]) continue;
    const queue = [start], cells: number[] = []; visited[start] = 1;
    let minX = width, minY = height, maxX = 0, maxY = 0;
    for (let cursor = 0; cursor < queue.length; cursor += 1) {
      const cell = queue[cursor], x = cell % width, y = Math.floor(cell / width);
      cells.push(cell); minX = Math.min(minX, x); minY = Math.min(minY, y); maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
      for (const [dx, dy] of NEIGHBORS_8) {
        const nx = x + dx, ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue;
        const next = ny * width + nx;
        if (mask[next] && !visited[next]) { visited[next] = 1; queue.push(next); }
      }
    }
    if (cells.length < minimumArea) continue;
    regions.push({
      minX, minY, maxX, maxY,
      width: maxX - minX + 1, height: maxY - minY + 1, pixelCount: cells.length,
      contour: traceRegionContour(cells, width)
    });
  }
  return regions.sort((left, right) => right.pixelCount - left.pixelCount);
}

const NEIGHBORS_8 = [[1,0],[-1,0],[0,1],[0,-1],[1,1],[1,-1],[-1,1],[-1,-1]] as const;

function denoiseMask(mask: Uint8Array, width: number, height: number) {
  const opened = new Uint8Array(mask.length), closed = new Uint8Array(mask.length);
  for (let y = 1; y < height - 1; y += 1) for (let x = 1; x < width - 1; x += 1) {
    const index = y * width + x;
    if (mask[index] && neighborCount(mask, width, x, y) >= 3) opened[index] = 1;
  }
  for (let y = 1; y < height - 1; y += 1) for (let x = 1; x < width - 1; x += 1) {
    const index = y * width + x, count = neighborCount(opened, width, x, y);
    if (opened[index] || count >= 5) closed[index] = 1;
  }
  return closed;
}

function neighborCount(mask: Uint8Array, width: number, x: number, y: number) {
  let count = 0;
  for (let dy = -1; dy <= 1; dy += 1) for (let dx = -1; dx <= 1; dx += 1) {
    if (dx || dy) count += mask[(y + dy) * width + x + dx];
  }
  return count;
}

function traceRegionContour(cells: number[], width: number): PixelPoint[] {
  const set = new Set(cells), edges = new Map<string, PixelPoint[]>();
  const add = (from: PixelPoint, to: PixelPoint) => {
    const key = `${from.x},${from.y}`; const values = edges.get(key) ?? []; values.push(to); edges.set(key, values);
  };
  for (const cell of cells) {
    const x = cell % width, y = Math.floor(cell / width);
    if (y === 0 || !set.has(cell - width)) add({ x, y }, { x: x + 1, y });
    if (x === width - 1 || !set.has(cell + 1)) add({ x: x + 1, y }, { x: x + 1, y: y + 1 });
    if (!set.has(cell + width)) add({ x: x + 1, y: y + 1 }, { x, y: y + 1 });
    if (x === 0 || !set.has(cell - 1)) add({ x, y: y + 1 }, { x, y });
  }
  const loops: PixelPoint[][] = [];
  while (edges.size) {
    const firstKey = edges.keys().next().value as string;
    const [x, y] = firstKey.split(",").map(Number), loop: PixelPoint[] = [{ x, y }];
    let key = firstKey;
    for (let guard = 0; guard < cells.length * 8; guard += 1) {
      const options = edges.get(key);
      if (!options?.length) break;
      const next = options.pop()!;
      if (!options.length) edges.delete(key);
      key = `${next.x},${next.y}`;
      if (key === firstKey) break;
      loop.push(next);
    }
    if (loop.length >= 3) loops.push(loop);
  }
  return loops.sort((left, right) => Math.abs(polygonArea(right)) - Math.abs(polygonArea(left)))[0] ?? [];
}

function simplifyClosedContour(points: PixelPoint[], tolerance: number) {
  if (points.length <= 8) return points;
  const sampled = points.filter((_, index) => index % Math.max(1, Math.floor(points.length / 240)) === 0);
  const open = [...sampled, sampled[0]];
  const simplified = rdp(open, tolerance).slice(0, -1);
  return simplified.length >= 3 ? simplified : sampled.slice(0, 3);
}

function rdp(points: PixelPoint[], epsilon: number): PixelPoint[] {
  if (points.length <= 2) return points;
  let maxDistance = 0, split = 0;
  for (let index = 1; index < points.length - 1; index += 1) {
    const distance = perpendicularDistance(points[index], points[0], points.at(-1)!);
    if (distance > maxDistance) { maxDistance = distance; split = index; }
  }
  if (maxDistance <= epsilon) return [points[0], points.at(-1)!];
  return [...rdp(points.slice(0, split + 1), epsilon).slice(0, -1), ...rdp(points.slice(split), epsilon)];
}

function perpendicularDistance(point: PixelPoint, start: PixelPoint, end: PixelPoint) {
  const dx = end.x - start.x, dy = end.y - start.y;
  if (!dx && !dy) return Math.hypot(point.x - start.x, point.y - start.y);
  return Math.abs(dy * point.x - dx * point.y + end.x * start.y - end.y * start.x) / Math.hypot(dx, dy);
}

function pixelToLngLat(point: PixelPoint, width: number, height: number, bounds: PixelBounds): [number, number] {
  return [
    bounds.minLng + point.x / width * (bounds.maxLng - bounds.minLng),
    bounds.maxLat - point.y / height * (bounds.maxLat - bounds.minLat)
  ];
}

function colorMetrics(r: number, g: number, b: number) {
  const rn = r / 255, gn = g / 255, bn = b / 255;
  const max = Math.max(rn, gn, bn), min = Math.min(rn, gn, bn), delta = max - min;
  let hue = 0;
  if (delta) {
    if (max === rn) hue = 60 * (((gn - bn) / delta) % 6);
    else if (max === gn) hue = 60 * ((bn - rn) / delta + 2);
    else hue = 60 * ((rn - gn) / delta + 4);
  }
  if (hue < 0) hue += 360;
  return { hue, saturation: max ? delta / max : 0, lightness: (max + min) / 2 };
}

function fillRatio(region: ColorRegion) { return region.pixelCount / Math.max(1, region.width * region.height); }
function touchesImageEdge(region: ColorRegion, width: number, height: number) { return region.minX <= 1 || region.minY <= 1 || region.maxX >= width - 2 || region.maxY >= height - 2; }
function polygonArea(points: PixelPoint[]) { return points.reduce((sum, point, index) => { const next = points[(index + 1) % points.length]; return sum + point.x * next.y - next.x * point.y; }, 0) / 2; }

function loadImage(dataUrl: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("Unable to decode the captured map image."));
    image.src = dataUrl;
  });
}

function visualKind(value?: string): Exclude<MapFeatureKind, "campus"> | null {
  return value && ["building", "road", "water", "vegetation", "sports"].includes(value)
    ? value as Exclude<MapFeatureKind, "campus">
    : null;
}

function geometryBounds(points: CandidatePoint[]) {
  return {
    minLng: Math.min(...points.map((point) => point.lng)), minLat: Math.min(...points.map((point) => point.lat)),
    maxLng: Math.max(...points.map((point) => point.lng)), maxLat: Math.max(...points.map((point) => point.lat))
  };
}
