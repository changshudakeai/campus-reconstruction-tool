import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";
const dir=".scratch/runtime-smoke"; mkdirSync(dir,{recursive:true}); const entry=resolve(`${dir}/candidate-confidence-entry.ts`),bundle=resolve(`${dir}/candidate-confidence-bundle.mjs`);
writeFileSync(entry, `
import { classifyCampusCandidates } from "../../src/services/candidateConfidence";
import { polygonCandidate } from "../../src/services/mapCandidateFactory";
const make=(id:string,delta:number)=>polygonCandidate({id,name:id,kind:"building",source:"overture",confidence:"medium",query:"c",rawId:id,notes:[],points:[[121,31],[121+delta,31],[121+delta,31+delta],[121,31+delta],[121,31]]});
const [normal,tiny]=classifyCampusCandidates([make("normal",0.0002),make("tiny",0.000005)]);
if(normal.confidence!=="high"||tiny.confidence!=="low"||!tiny.confidenceReasons?.length) throw new Error("candidate confidence classification failed");
console.log("Candidate confidence smoke test passed.");
`);
await build({entryPoints:[entry],outfile:bundle,bundle:true,platform:"node",format:"esm",logLevel:"silent"}); await import(`${pathToFileURL(bundle).href}?t=${Date.now()}`);
