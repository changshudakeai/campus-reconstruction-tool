# Foundation Mode Rebuild Blueprint

## Outcome

Rebuild Foundation Mode as a local-first, map-centric review workflow that
produces a portable reviewed campus model and deterministic Foundation export.
It retains its current role: establish the real-world campus extent, terrain
context, buildings, circulation, water, vegetation, and sports features that
anchor Detailed Building Mode.

Foundation Mode does not become an automatic map converter. Structured map data
and visual recovery are evidence; a user-reviewed campus model is the only
generation input.

## Product workflow

1. **Create or resume a Campus Reconstruction Project.** Startup presents two
   explicit choices: create a new project or resume a named project at its next
   incomplete task. A stale unnamed draft never silently becomes the landing
   screen. Confirm the Campus Target, Campus Scale, and Campus Orientation once
   for both modes.
2. **Confirm the Campus Boundary.** Propose a boundary from structured sources
   or draw it on the reference map. It stays editable until confirmation and
   gates all later retrieval.
3. **Acquire Foundation Source Snapshots.** Query source adapters by feature
   kind and spatial tile inside the confirmed boundary. A provider failure is
   visible and retryable without discarding usable snapshots from other
   providers.
4. **Review the map, not a queue of forms.** The Foundation Feature Review Map
   shows Buildings, Circulation, Water, Vegetation, and Sports as separately
   filterable layers. Map popups and keyboard/batch actions write decisions to
   the Foundation Review Ledger.
5. **Resolve duplicates and gaps.** Conflated candidates retain supporting
   provenance. A missing or wrong source shape is manually drawn as a new Map
   Feature rather than edited in place. Visual recovery remains a gap-filling
   source, never automatic truth.
6. **Generate from the reviewed campus model.** A selected Foundation Style
   Pack converts only reviewed features into a preview and `.schem`. Changing a
   style affects no geometry or review decision.
7. **Refresh safely.** A later retrieval creates a new Foundation Source
   Snapshot and proposes additions, removals, or material geometry changes for
   review. It never silently changes confirmed output.

## Logical modules

| Module | Owns | Does not own |
| --- | --- | --- |
| `Campus Scope Module` | Campus Target, Boundary, Scale, Orientation | Provider requests or feature styles |
| `Foundation Acquisition Module` | Tiled provider queries, Source Snapshots, deduplication inputs, retry state | Review decisions or Minecraft blocks |
| `Foundation Review Ledger` | Candidate acceptance, rejection, conflation, manual replacement, refresh differences | Provider implementation or UI map controls |
| `Reviewed Campus Model` | Reviewed Map Features and Building Slots | Raw provider response caches |
| `Foundation Style Module` | Foundation Style Packs and generator validation | Feature geometry or candidate review |
| `Foundation Compiler` | Deterministic voxel model from a Reviewed Campus Model plus Style Pack | Project persistence, provider state, or UI |
| `Foundation Workspace` | Map-first review and export intentions | Direct mutation of acquisition, ledger, or compiler internals |

## Rules that remain true

- The Campus Boundary is not a search radius and does not edit source geometry.
- Imported Map Candidate geometry is immutable; corrections create traceable
  manual Map Features.
- A Building Slot is created only from a reviewed Building with a resolved
  name; it remains the Detailed Building massing anchor.
- Confidence chooses a review presentation, never automatic confirmation.
- Source data, visual recovery, and manual geometry remain distinguishable in
  preview, export provenance, and later refreshes.
- Foundation Style Packs are declarative, versioned, and safe to import; they
  cannot run arbitrary code or modify the reviewed campus model.

## Foundation Workspace and action hierarchy

The selected native information architecture is a **Project Workbench** with
three stable regions: project tasks on the left, the single current task in the
centre, and current project context on the right. Internal workflow states do
not become navigation. The task rail uses user goals: select campus, confirm
scope, review Foundation, generate Foundation, and refine one building.

The current nine persistent step buttons and broad toolbar are replaced with a
single map workspace in three phases:

