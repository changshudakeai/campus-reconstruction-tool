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

- Added an explicit review-desk refresh control and end-to-end controlled-service delivery path. The service publishes an authoritative refresh-bundle precedence, and current evidence remains authoritative until the selected manifest and every chunk validate.
- Added upstream source-record identity, deterministic dependency/content digests, observation and coverage difference records, and containment-aware boundary expansion/shrink/relationship classification.
- Review operations now retain granular Review Dependency Basis data. Unchanged operations and completions are carried to the new basis with remapped version-qualified observation IDs; withdrawn cumulative state is omitted without blocking unrelated carry-forward, and conflict decisions invalidate by dependency type.
- Withdrawn observations and prior manifests remain in refresh history. Known Feature Gap acknowledgement, evidence-linked resolution, and reopening replay across Dataset Bundles without replacing earlier ledger history; changed resolution evidence automatically appends a reopen action.
- Generated and exported Foundation results retain only the dependency basis of selected generation inputs. Changed selected results move to durable stale history, remain inspectable, and cannot satisfy current formal export; semantically unchanged or rejected-only refreshes remain reusable.

## Verification

- `cargo +stable test --manifest-path native/Cargo.toml -p campus-state --test foundation_refresh` — 11 passed.
- `cargo +stable test --manifest-path native/Cargo.toml -p campus-services --test project_acquisition` — 2 passed.
- `cargo +stable clippy --manifest-path native/Cargo.toml --workspace --all-targets -- -D warnings` — passed with zero warnings.
- `cargo +stable test --manifest-path native/Cargo.toml --workspace` — passed across the full workspace.
