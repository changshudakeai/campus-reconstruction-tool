# ADR-0020: Use visual recognition as a supplementary feature provider

## Status

Accepted

## Decision

Foundation Mode may call a user-configured local or remote HTTP Visual Feature Provider with a georeferenced map image. The provider returns classed masks or polygons with confidence; the application converts them into `screenshot_analysis` Map Candidates, uses them to fill coverage gaps, and requires review instead of silently replacing stronger Overture or OSM geometry.

Visual Feature Recovery does not begin with custom model training. After OSM and Overture retrieval, a semi-automatic label-free capture supports deterministic color and contour extraction for ponds, lakes, rivers, vegetation areas, regular sports surfaces, and obvious road continuity gaps. Roads remain structured-data-first, with screenshot processing limited to continuity gaps. Optional prompt-guided general segmentation may refine a user-indicated object before a trained SegFormer-, Mask2Former-, or equivalent provider is justified by measured residual gaps and a reviewed dataset.

Visual recognition is always user-triggered after structured providers finish and suspected coverage gaps are visible. The application does not automatically transmit screenshots or call a configured model; the user first confirms whether the local screenshot is suitable, and returned candidates enter the same Candidate Confidence review workflow.

The normal visual-supplement workflow is a primary action in each non-building review step. It opens the Gaode campus base map in a two-dimensional north-up, label-free capture workspace and initially fits the confirmed Campus Boundary. The user then adjusts pan and zoom and explicitly captures the current viewport before recognition. The exact captured viewport supplies geographic bounds, limiting duplicate geometry, false positives, and processing cost.

For every capture, the application records the exact current GCJ-02 viewport and restores the user's previous remembered 3D view afterward; an arbitrary pitched screenshot is never treated as planar geometry.

The capture style also hides place names, POI icons, text labels, review overlays, and edit handles, retaining only the background and relevant road, building, water, and vegetation rendering needed by deterministic extraction or a model. Normal interactive map labels and overlays return with the restored user view.

## Consequences

- Model choice and hosting remain user-configurable.
- Every visual candidate retains image bounds, model identity, class, confidence, and source lineage.
- Geometry conflation prefers traceable map sources; conflicting visual candidates remain pending.
