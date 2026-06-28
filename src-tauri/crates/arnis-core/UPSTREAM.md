# Arnis upstream record

- Project: https://github.com/louis-e/arnis
- Upstream commit: `7d2a0ebed00f0b023a4bb8238ea7cbe9d35aa148`
- Upstream version: `2.9.0`
- License: Apache License 2.0; see `LICENSE`
- Copyright: 2022-2026 Louis Erbkamm (louis-e)

This vendored fork extracts and modifies the coordinate, deterministic generation,
building, roof, and block-placement logic for an in-memory single-building pipeline.
It intentionally excludes Arnis GUI, CLI, terrain generation, and Minecraft world writers.

Modified files carry a notice in their module documentation. Unmodified extracted files
retain their upstream comments and implementation.

Exact upstream snapshots used during extraction are retained under `upstream-reference/`:
`osm_parser.rs`, `overture.rs`, and `element_processing/buildings.rs`. Runtime code in
`src/lib.rs` adapts their single-building data model, polygon handling, roof dispatch,
deterministic generation, and block-placement behavior to the `BlockSink` interface.
