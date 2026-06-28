# Build Minimal Arnis Adapter First

The First Vertical Slice will implement a Minimal Arnis Adapter inside this project before forking or modifying Arnis itself. The adapter should focus only on the data and interpretation needed for the Putuo Campus Library, keeping the workflow small enough to validate before taking on Arnis's full Rust, Tauri, and world-generation architecture.

## Superseded for building generation

The visual-checkpoint rejection showed that the Minimal Arnis Adapter could silently reuse placeholder geometry and that its TypeScript generator did not provide Arnis behavior. Building acquisition and generation now use the vendored `arnis-core` Rust adapter pinned in `src-tauri/crates/arnis-core/UPSTREAM.md`. The Minimal Arnis Adapter remains only for explicitly labeled offline fixtures during migration.
