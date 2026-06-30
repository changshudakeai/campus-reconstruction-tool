# V1 functional parity matrix

Reference surface: the user-visible React/Tauri desktop workflow at the native
cutover commit. `Shipped` requires a native Slint control, Rust-owned state,
persistence where applicable, and an exercised result. `Partial` and `Gap`
block the atomic cutover required by ADR 0031.

| Area | User-visible capability | Native evidence | Status |
|---|---|---|---|
| Global | Create, open/import, save, and save-as portable projects | Slint toolbar callbacks; `DesktopApplicationState::{open,save_to}`; legacy V1 import test | Shipped |
| Global | Atomic autosave, recovery copy, undo/redo, Ctrl+S/Z/Y | `campus-state`; top-level Slint key bindings; round-trip tests | Shipped |
| Global | Gaode credentials in Windows Credential Manager | Map settings dialog and `keyring` Windows backend | Shipped |
| Global | Chinese/English language selection | Header selector updates the Rust-owned `DesktopLocale`; Slint static copy, AppState projections, review models, status feedback, Gaode helper, and native-preview title switch together, while the preference persists outside portable project data | Shipped |
| Foundation | Select a Campus Target using Gaode search/map evidence | The isolated Gaode helper provides POI search, result preview, and explicit confirmation; the project persists POI identity plus GCJ-02/WGS-84 lineage and reopens at the confirmed view | Shipped |
| Foundation | Draw, clear, save, and resume Campus Boundary | `campus-map` drawing toolbar; typed boundary event; autosaved project polygon | Shipped |
| Foundation | Review Campus Orientation and Campus Scale | Native orientation/scale controls persist project-wide values | Shipped |
| Foundation | Separate building, road, water, vegetation, and sports review steps | Nine-step Slint workflow and per-kind candidate projection | Shipped |
| Foundation | Candidate details, confidence queues, pagination, confirm/reject/revoke | Native detail dialog exposes source object ID/tags, confidence queues and pagination are AppState-owned, and confirm/reject/revoke are reversible | Shipped |
| Foundation | Batch accept/reject and batch-review undo | Current-kind pending candidates support one-operation accept/reject; global undo restores the batch snapshot | Shipped |
| Foundation | Human-drawn correction for missing/incorrect feature geometry | Map protocol v3 has a dedicated feature-drawing purpose for all five kinds; accepted manual geometry is separately traceable and manual buildings create valid Building Slots | Shipped |
| Foundation | Label-free visual capture and deterministic gap recovery | The map helper captures a label-free PNG independently from structured retrieval; protocol v3 returns it to Rust for deterministic color segmentation, WGS-84 georeferencing, evidence persistence, and human review | Shipped |
| Foundation | Campus Building Directory naming and suppression | Reviewed names propagate to features/slots; persistent suppression tombstones block rediscovery and remain recoverable; portable web directory records migrate | Shipped |
| Foundation | Four fixed Foundation style packs and advanced style-pack import | All four legacy packs persist full generator rules (road edges, water/sports borders, deterministic vegetation trees, density/seed/palette); Slint exposes validated JSON import using `foundation-style-pack.schema.json`, and preview/export share the same Rust generator | Shipped |
| Foundation | Generate and inspect native Foundation preview before export | Export model is converted to the native preview snapshot and launched through the preview helper | Shipped |
| Foundation | Sponge schematic plus portable project export | `campus-export` and Slint Foundation export action | Shipped |
| Detailed | Building Slot queue and selected-slot handoff | Slint slot combo; selected slot stored in project | Shipped |
| Detailed | Gaode 3D reference and open-geodata footprint comparison | Protocol v3 gives the isolated map process an explicit Building Evidence purpose, converts reviewed WGS-84 geometry to Gaode GCJ-02, centers on the selected slot, and hides campus-edit commands | Shipped |
| Detailed | Preserve measured footprint, height, floors, and roof | Native measurement editor; Arnis regression tests preserve massing | Shipped |
| Detailed | Complete fixed Arnis appearance categories | All 19 upstream categories are exposed in Slint and tested for distinct output | Shipped |
| Detailed | Generate a building and open native orbit/zoom preview | Arnis core generation; independent `campus-preview` process | Shipped |
| Detailed | Inspect selected block type and integer coordinates | Preview selection returns through typed IPC into Desktop Application State and a dedicated Slint summary | Shipped |
| Detailed | Batch palette replacement with validation | Native replacement fields and generated-model rewrite | Shipped |
| Detailed | Single-block editing and semantic feature preservation controls | Selected-coordinate editing and five semantic annotation types are native, persisted per refinement, provenance-recorded, and preserve the model envelope | Shipped |
| Detailed | Observed evidence and generated-interpretation comparison | Native evidence workspace shows source identity, measured/unknown massing, generated dimensions, generator, scale, roof/floors, block count, and correction count side by side | Shipped |
| Detailed | External-model and source-conflict review | 3DMR/Wikidata tags are retained from source objects; native review enforces adaptation/attribution rules, detects licensing and dimension conflicts, and persists reasoned decisions | Shipped |
| Detailed | Confirmed, versioned Building Slot refinements | Every generation creates a retained versioned draft; explicit confirmation archives the prior confirmed version and marks the slot refined | Shipped |
| Detailed | Export the edited Detailed building schematic | Native Detailed export action | Shipped |
| Packaging | Separate supervised map/preview processes with authenticated named-pipe IPC | Integration tests for both helper processes | Shipped |
| Packaging | Installer, uninstall registration, recovery/offline operation, and <50 MB size | Real per-user install/uninstall passed; installed payload and registry were verified; the installed binary completed 3 offline cycles (57 Arnis generations), all 4 Foundation packs, five feature kinds, schematic exports, and corrupt-primary recovery; final installer is 6.17 MB | Shipped |

## Explicitly outside V1 parity

- Photo-trained template matching, contributed training crops, and model
  training are V2 work and were not reachable V1 controls at cutover.
- Permanently hidden branches, obsolete debug-only panels, and the future
  Cloud Web Companion are not desktop parity requirements.

## Cutover rule

The native entry point may be proposed in a draft branch while this matrix has
`Partial` or `Gap` rows. It must not be merged as the atomic product cutover
until every in-scope row is `Shipped` and the final interactive acceptance
evidence named in ADR 0031 is recorded.
