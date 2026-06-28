import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const dir = ".scratch/runtime-smoke"; mkdirSync(dir, { recursive: true });
const entry = resolve(`${dir}/campus-project-store-entry.ts`);
const bundle = resolve(`${dir}/campus-project-store-bundle.mjs`);
writeFileSync(entry, `
import { createCampusProject, duplicateCampusProject, exportCampusProjectJson, importCampusProjectJson, listCampusProjects } from "../../src/services/campusProjectStore";
const values = new Map<string,string>();
globalThis.localStorage = { getItem:k=>values.get(k)??null,setItem:(k,v)=>void values.set(k,v),removeItem:k=>void values.delete(k),clear:()=>values.clear(),key:()=>null,length:0 } as Storage;
const campus={id:"c",schoolName:"School",canonicalName:"School Campus",aliases:[],center:{lng:1,lat:1},openCenter:{lng:1,lat:1},radiusM:500,gaodePoiId:"g",blocksPerMeter:1,orientationDegrees:0,orientationLine:null};
const foundation={boundaryDraft:{id:"b",name:"b",kind:"campus",block:"grass_block",points:[]},candidates:[],reviews:{},manualFeatures:[],foundationStyle:{blocks:{campus:"grass_block",building:"quartz_block",road:"gray_concrete",vegetation:"moss_block",water:"water",sports:"orange_concrete"},roadWidthBlocks:3},foundationStylePack:{schemaVersion:"1.0",id:"p",name:"p",features:{}},orientationDegrees:0};
const first=createCampusProject({name:"Plan A",campus,foundation});
const second=duplicateCampusProject(first,"Plan B");
if(listCampusProjects(campus).length!==1||second.name!=="Plan B") throw new Error("project save-as failed");
const json=exportCampusProjectJson(second); const imported=importCampusProjectJson(json);
const projects=listCampusProjects(campus);
if(imported.schemaVersion!=="1.0"||projects.length!==2||imported.id===second.id||imported.name!=="Plan B (imported)") throw new Error("project import copy failed");
console.log("Campus project store smoke test passed.");
`);
await build({entryPoints:[entry],outfile:bundle,bundle:true,platform:"node",format:"esm",logLevel:"silent"});
await import(`${pathToFileURL(bundle).href}?t=${Date.now()}`);
