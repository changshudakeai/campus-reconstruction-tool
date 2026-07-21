# ADR-0017: Name campus source objects before building search

## Status

Accepted; automatic naming and cross-source conflation rules superseded by ADR-0032

## Context

Most OSM and Overture building footprints do not carry a human-recognizable campus building name. Asking for a building name before resolving these source objects makes search appear incomplete and forces users to repeat the same manual identification work. Gaode reverse geocoding can help, but it has a limited call quota and cannot be retried blindly.

## Decision

Detailed Building Mode maintains one Campus Building Corpus before the building search stage. It merges campus-bounded Overture and OSM footprints, then feeds every building returned by a later live Arnis query back into the same corpus. The naming step and Nearby Building Candidate review therefore cannot diverge into separate pools. The Overture bridge ranks intersecting footprints by distance before applying its result limit.

Naming uses either one cached Gaode reverse-geocode attempt per campus/source ID or a user-entered name after focusing that footprint in Gaode 3D. After Foundation building discovery, the application schedules all unnamed candidates in high-, medium-, then low-confidence order with four-way concurrency. Naming runs in the background and does not block map review; each candidate exposes pending, matched, no-match, or failed naming state, and manual entry may finish it early. When Gaode returns several nearby POIs, a school- or campus-prefixed POI is preferred over a closer unrelated POI. A result that passes the selected school, campus, or alias identity rules becomes the confirmed editable Campus Building Name Match without a separate naming-review step; the candidate map popup may correct it later.

A reverse-geocoded name belongs to the Campus Target only when it begins with the selected school, canonical campus, or campus alias. Other objects become persistent Campus Building Suppressions. Manual deletion creates the same campus-local source tombstone and removes any local name record, so the source object does not reappear on the next load.

Names and suppressions are stored as portable Campus Building Annotations. Cross-source candidates may reuse a reviewed annotation when their source IDs match, the stored point lies inside the candidate footprint, or their WGS-84 centers are within the small fallback tolerance. Annotations augment search and review only; they do not replace geometry, coordinate lineage, or provenance.

Editing a confirmed building name from the Foundation Feature Review Map updates the shared Campus Building Directory and the associated Building Slot identity used by Detailed Building Mode.

Detailed Building Mode performs no campus-wide name search or reverse geocoding. It displays the Reviewed Building Slot name and may navigate back to the Foundation Feature Review Map for correction.

## Consequences

- Repeated reverse-geocode visits do not consume additional Gaode calls, including previous no-match and error attempts.
- Bounded concurrency makes current-page naming faster without issuing an unbounded burst.
- Non-campus and manually deleted objects disappear from future campus and nearby candidate lists.
- OSM and Overture representations of the same building can share one recognizable name even when large footprints have different centers.
- A campus can gradually acquire a reusable community-maintained building directory.
- Detailed Building Mode receives resolved Slot identities even when Gaode or open-data names were absent and manually supplied in Foundation Mode.
- Overture and OSM are merged rather than treated as mutually exclusive fallbacks; Arnis observations are reconciled into that same corpus.
