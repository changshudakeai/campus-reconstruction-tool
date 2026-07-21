# Detailed Building Mode Rebuild Blueprint

## Outcome

Rebuild Detailed Building Mode as a local-first evidence-to-rules workflow.
Foundation Mode retains its present purpose and exports; its only Detailed
Building responsibility is to provide a Reviewed Building Slot with stable
massing, scale, orientation, identity, and location.

Detailed Building Mode turns local photographs and a selected Parametric
Building Template into an editable Detailed Building Rule Stack, then generates
a versioned Minecraft exterior. It does not treat a photo, template, or AI
prediction as final truth.

## Product workflow

0. **Enter only from a Building Slot.** When Foundation Mode has no Reviewed
   Building Slot, Detailed Building Mode is a blocked handoff with one action:
   return to building review. It does not render the editor, accept photos, or
   expose preview and export controls.
1. **Select a Building Slot.** Its footprint, position, measured height, and
   floor count are the massing anchor and cannot be changed by templates or
   photo recognition.
2. **Classify without a form.** Infer Building Function Classification from
   the name, map tags, POI/campus-directory data, and available photos. A
   high-confidence result is an editable label; only low-confidence or
   conflicting evidence requests correction.
3. **Start with evidence or a template.** Users may add local photographs and
   create Visual Evidence Crops, or begin with no photos. In the latter case,
   the mode creates a Template-Provisional Detailed Building.
4. **Propose, never silently apply.** Rank at most three Template Match
   Proposals. A confirmed project-local template from the same Building
   Function Classification ranks above catalog templates and the generic Arnis
   Style Presets. The user explicitly selects one.
5. **Build the Facade Reconstruction Draft.** The Local Facade Reconstruction
   Model proposes floors, bays, openings, facade features, roof candidates, and
   material labels with confidence and evidence links.
6. **Review differences non-destructively.** The selected template is the base
   layer. Automated drafts and later photos create differences; accepted photo
   or manual changes are higher rule layers. Existing accepted rules are never
   overwritten.
7. **Generate, compare, and confirm.** A deterministic compiler converts the
   effective rule stack into Minecraft blocks. The user compares reference
   evidence and preview, retains versions, then confirms or exports. A
   template-provisional export remains visibly provisional rather than a
   complete refinement.

The workspace reveals only the current task and its immediate evidence. It
does not show measurements, classification, template parameters, photo import,
and export as one long form. Export is unavailable until a generated revision
exists, and user-facing failures are localized consistently with a separate
expandable diagnostic detail.

Detailed Building uses the same selected **Project Workbench** structure as
Foundation: the left task rail shows the Building Slot handoff state, the
centre shows exactly one evidence-to-rules task, and the right context panel
shows the selected Building Slot and one primary action. The former all-in-one
editor is not retained.

## Logical modules

| Module | Owns | Does not own |
| --- | --- | --- |
| `Detailed Building Repository` | Slot-specific revisions, rule stacks, local evidence references, provenance | Campus map discovery or global UI state |
| `Evidence Library` | Local photos, Visual Evidence Crops, calibration, evidence links | Template selection or block generation |
| `Building Function Classifier` | Ranked use classification and reasons | Building identity or template application |
| `Template Catalog` | Bundled, project-local, and selected Parametric Building Templates; provenance and licence metadata | Photo recognition or mutable generated blocks |
| `Facade Reconstruction Model` | Local inference and Facade Reconstruction Drafts | Final generation decisions |
| `Rule Stack Resolver` | Ordered template, draft, and accepted overrides; difference proposals | Image storage or voxel rendering |
| `Detailed Building Compiler` | Deterministic voxel model from a resolved rule stack | Evidence ranking, UI workflow, or persistence policy |
| `Detailed Building Workspace` | Presentation, previews, selection, and user intentions | Direct mutation of persistence or generation internals |

The current Arnis exterior implementation becomes a compiler adapter and source
of the 19 initial base template families. It must not remain the Detailed
Building data model or the template catalog.

## Code disposition

