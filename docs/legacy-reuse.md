# Legacy Prototype Reuse

The first prototype remains prior art, not a runtime dependency of the new Tauri app.

## Reused Decisions

| Proven prototype capability | Legacy evidence | New implementation |
| --- | --- | --- |
| Sponge v2 `.schem` output | `../ecnu-mc-replication/cli/mc_build_tools/schematic.py` | `src/services/spongeSchematic.ts` |
| Named Minecraft block palettes | `../ecnu-mc-replication/data/block_colors.json` | `src/domain/schematicModel.ts` |
| Three.js building preview | `../ecnu-mc-replication/web/js/scene.js` | `src/components/SchematicPreviewer.tsx` |
| Campus foundation generation | `../ecnu-mc-replication/cli/mc_build_tools/foundation.py` | `src/services/foundationManifestToSchematic.ts` |
| Building candidate and manifest concepts | `../ecnu-mc-replication/data/manifest.json` | `src/domain/foundationManifest.ts` |

## Deliberate Replacements

- Python/browser-global state was replaced by typed React state and TypeScript domain contracts.
- Building specs are not treated as the primary geometry source; the Minimal Arnis Adapter uses Overture, OSM/Overpass, reviewed Manifest data, and manual correction.
- The old page collection was replaced by one Modern App Shell organized around Foundation Mode and Detailed Building Mode.
- Generated world-file reverse extraction was not reused because Building Geometry is the explicit handoff before schematic generation.

This approach preserves proven behavior without coupling the new app to legacy runtime code or low-quality cached geometry.
