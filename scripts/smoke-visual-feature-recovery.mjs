import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";
const dir = ".scratch/runtime-smoke"; mkdirSync(dir, { recursive: true });
const entry = resolve(".scratch/runtime-smoke/visual-feature-recovery-entry.ts");
const bundle = resolve(".scratch/runtime-smoke/visual-feature-recovery-bundle.mjs");
writeFileSync(entry, `
import { extractDeterministicVisualFeaturesFromPixels } from "../../src/services/visualFeatureProvider";
const width=80,height=60,pixels=new Uint8ClampedArray(width*height*4);
for(let i=0;i<width*height;i++){ pixels[i*4]=185; pixels[i*4+1]=170; pixels[i*4+2]=150; pixels[i*4+3]=255; }
for(let y=8;y<48;y++)for(let x=10;x<22;x++){const o=(y*width+x)*4;pixels[o]=40;pixels[o+1]=140;pixels[o+2]=220;}
for(let y=36;y<48;y++)for(let x=10;x<58;x++){const o=(y*width+x)*4;pixels[o]=40;pixels[o+1]=140;pixels[o+2]=220;}
const found=extractDeterministicVisualFeaturesFromPixels(pixels,width,height,{minLng:121,minLat:31,maxLng:122,maxLat:32},"Campus");
const water=found.find(item=>item.kind==="water");
if(!water)throw new Error("water region not detected");
if(water.geometry.points.length<6)throw new Error("recognition regressed to a bounding rectangle");
if(!water.provenance.rawId.includes("deterministic-v2"))throw new Error("recognition provenance version missing");
console.log("Visual feature recovery smoke test passed.");
`);
await build({entryPoints:[entry],outfile:bundle,bundle:true,platform:"node",format:"esm",logLevel:"silent"});
await import(`${pathToFileURL(bundle).href}?t=${Date.now()}`);
