# Use Building Geometry as Adapter Output

The Minimal Arnis Adapter outputs structured Building Geometry rather than `.schem` files. This keeps data acquisition and interpretation separate from Detailed Building Mode, which is responsible for turning Building Geometry into an editable and exportable Axiom-Compatible Schematic.

## Amendment: Rust generation output

The Arnis Rust Core Adapter keeps Building Geometry as its review interface, then accepts the reviewed/corrected Building Geometry and returns a palette plus deterministic RLE block buffer. It does not write a Minecraft world or `.schem`. Detailed Building Mode remains responsible for inspection, replacement, visual review, and Axiom-Compatible Schematic export.
