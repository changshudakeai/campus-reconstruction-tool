# 14 — Deliver explicit refresh and dependency-local invalidation

**What to build:** Let a user request newer controlled evidence, understand exactly what changed, preserve unaffected review, and reopen only decisions and outputs whose dependencies actually changed.

**Blocked by:** 13 — Deliver the five-category review ledger and queue.

**Status:** completed

- [x] Opening a project never refreshes evidence implicitly; refresh is an explicit user operation producing a new pinned Dataset Bundle.
- [x] Refresh classifies unchanged, added, changed, withdrawn, and coverage-changed observations with stable identity and digests.
- [x] Unchanged review decisions carry forward; changed dependencies reopen only affected ledger decisions and generated results.
- [x] Withdrawn evidence cannot silently delete a confirmed feature and remains reviewable with its prior lineage.
- [x] Every decision and generation result retains a Review Dependency Basis sufficient to explain invalidation.
- [x] Boundary shrink reassesses removed or relationship-changed objects, while expansion acquires and reviews only the added area.
- [x] Unaffected categories and decisions remain complete, and viewing completed work alone does not reopen it.
- [x] Known Feature Gap resolution and reopening preserve evidence-linked history rather than replacing prior acknowledgement.
- [x] Stale previews and schematics remain traceable/read-only and cannot satisfy current formal export.
- [x] End-to-end fixtures demonstrate local invalidation for geometry, grouping, naming, attribute, containment, licence, coverage, and rule-version changes.

## Implementation notes

- Added an explicit controlled-service refresh request that negotiates a different immutable Dataset Bundle while keeping current evidence authoritative until the new manifest and all chunks validate.
- Added stable source-record identity, deterministic dependency/content digests, observation and coverage difference records, and boundary expansion/shrink classification.
- Review operations now retain granular Review Dependency Basis data. Unchanged operations and completions are carried to the new basis with remapped version-qualified observation IDs; only subject-, coverage-, boundary-, or rule-linked work reopens.
- Withdrawn observations and prior manifests remain in refresh history. Known Feature Gap acknowledgement, evidence-linked resolution, and reopening replay across Dataset Bundles without replacing earlier ledger history.
- Generated and exported Foundation results retain their dependency basis. Changed results move to durable stale history, remain inspectable, and cannot satisfy current formal export; semantically unchanged refreshes remain formally reusable.

## Verification

- `cargo +stable test --manifest-path native/Cargo.toml -p campus-state --test foundation_refresh` — 8 passed.
- `cargo +stable test --manifest-path native/Cargo.toml -p campus-services --test project_acquisition` — 2 passed.
- `cargo +stable clippy --manifest-path native/Cargo.toml --workspace --all-targets -- -D warnings` — passed with zero warnings.
- `cargo +stable test --manifest-path native/Cargo.toml --workspace` — passed across the full workspace.
