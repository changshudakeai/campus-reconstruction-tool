# Campus Reconstruction Tool deployment notes

## Client deployment

Use the Windows x64 installer produced by Tauri:

- `Campus Reconstruction Tool_0.1.0_x64-setup.exe`
- Optional enterprise installer: `Campus Reconstruction Tool_0.1.0_x64_en-US.msi`

The installer includes the compiled desktop application and bundled web assets. The target computer needs Microsoft Edge WebView2 Runtime, which is normally present on modern Windows. No Node.js, Rust, source files, or `node_modules` are required on the client machine.

## Repository-only files

Keep these in GitHub/source control, not in the client deployment package:

- `src/`, `src-tauri/`, `scripts/`
- `package.json`, `package-lock.json`
- `docs/`, ADRs, smoke tests
- `public/` source assets

## Generated files not meant for source control

These are build outputs and can be regenerated:

- `dist/`
- `src-tauri/target/`
- `.scratch/`

## Local user data

Campus projects, cached reverse-geocode attempts, and local review state are stored on the user machine by the desktop app. They are not shipped with the installer. Export portable Campus Reconstruction Projects from inside the app when moving work between computers.
