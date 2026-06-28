# ADR-0023: Adapt Arnis surface rules as versioned Foundation Feature Generators

## Status

Accepted

## Decision

Road, water, vegetation, and sports output uses registered, versioned deterministic Foundation Feature Generators adapted from Arnis `highways`, `water_areas`, `landuse`, `natural`, and `leisure` rules rather than embedding its full world generator. Normal UI offers four built-in declarative Foundation Style Packs: Arnis Classic, Modern Campus, Historic Red-Brick Campus, and Lightweight Draft. Custom JSON import remains under Advanced and accepts only validated parameters; style packs cannot execute code, while new algorithms require developer-registered generator versions.

## Consequences

- Export provenance records generator identity, version, style pack, parameters, and deterministic seed.
- Custom styles remain portable and safe.
- The current single-material rasterizer becomes a fallback generator, not the quality target.
