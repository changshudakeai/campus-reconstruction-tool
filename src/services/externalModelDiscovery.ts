import type { ArnisBuildingCandidate } from "../adapters/arnisRustCoreAdapter";
import type { ExternalModelCandidate, ExternalModelLicense } from "../domain/externalModel";
import { classifyExternalModelLicense } from "./externalModelReview";

const THREE_D_MODEL_KEYS = [
  "3dmr",
  "3dmr:id",
  "model:3dmr",
  "model",
  "3d_model"
];

export function externalModelCandidatesFromArnis(candidate: ArnisBuildingCandidate): ExternalModelCandidate[] {
  const candidates: ExternalModelCandidate[] = [];
  const tags = candidate.tags;

  for (const key of THREE_D_MODEL_KEYS) {
    const value = tags[key]?.trim();
    if (!value) continue;
    candidates.push({
      id: `${candidate.id}:3dmr:${slug(value)}`,
      source: "3dmr",
      title: tags["3dmr:title"] || tags["model:title"] || candidate.name || "External 3D model",
      sourceUrl: modelUrl(value, tags["3dmr:url"] || tags["model:url"]),
      author: tags["3dmr:author"] || tags["model:author"] || tags.author || "",
      license: licenseFromTags(tags, "3dmr"),
      linkedFeatureId: candidate.id,
      dimensionsMeters: {
        width: candidate.widthM,
        height: candidate.heightM ?? undefined,
        length: candidate.lengthM
      },
      notes: [`Discovered from source tag ${key}.`]
    });
  }

  const wikidata = tags.wikidata?.trim();
  if (wikidata) {
    candidates.push({
      id: `${candidate.id}:wikidata:${slug(wikidata)}`,
      source: "wikidata",
      title: tags.name || candidate.name || `Wikidata-linked model ${wikidata}`,
      sourceUrl: tags["wikidata:model_url"] || `https://www.wikidata.org/wiki/${wikidata}`,
      author: tags["wikidata:model_author"] || tags.author || "",
      license: licenseFromTags(tags, "wikidata"),
      linkedFeatureId: candidate.id,
      wikidataId: wikidata,
      dimensionsMeters: {
        width: candidate.widthM,
        height: candidate.heightM ?? undefined,
        length: candidate.lengthM
      },
      notes: ["Discovered from a Wikidata-linked source object."]
    });
  }

  return dedupeById(candidates);
}

export function summarizeExternalModelCandidates(candidate: ArnisBuildingCandidate) {
  const externalModels = externalModelCandidatesFromArnis(candidate);
  const eligible = externalModels.filter((model) => classifyExternalModelLicense(model).eligibility === "eligible").length;
  const blocked = externalModels.filter((model) => classifyExternalModelLicense(model).eligibility === "blocked").length;
  return { total: externalModels.length, eligible, blocked };
}

function licenseFromTags(tags: Record<string, string>, namespace: "3dmr" | "wikidata"): ExternalModelLicense | null {
  const name = tags[`${namespace}:license`] || tags["model:license"] || tags.license;
  if (!name?.trim()) return null;
  return {
    name: name.trim(),
    url: tags[`${namespace}:license_url`] || tags["model:license_url"] || licenseUrl(name),
    allowsAdaptation: allowsAdaptation(name),
    requiresAttribution: /\bBY\b|attribution/i.test(name),
    requiresShareAlike: /\bSA\b|share.?alike/i.test(name),
    allowsCommercialUse: !/\bNC\b|non.?commercial/i.test(name),
    noDerivatives: /\bND\b|no.?derivatives/i.test(name)
  };
}

function allowsAdaptation(name: string) {
  if (/\bND\b|no.?derivatives/i.test(name)) return false;
  if (/CC0|public domain|CC-BY|CC BY|ODbL|MIT|Apache|BSD/i.test(name)) return true;
  return undefined;
}

function licenseUrl(name: string) {
  const normalized = name.toUpperCase().replace(/\s+/g, "-");
  if (normalized.includes("CC-BY-SA-4.0")) return "https://creativecommons.org/licenses/by-sa/4.0/";
  if (normalized.includes("CC-BY-4.0")) return "https://creativecommons.org/licenses/by/4.0/";
  if (normalized.includes("CC-BY-NC-4.0")) return "https://creativecommons.org/licenses/by-nc/4.0/";
  if (normalized.includes("CC-BY-ND-4.0")) return "https://creativecommons.org/licenses/by-nd/4.0/";
  if (normalized.includes("CC0")) return "https://creativecommons.org/publicdomain/zero/1.0/";
  return undefined;
}

function modelUrl(value: string, explicitUrl?: string) {
  if (explicitUrl?.trim()) return explicitUrl.trim();
  if (/^https?:\/\//i.test(value)) return value;
  return `https://3dmr.eu/models/${encodeURIComponent(value)}`;
}

function slug(value: string) {
  return value.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || "model";
}

function dedupeById(candidates: ExternalModelCandidate[]) {
  const seen = new Set<string>();
  return candidates.filter((candidate) => {
    if (seen.has(candidate.id)) return false;
    seen.add(candidate.id);
    return true;
  });
}
