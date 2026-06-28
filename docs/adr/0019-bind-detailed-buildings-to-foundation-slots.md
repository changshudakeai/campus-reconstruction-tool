# ADR-0019: Bind every Detailed Building to a Foundation Building Slot

## Status

Accepted

## Decision

Detailed Building Mode is entered through the complete set of Reviewed Building Slots from Foundation Mode, never through an independent campus-wide building search. A Building becomes a Reviewed Building Slot after a human confirms its geometry and it has either a campus-qualified automatic name match or a manually supplied name; Candidate Confidence alone cannot admit it. Detailed Building evidence, edits, previews, and exports retain that Slot identity. Detailed work creates versioned Building Slot Refinements. Drafts do not affect Foundation output; the latest confirmed refinement is active, older confirmed versions remain recoverable, and the original footprint and source evidence remain available for audit.

Detailed Building Mode does not run building-name matching, reverse geocoding, nearby-building discovery, or free-form campus search. It may retrieve detailed evidence only for the selected Slot, such as height, levels, roof, building parts, and facade observations. Name correction returns the user to the Slot on the Foundation Feature Review Map.

## Consequences

- Detailed Building Mode cannot introduce buildings outside the confirmed Campus Boundary.
- Every accepted Foundation building remains findable in Detailed Building Mode.
- Search may attach evidence to a selected Slot but cannot create a free-floating Detailed Building.
- Refinement changes the generated replacement, not the historical source observation.
- Detailed Building Mode presents the full Building Slot Work Queue with unstarted, draft, refined, insufficient-data, and deferred states; it prioritizes unstarted high-confidence slots rather than auto-selecting only the Representative Building.
- Removing a refined Slot requires confirmation and archives its Refinement history instead of destroying it; restoring or replacing the Slot may reattach that history.
- Revoking a confirmed candidate returns it to its original Candidate Confidence queue. Existing Detailed Building drafts remain archived and are reattached when the same stable source identity is confirmed again.
