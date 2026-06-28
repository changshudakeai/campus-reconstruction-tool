# ADR-0018: Separate campus boundary acquisition from foundation feature discovery

## Status

Accepted

## Context

A radius around a campus POI admits neighboring schools and city buildings. Open building geometry is useful inside a campus, but it cannot by itself establish which educational parcel the user selected. Gaode visibly distinguishes school grounds, yet its public POI/JS APIs do not reliably expose that rendered outline as an AOI polygon.

## Decision

Foundation Mode starts after a Campus Reconstruction Project has been selected or created. Its user-facing workflow is ordered as `01 · Project Parameters`, `02 · Campus Boundary`, `03 · Foundation Feature Review Map`, and `04 · Foundation Preview and Export`. Project Parameters collect Campus Scale, Arnis style choices, and per-feature style settings before map review begins.

Foundation Mode uses two ordered map-data pipelines. They run sequentially rather than concurrently: internal feature providers do not start until the user confirms the Campus Boundary.

Campus Boundary acquisition starts from the user-confirmed Gaode campus identity and displays Gaode 3D as the review base map. The `Find Campus Boundary` action belongs directly below that map inside the Campus Boundary step rather than in the Foundation Mode header, so users see the reference map before looking for the boundary query action. OSM education ways and relations are assembled into complete rings before they may become boundary candidates; relation fragments must never be presented as standalone triangles. A valid closed, non-obviously-self-intersecting polygon with non-trivial area is the minimum eligibility gate. Campus or alias name match, plausible area, distance, and containment of (or proximity to) the confirmed Gaode anchor contribute to confidence and ranking rather than acting as mandatory filters. The best candidate may load into the editor but always requires human confirmation; lower-confidence valid candidates remain selectable. When no valid ring exists, the user draws the boundary directly over the Gaode map and must avoid neighboring schools. Boundary review is a shared editable-polygon workflow for both proposed and manually drawn shapes: users can drag vertices, insert a vertex on an edge, delete a selected vertex, move the whole polygon, and undo edits before confirmation.

After boundary confirmation, Foundation Feature discovery queries the complete Campus Boundary coverage before applying quality filters. Large boundaries are divided into spatial tiles and each tile is exhausted, deduplicated, and reported as complete rather than silently truncating the campus at a fixed candidate limit. The actual polygon then applies according to feature semantics. Roads, water, vegetation, and sports geometry are clipped to it. Building footprints are never cut into partial buildings: a building whose main body lies inside remains whole, while a boundary-straddling building with unclear ownership remains pending. Overture and OSM are merged for buildings; OSM supplies roads, water, vegetation, and sports geometry; Gaode supports naming and visual verification; human correction remains final fallback. Results appear together on one Foundation Feature Review Map as distinct toggleable feature layers. The application may acquire feature layers in parallel or independently, but the user-facing review order is strict: draw the Campus Orientation line, review building candidates, review water candidates, review sports candidates, review vegetation candidates, review road candidates, then use recovery and advanced data tools only for gaps. Locked layers may show availability counts but not expose review actions before their step is reached. Each candidate-kind substep advances only when the user explicitly marks that kind complete. The application does not require every candidate to be accepted or rejected before advancing, but it reports the remaining pending count and requires confirmation before moving past unresolved candidates. Building candidate cards provide a `View in 3D Map` action that focuses the existing Gaode 3D campus base map on that candidate's outline and opens the same candidate popup used by map clicks, so naming, confirmation, rejection, and revocation remain one interaction model. Geometry that is valid and trustworthy for its feature kind remains pending until human review. Imported candidate geometry is immutable and supports accept or reject only. Missing or incorrect geometry is rejected and replaced by a separately traceable human-drawn building polygon, road polyline, or water, vegetation, or sports polygon. Manual drawing is not exposed as a persistent global action on the review map; it appears only as the fallback at the end of the active feature-kind substep after structured candidates and optional recognition recovery are understood. Campus Boundary editing remains its own editable-polygon workflow.

The Foundation Feature Review Map has one mutually exclusive Map Interaction Mode. Candidate review mode makes visible candidate overlays clickable and opens their review popup. Campus orientation, manual feature drawing, and local visual-capture modes make all existing campus and feature overlays click-through so map clicks reach the active tool. Completing or cancelling a tool returns to candidate review mode.

Building quality classification happens only after complete retrieval and deduplication. No single simplicity threshold may remove a building: area, minimum width, source classification, available height or floor evidence, cross-source agreement, and contradictory overlap with sports, water, or vegetation evidence contribute to the decision. Objects classified as Low-Confidence Structures are absent from normal map overlays, feature counts, and exports while retaining their provenance and classification reasons for recovery.

Building candidate review uses mutually exclusive High, Medium, Low, and Confirmed views. High, Medium, and Low contain only pending candidates of that Candidate Confidence; switching views changes both cards and map overlays. Confirmation removes a candidate from its confidence view and moves it exclusively to Confirmed. Rejected candidates are hidden from normal review but remain recoverable under Advanced Data. Generation reads only Confirmed Map Features.

Map color represents feature kind, while line style represents Candidate Review State. Buildings remain orange; pending candidates use dashed outlines and confirmed candidates use solid outlines. Rejected candidates are absent. Line width and color do not encode Candidate Confidence, which appears only through the active High, Medium, or Low view and the candidate popup label.

