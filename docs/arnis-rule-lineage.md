# Arnis Rule Lineage

The Minimal Arnis Adapter adapts a narrow set of pre-world-output interpretation rules from [`louis-e/arnis`](https://github.com/louis-e/arnis) commit `7d2a0ebed00f0b023a4bb8238ea7cbe9d35aa148`.

## Retrieval path studied

- `src/retrieve_data.rs`: bounded Overpass query, complete relation/way/node retrieval, primary and fallback endpoints, timeouts, and smaller-area failure guidance.
- `src/overture.rs`: release-aware Overture GeoParquet access, bounding-box partition filtering, non-fatal provider fallback, safety limits, OSM-source filtering, and spatial de-duplication.

## Building rules adapted

Source: `src/element_processing/buildings.rs` at the commit above.

- `arnis-explicit-height-overrides-levels`: explicit `height` overrides `building:levels`; `min_height` is subtracted from absolute height.
- `arnis-levels-to-height`: when height is absent, generation height follows `levels × 4 + 2` at scale 1.
- `arnis-roof-shape-synonyms`: explicit `roof:shape` values are normalized through Arnis's `parse_roof_type` synonym groups.
- `arnis-default-flat-roof`: non-residential/non-agricultural buildings without an explicit shape do not enter Arnis's auto-gable categories and fall back to flat.
- `arnis-school-institutional-bands`: the School preset uses regular windows, an accent roof line, a parapet, and `InstitutionalBands` facade depth.

These are stored as explicit `ArnisRuleDecision` records. They run on structured observations before schematic/world output; no generated Arnis world is reverse-extracted.

Field selection is evidence-driven rather than provider-order-only. Each selected field records its source observation, confidence, and quality score; contradictory candidates retain the same attribution. Arnis interpretation cannot override stronger reviewed manual evidence, and it only runs when an Overture or OSM/Overpass observation supplies raw geographic evidence. Derived observations are recorded separately as `arnis_derived`, while `usedSources` remains the ordered list of providers that participated in retrieval and merge.
