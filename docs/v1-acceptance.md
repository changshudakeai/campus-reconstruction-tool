# V1 acceptance record

Date: 2026-06-29  
Platform: Windows 11 x64 (Windows 10/11 x64 target)

## Functional

- Native Slint main application launches from the release binary.
- Main state owns project mode, nine-step workflow, reviews, map camera, boundary, measurements, template selection, and generated paths.
- Map and preview helpers authenticate with random session tokens over versioned length-prefixed named-pipe JSON.
- Map camera and capture bounds persist in the project.
- Map boundary drawing and semi-automatic “adjust view, then capture and identify” flow are present.
- OSM/Overpass covers building, road, water, vegetation, and sports candidates.
- Optional hosted Overture data merges building candidates without adding Python or datasets to the installer.
- Foundation export produces gzip-compressed Sponge V3 NBT plus project JSON.
- Detailed generation preserves measured footprint, height, floor count, and roof input while applying one of 19 Arnis appearance categories.
- Detailed export produces gzip-compressed Sponge V3 NBT.
- Native preview supports orbit/zoom and reports the selected block type and integer coordinates.
- Generated palettes support validated batch replacement before preview/export.
- Old portable web V1 projects import into the native project model.
- Autosave uses same-directory atomic replacement and retains a recovery copy.
- Undo, redo, Ctrl+S, Ctrl+Z, and Ctrl+Y work from the top-level workbench.

## Automated verification

- Native workspace tests: pass.
- Arnis core tests: 12 pass, 1 documentation example intentionally ignored.
- Map helper process IPC integration: pass.
- Preview helper process IPC integration: pass.
- Foundation Sponge V3 export test: pass.
- Portable web V1 import test: pass.
- OSM and Overture parser tests: pass.
- Detailed fixture generation test: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- Arnis core `cargo clippy --all-targets -- -D warnings`: pass.
- Legacy/future cloud web reference production build: pass.
- Legacy/future cloud web reference offline smoke contracts: 50 pass.

## Packaging

- Release executable payload: 16.15 MB.
- NSIS installer: 5.97 MB.
- V1 installer budget: 50 MB — pass.
- Rust `target/`, toolchains, Node.js, Python, Overture cache, datasets, and model weights: excluded.
- Installer: `artifacts/installer/Campus-Reconstruction-Tool-V1-Setup.exe`.
- Installer SHA-256: `BC1DDD72919F9BCD565AE7DA57E3CF6EC26E247F0AF77FD530F966A8ADC04964`.
