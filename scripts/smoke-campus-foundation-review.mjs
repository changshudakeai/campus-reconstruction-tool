import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const smokeDir = ".scratch/runtime-smoke";
const entryPath = resolve(`${smokeDir}/campus-foundation-review-entry.ts`);
const bundlePath = resolve(`${smokeDir}/campus-foundation-review-bundle.mjs`);
mkdirSync(smokeDir, { recursive: true });
writeFileSync(entryPath, `
import { assembleBoundaryRings, boundaryRingIsValid } from "../../src/services/campusBoundary";
import { scopeCandidatesToBoundary, targetFromCampusBoundary } from "../../src/services/campusFeatureReview";
import { OnlineMapQueryService } from "../../src/services/onlineMapQuery";
import { polygonCandidate, polylineCandidate } from "../../src/services/mapCandidateFactory";
const assert = (value: unknown, message: string) => { if (!value) throw new Error(message); };
const a={lng:121,lat:31}, b={lng:121.01,lat:31}, c={lng:121.01,lat:31.01}, d={lng:121,lat:31.01};
const rings=assembleBoundaryRings([[a,b],[c,b],[c,d],[a,d]]);
assert(rings.length===1 && rings[0].length===5, "relation segments were not assembled into one closed ring");
assert(!boundaryRingIsValid([{lng:121,lat:31},{lng:121.00001,lat:31},{lng:121,lat:31.00001},{lng:121,lat:31}]), "tiny triangle passed campus boundary validation");
const boundary=[a,b,c,d,a];
const building=polygonCandidate({id:"b",name:"B",kind:"building",source:"overture",confidence:"medium",query:"q",rawId:"b",notes:[],points:[[121.002,31.002],[121.004,31.002],[121.004,31.004],[121.002,31.004],[121.002,31.002]]});
const road=polylineCandidate({id:"r",name:"R",kind:"road",source:"osm_overpass",confidence:"medium",query:"q",rawId:"r",notes:[],points:[[120.99,31.005],[121.005,31.005],[121.02,31.005]]});
const scoped=scopeCandidatesToBoundary([building,road],boundary);
assert(scoped.length===2 && scoped.every((item)=>item.defaultAccepted), "valid campus features were not included by default");
const clippedRoad=scoped.find((item)=>item.candidate.id==="r")!.candidate.geometry.points;
assert(clippedRoad[0].lng>=121 && clippedRoad.at(-1)!.lng<=121.01, "road was not clipped to campus boundary");
const target=targetFromCampusBoundary({query:"q",campus:"c",center:a,radiusM:1},boundary);
assert(target.radiusM>500 && target.center.lng>121, "boundary did not drive feature query scope");
const service=new OnlineMapQueryService([
  {source:"overture",query:async()=>[building]},
  {source:"osm_overpass",query:async()=>{throw new Error("offline")}}
],["overture","osm_overpass"]);
const result=await service.queryCampus(target);
assert(result.candidates.length===1, "one provider failure erased successful results");
assert(result.providerDebug.find((entry)=>entry.source==="osm_overpass")?.error==="offline", "provider failure was not preserved for retry UI");
console.log("Campus foundation review smoke test passed.");
`);
await build({ entryPoints: [entryPath], outfile: bundlePath, bundle: true, platform: "node", format: "esm", target: "node20", logLevel: "silent" });
await import(`${pathToFileURL(bundlePath).href}?t=${Date.now()}`);
