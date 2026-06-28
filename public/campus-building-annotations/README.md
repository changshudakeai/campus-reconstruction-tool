# Campus Building Annotations

Version-controlled, campus-scoped mappings from traceable OSM/Overture source IDs to human-reviewed building names.

- `index.json` maps campus canonical names and aliases to annotation files.
- Each campus file uses schema version `0.1.0` and contains `sourceId`, `name`, source coordinates, naming source, update time, and optional inclusion/exclusion classification.
- Deleted candidates are stored in `suppressedBuildings` as source-identity tombstones. They are omitted from future loads and are not exported as named buildings.
- Local user edits override bundled shared annotations. Future GitHub-hosted files can be loaded by setting `VITE_CAMPUS_ANNOTATION_BASE_URL`.
