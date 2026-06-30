# Campus Reconstruction Tool V1 deployment

## Windows client

Install `artifacts/installer/Campus-Reconstruction-Tool-V1-Setup.exe` on
Windows 10/11 x64. The per-user NSIS package installs:

- `campus-native.exe` — Slint/Rust main application and sole AppState owner;
- `campus-map.exe` — isolated Gaode WebView2 helper;
- `campus-preview.exe` — isolated native wgpu preview helper;
- `THIRD_PARTY_NOTICES.md` and the uninstaller.

No Node.js, Rust toolchain, Python runtime, source tree, model weights, or
dataset is installed. Microsoft Edge WebView2 Runtime is required only when
opening the Gaode helper and is normally present on supported Windows systems.
Existing projects, generation, preview, editing, and export remain available
offline.

The installer registers a per-user uninstaller under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\CampusReconstructionTool`
and creates Start Menu shortcuts.

## Release verification

From the repository root:

```powershell
npm run bundle:v1
npm run verify:installer
npm run size:release
```

`verify:installer` performs a real silent installation, validates the exact
payload and uninstall registration, runs the installed binary's offline
multi-cycle self-test (project recovery, all four Foundation packs, all 19
Arnis categories, Foundation and Detailed schematic export), and then performs
a real silent uninstall.

## Local user data

Projects and generated snapshots live under
`%LOCALAPPDATA%\CampusReconstructionTool`. Gaode credentials live in Windows
Credential Manager. Language preference is application-local; portable
project JSON remains language-neutral. Export a portable project when moving
work between computers.

## Repository-only and generated files

Source (`native/`, `src/`, `src-tauri/`, `scripts/`, `docs/`) is not deployed.
Build outputs under `native/target/`, `dist/`, and `artifacts/` are
regenerable and are not application runtime dependencies.
