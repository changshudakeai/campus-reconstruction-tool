# First Vertical Slice Architecture

```mermaid
flowchart LR
  A["Online Map Query"] --> B["Map Candidates"]
  B --> C["Review and Geometry Editing"]
  C --> D["Foundation Manifest"]
  D --> E["Minimal Arnis Adapter"]
  E --> F["Building Geometry"]
  F --> G["Schematic Model"]
  G --> H["Three.js Preview and Block Editing"]
  H --> I["Sponge v2 + Provenance Export"]
```

## Seams

| Seam | Contract | Main implementation |
| --- | --- | --- |
| Online query | `CandidateProvider` | `src/services/onlineMapQuery.ts` |
| Review | `ReviewedCandidate` to `MapFeature` | `src/services/candidateReview.ts` |
| Mode handoff | `FoundationManifest` and `BuildingSlot` | `src/domain/foundationManifest.ts` |
| Building enrichment | `BuildingGeometryProvider` | `src/adapters/minimalArnisAdapter.ts` |
| Voxel generation | `BuildingGeometry` to `SchematicModel` | `src/services/buildingGeometryToSchematic.ts` |
| Preview/edit/export | `SchematicModel` | `src/components/SchematicPreviewer.tsx`, `src/services/schematicEditing.ts`, `src/services/detailedSchematicExport.ts` |

Each seam has a smoke test that exercises externally visible behavior. `scripts/smoke-first-vertical-slice-completion.mjs` verifies the complete offline path without calling live services.
