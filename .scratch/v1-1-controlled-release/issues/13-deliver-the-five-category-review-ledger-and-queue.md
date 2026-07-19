# 13 — Deliver the five-category review ledger and queue

**What to build:** Let a user complete Buildings, Circulation, Water, Vegetation, and Sports through the selected list-first queue, with atomic review operations, real overlap semantics, acknowledged gaps, deterministic projection, and next-incomplete resume.

**Blocked by:** 11 — Review complex typed evidence as Building Entities; 12 — Deliver the automatic Boundary evidence desk.

**Status:** completed

- [x] Five category tabs expose independent progress, terminal acquisition state, pending blockers, and explicit completion.
- [x] The queue is the primary work surface; the coordinated map and evidence panel expose spatial context, multidimensional assessment, lineage, provenance, provider state, and actions.
- [x] Source observations remain immutable while the append-only Ledger records accept, reject, revoke, grouping, containment, naming, repair, attribute, gap, and completion decisions.
- [x] Circulation and linear water preserve centreline/area form, subtype, and width provenance; continuous review geometry is traceably clipped.
- [x] Sports containers, tracks, pitches, and sports-hall Buildings retain correct containment and generation semantics.
- [x] Batch accept/reject prevalidates an exact subject set, commits all-or-nothing, and creates one save/history unit without completing a category.
- [x] Completion is enabled only when every candidate has a disposition, conflicts are resolved, and remaining Known Feature Gaps are acknowledged.
- [x] A Deferred Source Observation requires a structured reason and linked acknowledged gap and never enters the Reviewed Campus Model.
- [x] Reopening selects the earliest incomplete category while completed categories remain inspectable without reopening.
- [x] No five-category blank-canvas drawing or screenshot-recovery action exists in the production workflow.

## Implementation notes

- Added a public, deterministic Ledger-to-queue projection for all five categories, including immutable source geometry, derived review geometry, assessment, lineage, provenance, provider outcomes, conflicts, Known Feature Gaps, and per-category progress.
- Added typed, append-only review operations for individual and exact-set batch dispositions, scoped revocation, gap acknowledgement/reopening, conflict declaration/resolution, and explicit completion. Every operation records its exact subjects, dependency basis, before/after state, optional explanation, and time. The schema distinguishes future evidence-linked gap resolution from acknowledgement; ticket 14 will connect resolution to refresh provenance. Later semantic operations invalidate the previous completion and resume at the earliest incomplete category.
- Routed every mutating desktop review event through one project semantic operation so an exact-set batch produces one durable save/history unit.
- Added the selected list-first review desk with coordinated spatial context and evidence/actions. Production drawing, screenshot capture, and visual-recovery commands were removed from this workflow.
- Routed Building candidates and exact-set groups through the stable Building Entity ledger, with unnamed entities represented as spatial, acknowledged gaps instead of invented names. Preserved sports containment/non-generating-container behavior.
- Bound replay to the current Review Dependency Basis, generated conflicts from retained acquisition suggestions, and projected reviewed naming, repair, and attribute decisions without mutating Source Observations.

## Verification

- `cargo +stable test --manifest-path native/Cargo.toml -p campus-state --test foundation_review_queue` — 10 passed.
- `cargo +stable clippy --manifest-path native/Cargo.toml --workspace --all-targets -- -D warnings` — passed with zero warnings.
- `cargo +stable test --manifest-path native/Cargo.toml --workspace` — passed across the full workspace.
