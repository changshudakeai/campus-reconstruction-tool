const endpoint = process.env.OVERTURE_BUILDING_ENDPOINT;
if (!endpoint) {
  console.log("Skipped live Overture smoke test: OVERTURE_BUILDING_ENDPOINT is not configured.");
  process.exit(0);
}

const center = { lng: 121.409, lat: 31.228 };
const radiusM = 120;
const latDelta = radiusM / 111_320;
const lngDelta = radiusM / (111_320 * Math.cos(center.lat * Math.PI / 180));
const bounds = [
  center.lng - lngDelta,
  center.lat - latDelta,
  center.lng + lngDelta,
  center.lat + latDelta
];
const url = new URL(endpoint);
url.searchParams.set("lng", String(center.lng));
url.searchParams.set("lat", String(center.lat));
url.searchParams.set("radius_m", String(radiusM));
url.searchParams.set("bbox", bounds.join(","));
url.searchParams.set("limit", "20");
url.searchParams.set("name", "Putuo Campus Library");
url.searchParams.set("theme", "buildings");
url.searchParams.set("type", "building");
if (process.env.OVERTURE_RELEASE_ID) {
  url.searchParams.set("release", process.env.OVERTURE_RELEASE_ID);
}

const response = await fetch(url, {
  headers: { accept: "application/geo+json, application/json" },
  signal: AbortSignal.timeout(180_000)
});
if (!response.ok) throw new Error(`Live Overture bridge returned HTTP ${response.status}`);
const payload = await response.json();
if (!Array.isArray(payload.features)) throw new Error("Live Overture bridge omitted features[].");
if (payload.features.length > 20) throw new Error("Live Overture bridge ignored the strict feature limit.");
const usable = payload.features.filter((feature) =>
  feature?.id && ["Polygon", "MultiPolygon"].includes(feature?.geometry?.type)
);
if (!usable.length) throw new Error("Live Overture bridge returned no traceable polygon buildings.");

console.log(`Live Overture smoke test passed with ${usable.length} traceable building feature(s).`);