Clicking a visible candidate opens an anchored review popup with its editable name, source, confidence and reasons, geometry summary, and state actions. When several OSM, Overture, or other candidates overlap the clicked point, the popup presents the complete Candidate Conflation Group, defaults to the strongest candidate, and lets the user inspect each source. Confirmation selects one primary geometry; other group members remain supporting evidence and are marked merged rather than rejected.

The same confidence-filter interaction applies to Road, Vegetation, Water, and Sports candidate cards, but each layer owns feature-appropriate rules. Road continuity, area closure, water classification, and valid sports nesting are evaluated independently rather than inheriting Building thresholds.

Filtered review supports batch confirmation and batch rejection scoped strictly to the active feature layer and pending Candidate Confidence view. Before applying, the UI reports the affected count and requires confirmation so a broad campus-wide action cannot be triggered ambiguously. A Building cannot enter Confirmed until its geometry is confirmed and it has either a Campus Building Name Match or a manually supplied name.

Batch building confirmation confirms only candidates that already have a Campus Building Name Match or manually supplied name. Unnamed candidates remain in their original confidence queue, and the result reports confirmed and skipped-for-name counts separately; synthetic placeholder names never enter the Building Slot Work Queue.

Each confirmed batch review is one atomic project-history operation. Undo restores every affected candidate to its prior state together rather than forcing item-by-item recovery.

Candidate Confidence is computed by a versioned, deterministic ruleset whose cards expose the contributing reasons. Normal product UI does not expose raw minimum-area, minimum-width, or scoring sliders; users review outcomes rather than creating irreproducible per-session threshold combinations.

Feature-review decisions are project-local and keyed by stable source identity. A later discovery run may reuse accepted, hidden, or restored status only while the source geometry remains materially equivalent; meaningful changes in position, area, or shape mark the candidate for renewed review instead of silently inheriting the old decision.

Road, water, vegetation, and sports discovery run as separate complete layer queries rather than sharing one global result cap. Each layer supports both ways and assembled multipolygon relations, uses a feature-specific tag vocabulary, merges available OSM and Overture structured geometry, and reports its own coverage. The Visual Feature Provider supplements visible gaps only; its candidates remain pending and never replace stronger structured geometry silently.

Water, sports, vegetation, and road review use the same recovery shape but different default source expectations. Water starts from OSM water candidates, then optional local visual recovery, then manual drawing. Sports starts from OSM `sport`, `pitch`, and `track` candidates, then optional local visual recovery, then manual drawing. Vegetation starts from OSM or open-data vegetation areas and rows, then optional local visual recovery, then manual drawing; individual trees remain collapsed by default. Roads start from OSM roads, paths, steps, and plaza-like paved areas; visual recovery only supplements continuity gaps and must not replace structured road geometry.

Visual recovery candidates enter the same Candidate Confidence ruleset as structured candidates. Usable visual gap-fill geometry for water, sports, vegetation, and roads may be medium confidence when its size and continuity are plausible, while incomplete or tiny geometry remains low confidence. Confidence never auto-confirms a candidate; it only chooses the review queue.

Visual recovery is a primary review action for water, sports, vegetation, and road gaps after structured candidates. The map enters a top-down label-free capture workspace fitted to the full Campus Boundary as a starting view; the user adjusts pan and zoom, then explicitly captures the current viewport. Recognition remains clipped to the confirmed Campus Boundary.

The Road layer preserves Campus Circulation Feature subtypes for vehicle roads, pedestrian paths, steps, and paved plazas. Subtypes retain separate width and material intent through review and generation rather than rendering every `highway` object as the same gray line.

The Vegetation layer preserves areas such as lawns, woodland, gardens, and shrub beds; tree rows remain line geometry; and individually mapped trees remain point geometry in a collapsed optional detail layer so dense tree records do not overwhelm normal campus review.

The Water layer deliberately covers only ponds, lakes, and rivers. Fountains, decorative water features, drainage channels, and underground drainage are outside Foundation Feature discovery so the base workflow does not grow a misleading or over-detailed water taxonomy.

The Sports layer preserves complete field or court geometry and its sport subtype. Running tracks may overlap enclosed football fields or other courts because the real facilities are nested; sports halls and gymnasium structures remain Buildings rather than Sports features.

Provider ordering remains an internal acquisition policy, not a primary user-facing ranking. The normal workflow summarizes discovered feature kinds and review status; per-provider ordering, failures, cache state, and provenance live under Advanced Data and Provenance.

Foundation Feature providers fail independently. Successful layers remain reviewable when another provider times out, is rate-limited, or returns no data; each failed layer reports its own warning and retry action. The overall discovery is considered exhausted only when no provider supplies usable geometry.

## Consequences

- Campus identity, campus extent, and internal features no longer share one ambiguous source priority.
- Feature-layer loading begins visibly after boundary confirmation and reports progress per layer.
- A search radius is retrieval scope, never campus truth.
- Neighboring-school features are excluded by the confirmed boundary.
- Campus-scale review happens spatially on the reference map rather than through one-by-one acceptance cards.
- Completion means every spatial tile covering the Campus Boundary has finished or visibly failed; a provider result limit is never treated as full coverage.
- Missing feature coverage is recoverable by drawing on the same map instead of switching to a detached editor.
- Automatic boundary failure is recoverable without a detached SVG editor.
- Both proposed and manually drawn boundaries use the same editable-vertex review interaction.
- The main workflow reports useful feature coverage rather than exposing a generic source-priority strip.
- A transient provider failure cannot erase usable results returned by other providers.
- The displayed Gaode school shading is reference evidence, not silently scraped geometry.
