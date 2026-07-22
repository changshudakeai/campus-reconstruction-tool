# Campus Reconstruction Tool V1.1.0 deployment

## Supported client

V1.1.0 is an online-required controlled release for Windows 11 x64. Install the
candidate executable recorded in
artifacts/candidates/<candidate-id>/distribution.json. The per-user NSIS package
installs:

- campus-native.exe, the Slint/Rust main process and schema-2 state owner;
- campus-map.exe, the isolated Gaode WebView2 helper;
- campus-preview.exe, the isolated native wgpu preview helper;
- third-party notices, unsigned/SmartScreen guidance, the exact candidate
  manifest, and the uninstaller.

No credentials, caches, fixtures, test projects, source tree, Node.js, Rust or
Python toolchain, model weights, or datasets are installed. Microsoft Edge
WebView2 Runtime is required by the Gaode helper.

Campus Target search plus new and refreshed Foundation acquisition require the
authenticated controlled service. Production uses only the compatible /v1
contract. Service outages pause acquisition and refresh without changing the
provider or pinned Dataset Bundle and without preventing work on persisted
projects and evidence.

The installer is per-user, requests no elevation, registers version 1.1.0 under
HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool,
and creates Start Menu shortcuts.

## Candidate production

Run from an exact clean commit:

    npm run candidate:v1.1 -- -CleanWindowsImageId "<clean-image-id>" -CleanWindowsImageManifest "<image-manifest.json>"

The image manifest must bind that ID to the current Windows build, an x64 clean
baseline assertion, the immutable VM/image source, and its snapshot SHA-256.
The candidate records both the snapshot digest and the manifest digest; a free
text image label alone is rejected.

The command checks formatting, warnings-denied Clippy, every Rust test, service
tests, and a locked workspace release build. It records the commit, clean source
state, operator-supplied clean Windows image identity, toolchains/dependency
inventory, commands, logs, exit states, test counts, and binary SHA-256. The
exact release binaries pass the three-process smoke before they are copied into
the packaging payload.

NSIS then consumes the copied tested-binaries payload without rebuilding it,
verifies every binary SHA-256 before and after packaging, and records the
installer size and exact SHA-256 in distribution.json and DISTRIBUTION.md.
Installer size is informational; V1.1.0 has no 50 MB gate.

Installed verification covers separate non-elevated silent-fresh,
interactive-fresh, and predecessor-upgrade scenarios. Every scenario verifies
the exact payload allowlist, 1.1.0 file and uninstall metadata, helper-process
handshake, first launch, normal shutdown, and complete uninstall. Final
candidate-evidence.json is sealed only after packaging and all three installed
acceptance records succeed.
The predecessor-upgrade scenario accepts only the frozen V1.0.1 Windows
installer baseline SHA-256. Its historical uninstall metadata reports
DisplayVersion 0.1.0; the evidence records both that legacy value and the pinned hash.

## Unsigned distribution

This candidate may be unsigned. Publish the exact installer SHA-256, clean source
commit, source statement, Windows 11 x64 and online-required boundaries, and the
SmartScreen instructions generated in DISTRIBUTION.md. Never direct users to
bypass SmartScreen when their SHA-256 differs.

## Local user data

Projects and generated snapshots live under
%LOCALAPPDATA%\CampusReconstructionTool. Credentials live in Windows Credential
Manager and never enter project files, candidate evidence, logs, or the
installer. Uninstalling the application does not silently delete user projects.
## Zero-waiver release gate

Before creating `v1.1.0`, follow `docs/releases/v1.1-release-gate.md` to seal and re-verify one candidate Evidence Bundle. The release tag command refuses candidates with missing evidence, installed-acceptance waivers, non-zero Release Blockers, digest drift, an unpinned service, incomplete sign-off, or a dirty/mismatched source commit.