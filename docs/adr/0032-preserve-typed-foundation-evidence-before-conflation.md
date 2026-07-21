# ADR-0032: Preserve typed Foundation evidence before conflation

## Status

Accepted

## Context

The current `MapCandidate.points` model flattens points, lines, polygons, multipolygons, holes, and disconnected parts into one array. It also lets source delivery, geometry quality, entity identity, and reverse-geocoded names collapse into one candidate and one confidence label. This creates duplicate overlapping buildings, copies one Gaode result onto many footprints, loses source geometry, and makes later refreshes impossible to audit safely.

## Decision

Every persisted Source Observation retains typed Source Geometry, original properties and coordinate reference system, and complete Source Lineage. Foundation Geometry supports Point, MultiPoint, LineString, MultiLineString, Polygon, and MultiPolygon; holes and disconnected parts remain intact. A versioned Geometry Derivation Record connects the immutable source to WGS-84 Review Geometry through relation assembly, coordinate conversion, validation or repair, and semantic Campus Boundary handling. Generation Geometry is derived later from reviewed evidence, project orientation and scale, and style rules; it is never source truth.

Buildings are reviewed as stable Building Entities rather than independently named provider candidates. Same-lineage OSM/Overture records are one evidence lineage, explicit `building`/`building_part` records form a whole/part relationship, and strongly overlapping but differently shaped observations enter a Candidate Geometry Conflict Group. Review selects a primary geometry, establishes parts, keeps features separate, or performs a reversible entity split; it never invents unioned or averaged source geometry.

Building naming occurs after conflation. A Gaode POI, source tag, or directory record is Building Name Evidence until it exclusively matches one Building Entity. Campus-level, address-level, reused, or ambiguous POIs do not become names, and one Gaode POI ID may automatically name at most one entity. Human confirmation may preserve genuinely duplicated display names. Entity merge and split never copy names automatically; names remain distinct from stable entity identity.

Feature relationships retain real overlap semantics. Building whole/part and sports-container/track/pitch relationships do not trigger duplicate removal. Circulation and linear water retain centreline geometry, subtype, potentially varying width, and explicit, rule-derived, or style-default width provenance; buffering happens only for Generation Geometry. Real area geometry remains an area. Complete sports facilities and Buildings are not clipped into partial objects, while continuous circulation, water, and vegetation derive campus-clipped review geometry. Point inclusion and all boundary decisions remain traceable.

Candidate Evidence Assessment separates geometry quality, semantic classification, entity match, and name match; completeness belongs to the Foundation Source Snapshot for one Foundation Feature Category. Each snapshot is pinned to a provider release and Campus Boundary version and records tiles, pagination, complete-empty/complete/partial/failed coverage, errors, gaps, and its Source Observations. Coarse Raster Derived Candidates retain date, resolution, class, thresholds, algorithm version, and vectorisation lineage and may fill only reported structured-data gaps.

Typed attributes carry Attribute Provenance independently, so geometry, height, levels, width, subtype, and name may come from different observations. Conflicting values remain visible until review selects a Reviewed Attribute; generation defaults never become source facts. Known Feature Gaps are persisted without guessed or hand-drawn geometry.

The append-only Foundation Review Ledger records acceptance, rejection, conflation, split, primary geometry, containment, boundary, naming, repair, attribute, and gap decisions. The Reviewed Campus Model is a deterministic projection of Source Snapshots through that ledger, and it is the only Foundation geometry input to generation.

## Consequences

- V1.1 removes five-category blank-canvas feature drawing; only Campus Boundary vertex editing remains.
- Full provider payloads and bulk datasets remain reconstructable caches, while the portable project retains self-contained Source Observations sufficient to restore existing review.
- Spatial conflation thresholds and acquisition pagination belong to the controlled service contract, but their measurements and rule versions must fit this model.
- Source refresh creates new observations and explicit differences; it cannot silently rewrite reviewed entities or geometry.