1. **Scope** — Campus Target, Campus Boundary, Scale, and Orientation. This
   phase exposes one primary action at a time: confirm the next missing scope
   fact.
2. **Review** — one map canvas with layer chips for Buildings, Circulation,
   Water, Vegetation, and Sports. Selecting a feature opens its review panel;
   candidate acceptance, rejection, conflation, and batch actions appear only
   in that panel. `Add missing feature` appears only after the active layer has
   been inspected.
3. **Generate** — Style Pack, preview, and export. These actions are unavailable
   while unresolved scope changes would invalidate the reviewed model.

Persistent chrome is limited to project state, undo/redo, save, the
Foundation/Detailed mode switch, and the phase progress indicator. Provider
diagnostics, credential configuration, style-pack import, rejected candidates,
and provenance live in an Advanced drawer rather than the primary toolbar.
The workspace always has one dominant next action; all other actions are either
contextual to a selected feature or progressively disclosed.

An incomplete phase cannot expose later-phase editors as if they were usable.
When a later mode depends on missing Foundation facts, it shows one blocking
reason and one action that returns to the missing task. Validation and recovery
messages appear on the same visible workspace that receives the action; a map
tool process must never report an error only to a window hidden behind it.

## Code disposition

| Current area | Decision | Target role |
| --- | --- | --- |
| `native/crates/campus-state` Foundation fields | Split and migrate | Campus Scope, Foundation Review Ledger, and Reviewed Campus Model schemas |
| `native/crates/campus-services` | Split | Provider adapters versus visual-recovery adapter; each owns its own failures and fixtures |
| `native/crates/campus-export` | Retain behind a narrower interface | Foundation Compiler output serializer, not a reader of the full Campus Project |
| `native/apps/campus-map` and protocol crate | Retain | Reference-map adapter and typed user intentions; no review/business rules |
| `native/apps/campus-native/src/main.rs` | Remove Foundation orchestration | Thin Foundation Workspace adapter only |
| Foundation Style Pack code | Retain and deepen | Style Module with validation and deterministic generator contracts |

## Persistence and refresh

Portable project data stores the confirmed Campus Scope, Foundation Review
Ledger, Reviewed Campus Model, selected Style Pack version, and content-addressed
Source Snapshot metadata. Raw downloaded datasets and generated previews are
reconstructable caches, not the only copy of a project decision.

When a source refresh differs materially from the snapshot behind a reviewed
feature, the ledger records a proposed difference. The previous reviewed
feature remains active until the user accepts the replacement; project history
supports undo of a whole batch decision.

## Migration

1. Convert existing candidates into the first Foundation Source Snapshot.
2. Convert review states, conflation choices, and manual Map Features into the
   Foundation Review Ledger.
3. Derive the first Reviewed Campus Model from those ledger entries and retain
   existing Building Slot IDs.
4. Preserve the selected Foundation Style Pack and export output as legacy
   artifacts; do not regenerate during migration.

## Delivery sequence

1. Establish Foundation domain schemas, ledger invariants, migration fixtures,
   and compiler contract while preserving legacy Foundation export fixtures.
2. Move provider retrieval into independent adapters and store Source Snapshots.
3. Rebuild the Foundation Workspace around map layers, review actions, and
   visible provider/retry status.
4. Move Style Packs behind the Style Module and compile from Reviewed Campus
   Model only.
5. Add safe refresh/difference review, portable-project recovery, and complete
   end-to-end export acceptance.

## Acceptance evidence

- A provider failure leaves successful feature layers reviewable and retryable.
- Refreshing a campus never changes a confirmed feature or export until a
  ledger difference is accepted.
- Rejecting a bad imported road and drawing its replacement preserves both
  source provenance and manual geometry lineage.
- A confirmed Building creates the same stable Building Slot ID before and
  after migration.
- Changing a Style Pack changes preview blocks but not candidate decisions,
  geometry, or Building Slot identity.
- Exporting the same Reviewed Campus Model and Style Pack produces identical
  voxel output across runs.
