# Use Foundation Manifest Between Modes

Foundation Mode and Detailed Building Mode communicate through a `foundation_manifest.json` file rather than through the exported `.schem` alone or by re-reading map data. The manifest preserves editable intent such as Building Slots, Map Features, source confidence, block choices, coordinates, dimensions, and replacement policy, so Detailed Building Mode can refine a known slot instead of guessing structure from voxel output.
