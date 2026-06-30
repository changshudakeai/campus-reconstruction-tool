# V1 acceptance record

Date: 2026-06-30
Platform: Windows 11 x64 (Windows 10/11 x64 target)

## Functional parity and runtime

- Every in-scope row in `v1-functional-parity.md` is `Shipped`.
- `campus-native.exe` is the only desktop entry and owns one
  `DesktopApplicationState`; `npm run dev`, `npm run build`, and
  `scripts/start-desktop.ps1` all resolve to it.
- Foundation provides campus POI confirmation, GCJ-02/WGS-84 lineage,
  boundary/orientation/scale, five review layers, confidence queues, batch
  review, manual geometry, deterministic visual recovery, naming/suppression,
  four complete generator packs, validated custom packs, native preview, and
  portable project plus Sponge V3 export.
- Detailed provides reviewed Building Slots, Gaode evidence overlay, all 19
  fixed Arnis appearance categories, measured geometry preservation,
  versioned refinements, native preview selection, single/batch block edits,
  semantic feature preservation, external-model licensing/source-conflict
  review, and Sponge V3 export.
- Main, Gaode, and preview processes use authenticated versioned named-pipe
  IPC. Tool processes do not own project state.
- Atomic save creates a recovery copy. Opening a corrupt primary project now
  automatically restores the previous valid copy and marks it dirty for
  repair.

## Automated verification

- Native workspace: 19 tests pass, including map/preview IPC, recovery,
  portable-project migration, deterministic screenshot analysis, both
  schematic exporters, and Detailed generation.
- Native clippy: `--workspace --all-targets -- -D warnings` passes.
- Arnis core: 12 tests pass; one documentation example is intentionally
  ignored. Arnis clippy passes with `-D warnings`.
- Web-reference production build passes.
- Legacy/cloud-reference offline contract suite: all 50 pass.
- GitHub `native-v1` and `web-reference` checks pass on the cutover branch
  before final release evidence; final commit must pass the same required
  checks before merge.

## Installed extended workflow

`scripts/verify-native-installer.ps1 -Cycles 3` performed a real user-level
silent installation and uninstall:

- exact five-file installed payload verified;
- uninstall registry and install location verified;
- installed executable ran without network access;
- 华东师范大学普陀校区 coordinates and all five Foundation feature kinds
  generated and exported;
- all four Foundation packs generated;
- all 19 Arnis categories generated for three cycles (57 generations);
- Foundation and Detailed `.schem` outputs were non-empty;
- deliberately corrupted primary project recovered from the atomic backup;
- silent uninstall removed payload and uninstall registration.

No blocking crash occurred during this multi-cycle workflow.

## Interactive Windows UI acceptance

The optimized release binary was launched and captured through Windows
Graphics Capture:

- the 1280×820 Slint workbench rendered cleanly without clipping at the tested
  window size;
- Chinese → English switched the toolbar, Foundation workflow, Detailed
  controls, models, summaries, and actions without changing project state;
- Foundation Campus and Export surfaces were reachable;
- the advanced JSON style-pack action was visible in the stable export action
  group;
- Detailed controls and all primary editing/export actions were reachable;
- with no Gaode credential present, the map action correctly opened the
  bilingual secure-credential dialog and did not launch an unauthenticated
  helper.

Map rendering with a production Gaode key is credential-dependent; the helper
process, authenticated pipe handshake, purposes, event round trips, and
credential gate are covered by automated tests and the interactive gate check.

## Packaging and size

- Release executable payload: 16.99 MB.
- NSIS installer: 6.17 MB.
- V1 installer budget: 50 MB — pass (12.3% of budget).
- Installer:
  `artifacts/installer/Campus-Reconstruction-Tool-V1-Setup.exe`.
- SHA-256:
  `5B3D9F4B051553D648A450BBC7486C662BC28590335DBE091C58B80C23B6C6D6`.
- Rust caches, toolchains, Node.js, Python, Overture cache, datasets, and model
  weights are excluded.
