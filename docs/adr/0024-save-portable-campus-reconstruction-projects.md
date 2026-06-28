# Save portable Campus Reconstruction Projects

## Status

Accepted

## Decision

The application saves each named Campus Reconstruction Project continuously and supports Save As so one Campus Target can have multiple independent reconstruction variants. A portable project export contains the project state plus the campus building names, suppressions, and source snapshots required to reproduce it. Importing a portable project creates a new local Campus Reconstruction Project rather than overwriting the current project. If the imported project belongs to a different Campus Target, the user must switch to that target or cancel. If the same campus already has a project with the imported name, the application renames the imported copy instead of replacing the existing project.

Each project retains the most recent complete candidate snapshot across all Candidate Confidence levels, including classification reasons, review status, provider or dataset versions, and spatial coverage results. Opening a project does not implicitly refresh external data; refresh is an explicit operation that presents source and geometry changes for review.

Project history records confirmed batch review changes atomically so autosave does not make a mistaken bulk accept or hide irreversible.

Autosave retains the most recent 50 semantic history operations across restarts, including boundary confirmation, batch review, human geometry edits, scale changes, and style changes. Continuous pointer movement is coalesced into one edit rather than recording every drag frame.

Important project decisions are autosaved into the active Campus Reconstruction Project after confirmation, including Campus Scale changes, Campus Boundary confirmation, Campus Orientation, candidate review, manual feature drawing, style choices, and provider snapshots. Before actions that can replace or leave the active project context, such as switching Campus Target or loading a different Campus Reconstruction Project, the application asks whether to explicitly save the current project snapshot first.

Portable projects record a project-schema version separately from their Minecraft target version. Older project schemas migrate with backup; unsupported newer schemas must not be guessed or partially imported. Minecraft version remains an independent compatibility input for block catalogs, generation rules, and schematic output.

A project remains pinned to its recorded Minecraft target version when opened or imported. Upgrading that target is an explicit migration preceded by a compatibility report for block identifiers and generation rules; the application never upgrades it silently.

## Consequences

- Foundation Manifest remains the handoff between reconstruction modes rather than becoming the project file.
- Project-local scale, orientation, boundary, feature review, human corrections, styles, and generation state do not leak between variants.
- Re-query review decisions remain project-local and require renewed review when the underlying source geometry changes materially.
- Building names and suppressions remain shared campus knowledge during normal work.
