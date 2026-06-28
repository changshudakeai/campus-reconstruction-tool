if (process.env.RUN_LIVE_OVERPASS !== "true") {
  console.log("Skipped live Overpass building smoke test: set RUN_LIVE_OVERPASS=true to opt in.");
  process.exit(0);
}

const endpoint = process.env.VITE_OVERPASS_ENDPOINT ?? "https://overpass-api.de/api/interpreter";
const query = `
[out:json][timeout:20];
(
  way(around:250,31.228,121.409)["building"];
  relation(around:250,31.228,121.409)["building"];
);
out body 100 geom;
`.trim();
const response = await fetch(endpoint, {
  method: "POST",
  headers: {
    accept: "application/json",
    "content-type": "application/x-www-form-urlencoded;charset=UTF-8",
    "user-agent": "Campus-Reconstruction-Tool/0.1 live-smoke"
  },
  body: new URLSearchParams({ data: query }),
  signal: AbortSignal.timeout(25_000)
});
if (!response.ok) throw new Error(`Live Overpass returned HTTP ${response.status}`);
const payload = await response.json();
if (!Array.isArray(payload.elements)) throw new Error("Live Overpass omitted elements[].");
if (payload.elements.length > 100) throw new Error("Live Overpass response exceeded the requested limit.");
const polygonElements = payload.elements.filter((element) =>
  element?.type === "way"
    ? Array.isArray(element.geometry) && element.geometry.length >= 4
    : element?.type === "relation" && Array.isArray(element.members)
);
if (!polygonElements.length) {
  const summary = payload.elements.map((element) => ({
    type: element?.type,
    id: element?.id,
    geometryPoints: element?.geometry?.length ?? 0,
    members: element?.members?.length ?? 0
  }));
  throw new Error(`Live Overpass returned no usable building polygons: ${JSON.stringify(summary)}`);
}
console.log(`Live Overpass smoke test passed with ${polygonElements.length} polygon building element(s).`);
