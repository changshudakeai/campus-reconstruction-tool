# Supersede V1 desktop release assumptions for V1.1

The accepted V1.1 controlled-release plan changes a small set of packaging and support assumptions made in ADR 0031. This decision supersedes ADR 0031 only where the statements below conflict.

V1.1 is supported and acceptance-tested on a clean Windows 11 x64 image. Windows 10 is no longer part of the V1.1 release gate. The exact Windows image identity and build must be recorded in candidate evidence.

V1.1 requires online access for supported acquisition and map-review flows. Offline operation is not a V1.1 acceptance gate. Network and provider failures must still be explicit, recoverable, and must not corrupt persisted project state.

Installer size is recorded as release information rather than constrained by the earlier 50 MB budget. Payload allowlisting, absence of credentials, caches, fixtures, and toolchains, and digest verification remain mandatory.

Portable Export is a project-context action. It may appear on a Campus Project Library row and in the completed project workspace because it exports the selected persisted project. This does not restore the legacy Save As flow and does not permit direct provider calls or ad-hoc export paths.

The public tag and release are created only after the controlled candidate evidence and sign-off are sealed. A tag-triggered workflow must not build, test, or publish a candidate that has not already passed that gate.

This decision does not supersede the native-shell architecture, isolated typed tool processes, single Rust-owned project state, atomic product cutover, or functional-parity requirements in ADR 0031. In particular, both Foundation and Detailed reconstruction modes, persistence, map review, generation, native preview, editing, and export remain release requirements.

## Consequences

- Candidate automation must prove Windows 11 x64 and an operator-supplied clean-image identity.
- Packaging consumes binaries that were already tested in their release form and records hashes before and after packaging.
- Interactive fresh install, silent fresh install, predecessor upgrade, first launch, three-process startup, normal shutdown, and uninstall are distinct evidence scenarios.
- Source and binary gates may run before tagging, but they cannot publish a GitHub release.
