# Apply photo evidence through scoped rule overrides

Detailed Building Mode retains each original image for provenance but derives suggestions only from a human-confirmed Visual Evidence Crop. Photo-guided changes follow a building-template, Facade Zone, then local-feature hierarchy and regenerate deterministic output through structured rule overrides; they never directly patch blocks or alter known building massing.

A confirmed crop creates a Facade Zone override by default. Only an explicit user action may promote it to the whole building, and photo evidence never automatically writes changes back into a shared Parametric Building Template.

A Facade Zone is bound to one or more contiguous footprint edges, not merely a cardinal direction. An official 3D capture may use its camera pose to suggest visible edges, while a field photograph requires explicit edge selection. The user confirms every suggestion. Evidence whose facade cannot be located is retained for provenance but creates no rule override.

The normal review interaction does not expose every inferred architectural parameter. The system chooses two or three catalogued Parametric Building Templates, adapts each template's permitted exterior rules to the photo evidence, and presents whole-building Photo-Guided Appearance Proposals ranked primarily by photo match and secondarily by visual quality. The user selects one primary template and may make a small set of adjustments for material, window density, and wall depth. A user may deliberately borrow a local rule from another proposal, but the system never automatically blends templates. Structured rules, confidence, and evidence lineage remain recorded underneath, while individual parameters are available only through advanced controls.
