# Save portable Campus Reconstruction Projects

## Status

Accepted

## Decision

The application manages Campus Reconstruction Projects in a local, Campus Target-scoped library. Every project has an immutable Project ID independent of its editable name, internal path, and portable-export filename. Project names are unique within one Campus Target: creation waits for a unique name, while an import conflict receives an `（导入 N）` suffix and never overwrites an existing project.

Each project retains the most recent complete candidate snapshot across all Candidate Confidence levels, including classification reasons, review status, provider or dataset versions, and spatial coverage results. Opening a project does not implicitly refresh external data; refresh is an explicit operation that presents source and geometry changes for review.

Confirmed semantic operations create atomic Project Save Points. A completed boundary drag or confirmed batch review is one operation; pointer frames and incomplete text input are not. `Ctrl+S` requests an immediate save point. Each project persists its latest 50 semantic undo/redo operations across restarts, and a new operation clears the redo branch.

The active project surface continuously exposes `saving`, `saved` with completion time, or `save failed` with the reason and a retry action. The Campus Project Library exposes each project's latest successful save time. A Project Context Change—switching campus or project, creating or importing a project, or normally exiting—proceeds only after the active project is durably saved. A failed save cancels the requested action and leaves the current project and its unsaved in-memory state active; the normal path never silently discards and continues.

Each project retains one previous confirmed save point for rollback and one latest validated Project Recovery State after an unclean exit. If recovery is newer than the confirmed project, startup presents its time and recent operations. Recovery opens without overwriting the confirmed project and becomes confirmed only after an explicit save; it is cleared after that save or after the user explicitly keeps the last confirmed version.

Opening an existing project resumes at its next incomplete required task, skipping completed tasks while keeping them reviewable. A fully complete project opens at its completion summary and export surface rather than replaying the workflow.

A Portable Project is a complete editable snapshot sufficient to continue review, undo/redo, generation, and export on another computer. It includes Campus Target evidence, confirmed boundary, workflow state, scale, orientation, five-layer source snapshots and provenance, review ledger, known gaps, campus names and suppressions, style and generation parameters, the 50-operation history, schema version, and schematic compatibility profile. It excludes credentials, application settings, absolute machine paths, logs, caches, previews, and regenerable `.schem` files.

Portable Project Export writes and validates a temporary artifact before atomically committing it, requires explicit confirmation before replacing an existing destination, and never changes the active project's identity or managed save destination. Failure or cancellation leaves both the current project and any existing destination unchanged.

Portable Project Import validates and migrates in a temporary area before creating a new local Project ID and recording source lineage; the source artifact remains unchanged. An identical stored Gaode POI ID automatically matches the selected Campus Target. Otherwise the application shows the two campus names and map locations for human confirmation and never merges by name or proximity alone. Importing a project for another campus requires explicit confirmation, a successful save of the active project, and then commits the new project under its embedded campus and switches to that campus. Any failure cancels the whole import.

Portable projects record a project-schema version separately from their schematic compatibility profile. Supported older imports migrate transactionally from a validated temporary copy. Existing local projects migrate transactionally from a backup on first open after an application upgrade, so one failed migration does not block the library or other projects. Unsupported newer schemas are rejected rather than guessed or partially imported.

V1.1 has one release-wide Schematic Compatibility Profile: Minecraft Java Edition 26.1.2. The preview, Minecraft Block Catalog, generation rules, and Sponge `.schem` exporter all use that same profile for Axiom compatibility. V1.1 does not expose a per-project Minecraft version selector. V2 may add selectable Minecraft Compatibility Profiles and trusted block-catalog download or import, but their packaging and migration rules require a separate V2 decision.

## Consequences

- Foundation Manifest remains the handoff between reconstruction modes rather than becoming the project file.
- Project-local scale, orientation, boundary, feature review, human corrections, styles, and generation state do not leak between variants.
- Re-query review decisions remain project-local and require renewed review when the underlying source geometry changes materially.
- Building names and suppressions remain shared campus knowledge during normal work.
- The old `Save As` mental model is replaced by Portable Project Export; external file paths never become the active managed project location.