| Current area | Decision | Target role |
| --- | --- | --- |
| `native/crates/campus-state` Detailed Building records | Replace incrementally | Focused Detailed Building Repository schema and migrations |
| `native/crates/arnis-core` | Retain behind an adapter | Initial base-template-family compiler; add upstream-conformance fixtures before claiming fidelity |
| `native/crates/campus-export` | Retain | Serialize the compiler's voxel model; do not read the whole Campus Project |
| `native/apps/campus-preview` and protocol crate | Retain | Isolated renderer for versioned voxel-model snapshots |
| `native/apps/campus-map` | Retain for Foundation | Campus map and Building Slot evidence only; no Detailed Building domain rules |
| `native/apps/campus-native/src/main.rs` | Split and replace by seam | Thin workspace adapter that sends intentions to focused modules |
| `native/apps/campus-native/ui/app.slint` | Rebuild Detailed workspace | Evidence, proposals, differences, preview, and revision surfaces; keep Foundation flow separate |

## Developer feedback loop

Development must never depend on the installer or silently launch a stale
release executable. Provide three explicit commands:

- `dev` supervises the debug main process and its map/preview helpers, rebuilds
  changed Rust/Slint code, and restarts only the affected process;
- `run-installed` starts the installed/release application only when explicitly
  requested;
- `package` creates the installer from a clean release build.

The helpers continue to use typed IPC, but their protocol is versioned and
their launch paths are injected by the development supervisor rather than
discovered through a release-directory probe. Fast module tests and fixture
contracts run independently of the desktop process.

## Persistence and provenance

Portable project data stores relative evidence references or content hashes,
not machine-specific generated paths. Each revision records:

- Building Slot ID and immutable massing reference;
- selected template and version;
- Building Function Classification and evidence/reasons;
- model version, Facade Reconstruction Draft, and confidence;
- ordered accepted/rejected/deferred differences;
- deterministic compiler version and generated artifact hash.

Generated previews and `.schem` files are disposable derived artifacts. The
rule stack and evidence lineage are the durable source of a Detailed Building.

## Local model lifecycle

- Distribute a versioned Local Facade Reconstruction Model for offline
  inference with the desktop application.
- Train or fine-tune it only in the controlled local workstation/private-server
  environment, not on end-user devices.
- Retain all photos locally by default. Explicit, rights-cleared contribution
  of selected training evidence is an upgrade item, not a first-release path.
- Validate each shipped model against a held-out facade dataset and recorded
  failure cases before making it the default.

## Migration

1. Preserve each existing Arnis Style Preset as an initial base template family.
2. Convert a legacy Detailed Building draft into a legacy rule-stack revision:
   preset, wall override, window density, wall depth, and semantic edits become
   explicit layers.
3. Keep legacy generated files readable as artifacts, but do not persist new
   absolute generated paths in portable projects.
4. Migrate confirmed existing refinements as confirmed revisions; do not rerun
   generation during migration.

## Delivery sequence

1. Establish the new domain crates, JSON migration, compiler contract, and
   fixture-driven regression tests without changing Foundation output.
2. Move Arnis presets behind the Template Catalog and build zero-photo,
   template-provisional generation with versioned rule stacks.
3. Add Evidence Library, automatic Building Function Classification, and
   explicit Template Match Proposals.
4. Add local model inference and editable Facade Reconstruction Drafts.
5. Add evidence-to-difference review, reference/preview comparison, and
   complete migration/portable-project acceptance tests.

## Acceptance evidence

- A user can complete a Detailed Building with no photos and see its provisional
  provenance in preview and export.
- A user can add photos later and accept one difference without losing a prior
  accepted edit.
- Two teaching buildings in one project rank a confirmed project-local template
  above generic templates, but still require explicit selection.
- A low-confidence or contradictory Building Function Classification does not
  silently select a template.
- Opening a portable project on a second computer preserves the rule stack and
  evidence lineage without requiring old artifact paths.
- Foundation Mode generation remains byte-for-byte stable for existing fixtures.
