# Minecraft Campus Reconstruction Tool

A tool for recreating a real school campus in Minecraft through editable map-derived geometry, block-style choices, schematic preview, and export.

## Language

**Campus Reconstruction Tool**:
The whole application that helps a user turn a real school campus into Minecraft structures and terrain.
_Avoid_: campus generator, map converter

**ECNU First Target**:
The first supported real-world target for the tool: East China Normal University, used to validate data sources, editing workflows, campus style, schematic export, and Axiom import.
_Avoid_: generic first release, demo campus

**Putuo Campus**:
The first campus scope inside the ECNU First Target, used as the initial source of truth for map data, building names, schematic exports, and visual QA.
_Avoid_: first area, sample campus

**Campus Reconstruction Project**:
A named, independently resumable reconstruction of one Campus Target with its own scale, orientation, confirmed boundary, source snapshots, feature and entity reviews, known gaps, style choices, and generation state. One campus may have multiple projects, while reviewed campus building names and suppressions remain shared campus knowledge.
_Avoid_: saved campus, Foundation Manifest, annotation file, autosave

**Campus-First Launcher**:
The application entry flow that confirms one Campus Target before exposing projects for that campus. The last-used campus may be offered first, but projects from different campuses never share one undifferentiated list.
_Avoid_: recent-file screen, global project list, project-first launcher

**Campus Project Library**:
The Campus Target-scoped collection of locally managed Campus Reconstruction Projects; one campus may contain many projects, but their names are unique within that campus. It presents project identity, progress, next incomplete task, and save state without making internal file locations the user's primary model.
_Avoid_: file picker, recent files, solution list, cloud project list

**Project ID**:
The immutable identity of one local Campus Reconstruction Project, independent of its editable name, internal path, and portable export filename. Renaming or exporting preserves it, while importing a portable project creates a new Project ID and records the source relationship.
_Avoid_: project name, file path, export filename

**Project Name Conflict**:
The attempt to use a name already owned by another project under the same Campus Target. New-project creation waits for a unique name, while import assigns an `（导入 N）` suffix; neither path overwrites the existing project.
_Avoid_: global name conflict, silent replacement, duplicate campus-local name

**Project Save Point**:
A durable project state written atomically after a confirmed semantic operation or requested immediately with `Ctrl+S`. Continuous pointer movement and text input are coalesced so only the completed operation creates a save point.
_Avoid_: per-frame save, temporary input state, manual file save

**Project Save Status**:
The visible state of the active project's latest durable write: saving, saved with its completion time, or failed with a reason and retry action. The Campus Project Library also exposes each project's latest successful save time.
_Avoid_: hidden autosave, settings-only status, transient failure toast

**Project Context Change**:
An action that leaves or replaces the active Campus Reconstruction Project, including switching campus or project, creating or importing a project, and normally exiting the application. It proceeds only after the active project is durably saved; a save failure cancels the requested change and preserves the current context.
_Avoid_: unchecked navigation, discard-and-continue, file switch

**Project Recovery State**:
A validated, internally coherent candidate project state retained separately from the last confirmed Project Save Point after an unclean exit. The user explicitly chooses recovery or the confirmed version; recovered state remains working state until successfully saved, and invalid or partial recovery is never merged into the confirmed project.
_Avoid_: automatic rollback, backup overwrite, partial recovery

**Project Resume Point**:
The destination selected when an existing project opens: the earliest incomplete required task in the canonical dependency order, including the status and recovery actions of a blocked task. Completed tasks are skipped but remain reviewable; a project with no incomplete required task opens at its completion summary and export surface.
_Avoid_: last visited screen, first workflow step, forced review replay

**Campus Reconstruction Project Completion**:
The state reached when Campus Target and Boundary are confirmed, five-category acquisition outcomes and acknowledged gaps are durable, all five reviews are explicitly complete, and the current dependency-valid Minecraft 26.1.2 result has been generated and exported as `.schem` with its Foundation Manifest. Later dependency-changing edits reopen affected work, while movement or deletion of an already exported external file does not erase the recorded completion.
_Avoid_: perfect source coverage, last workflow screen, preview-only completion

**Project History Operation**:
One reversible, user-meaningful project change, such as a completed boundary drag or one confirmed batch review. Each project retains its latest 50 operations across restarts; transient input frames are not history operations, and a new operation clears the redo branch.
_Avoid_: pointer frame, autosave event, UI interaction log

**Batch Review Operation**:
An atomically confirmed set of Foundation Review Ledger decisions over an explicit, prevalidated subject set. It creates one save and undo/redo unit, fails as a whole if its dependency basis becomes stale, and never implies Foundation Layer Review Completion.
_Avoid_: partial bulk success, repeated row action, automatic layer completion

**Portable Project**:
A complete, editable export snapshot of one Campus Reconstruction Project that can be imported on another computer and resumed without changing the active local project's identity or save destination. Import creates a new local Project ID while retaining the exported project's source lineage.
_Avoid_: Save As target, final-result archive, shared live project

**Portable Project Export**:
The transactional creation of a validated external Portable Project copy without changing the active project's identity or managed save destination. Cancellation or failure leaves both the active project and any existing destination file unchanged, and replacement requires explicit confirmation.
_Avoid_: Save As, active-project relocation, silent overwrite

**Portable Project Import**:
The transactional creation of a new local project from a validated Portable Project while leaving the source file unchanged. A different embedded Campus Target requires explicit switch confirmation and a successful save of the active project before the imported project is committed under its own campus.
_Avoid_: open external file in place, overwrite current project, attach to selected campus

**Project Schema Migration**:
The transactional conversion of an older supported project structure into the current structure. Portable imports migrate a validated temporary copy, while local projects migrate from a backup on first open; neither commits partial results, alters an imported source, or guesses at a newer unsupported schema.
_Avoid_: partial import, in-place source rewrite, best-effort decoding

**Campus Target**:
The user-confirmed school campus that bounds both Foundation Mode discovery and Detailed Building Mode search. A Campus Target has one canonical name, may retain aliases, and provides the stable location scope for all later work. It does not own Campus Scale; each Campus Reconstruction Project chooses its own scale.
_Avoid_: search string, city, current map center

**Campus Target Match**:
The import-time decision that the selected Campus Target and a Portable Project's embedded target refer to the same campus. An identical stored Gaode POI ID matches automatically; otherwise names and map locations are shown for human confirmation and never cause an automatic merge.
_Avoid_: name-only match, nearby-coordinate merge, OSM identity match

**Campus Scale**:
The project-wide ratio of real-world meters to Minecraft blocks shared by Foundation geometry, Building Slots, Detailed Buildings, and style templates. Changing it after confirmed output makes generated results require regeneration and review.
_Avoid_: export zoom, per-building scale, viewport scale

**Campus Orientation**:
The project-wide geographic-to-Minecraft rotation established from a reviewed campus direction line and aligned to a chosen Minecraft axis. It changes generated coordinates without altering source geography or provenance.
_Avoid_: map rotation, camera angle, per-building rotation

**Campus Building Directory**:
The campus-scoped collection of human-recognizable building names attached to traceable source objects. It merges bundled/shared annotations, the desktop app-data annotation file, and the current user's local records. A saved name improves later discovery and review without replacing the source object's identity or geometry.
_Avoid_: OSM name, global building database, geometry override

**Campus Building Name Match**:
A confirmed association between one Building Entity and a human-recognizable campus building name. Automatic confirmation requires an exclusive building-level POI or an existing traceable directory match; campus-level, address-level, reused, or ambiguous results remain Building Name Evidence.
_Avoid_: name suggestion, global POI name, immutable name

**Building Name Evidence**:
A traceable but unconfirmed name clue from a Gaode POI, source tag, campus directory, or spatial association. It may support naming one Building Entity but never serves as building identity or gets copied across ambiguous overlaps.
_Avoid_: confirmed building name, reverse-geocode truth, entity ID

**Gaode POI Name Evidence**:
Building Name Evidence that retains the POI ID, category, name, original GCJ-02 location, normalized WGS-84 location, and coordinate-transformation lineage. Its point may support exclusive proximity matching but never serves as a building footprint.
_Avoid_: Gaode building geometry, coordinate-system-neutral POI, copied reverse-geocode label

**Building Display Name**:
The human-recognizable, editable name of one Building Entity rather than its identity. Different entities may share a display name only through explicit human confirmation; automatic matching remains exclusive.
_Avoid_: Building Entity ID, globally unique name, copied POI label

**Building Name Reconciliation**:
The explicit review decision that assigns names after Building Entities are merged or split. A split never copies one name to every result, while a merge selects one display name and retains other names as aliases or evidence without altering entity lineage.
_Avoid_: automatic name inheritance, duplicate reverse-geocode name, identity rename

**Campus Building Corpus**:
The single campus-scoped set of traceable building source objects used by both pre-search naming and later Arnis candidate review. Overture, OSM, and live Arnis observations enter this same corpus; a Nearby Building Candidate must never exist outside it.
_Avoid_: naming candidates, Arnis candidate list, nearby pool

**Campus Boundary**:
The user-confirmed polygon separating the selected Campus Target from neighboring schools and surrounding city blocks. It gates all Foundation Mode feature discovery and export.
_Avoid_: search radius, map viewport, campus center

**Campus Boundary Review**:
The campus-first decision surface where ranked automatic boundary candidates and their evidence are compared, one valid candidate may be vertex-edited, and the result is explicitly confirmed before five-category acquisition begins. Unavailable or invalid candidates remain recoverable outcomes and never become blank-canvas drawing.
_Avoid_: boundary drawing, map editor, automatic boundary acceptance

**Campus Boundary Candidate Query**:
A bounded request to the Controlled Foundation Acquisition Service using the confirmed Campus Target's name, aliases, and WGS-84 anchor to retrieve complete OSM and Overture education-area candidates before feature acquisition. It never receives Gaode credentials or confirms a boundary on the user's behalf.
_Avoid_: Gaode AOI query, feature acquisition job, automatic campus extent

**Boundary Discovery Snapshot**:
The pinned Dataset Bundle, source candidates, coverage, ranking evidence, and derivations produced by a Campus Boundary Candidate Query. The first Foundation Campus Baseline reuses its bundle; later bundles may propose differences but never silently replace the confirmed boundary.
_Avoid_: live boundary search, confirmed Campus Boundary, versionless candidate list

**Campus Boundary Relationship**:
The classified spatial relationship of a source feature to the confirmed Campus Boundary: inside, outside, or straddling. Continuous circulation, water, and vegetation may derive a clipped Review Geometry, while straddling Buildings and complete sports facilities remain whole and require review to retain or exclude.
_Avoid_: centroid-only inclusion, half building, search-radius match

**Foundation Feature Review Map**:
The campus-scoped review surface where source-backed building, circulation, water, vegetation, and sports candidates are compared, conflated, accepted, rejected, or recorded as known gaps over the reference map. Candidate geometry remains immutable; only the Campus Boundary exposes vertex editing in V1.1.
_Avoid_: detection result, candidate list, automatic truth

**Map Interaction Mode**:
The single active interpretation of clicks on the Foundation Feature Review Map: candidate review, campus orientation, or visual recapture selection. Review mode opens candidate information; capture modes make existing overlays click-through or hidden until completion or cancellation returns to review.
_Avoid_: overlapping tools, selected layer, map state

**Campus Circulation Feature**:
A reviewed vehicle road, pedestrian path, cycleway, steps, or paved plaza represented by its source-backed centreline or area geometry. It retains subtype, potentially varying width, and whether each width is explicit, rule-derived, or a style default so generation can create a surface without rewriting source evidence.
_Avoid_: generic highway, uniform road, map line

**Campus Water Feature**:
A reviewed pond, lake, river, stream, or canal represented by a source-backed water surface or centreline. Explicit or inferred width may derive a generation surface from a line, but that surface remains distinguishable from a sourced shoreline; an unusable point-only observation is a known gap.
_Avoid_: inferred shoreline, fountain, drainage network

**Campus Sports Feature**:
A reviewed pitch, court, running track, or sports container that retains its sport subtype, complete geometry, and Feature Containment Relationships. Containers do not generate as filled playing surfaces, nested tracks and pitches remain separate, and sports-hall structures remain Building Entities.
_Avoid_: flattened stadium, overlap duplicate, gymnasium surface

**Campus Vegetation Feature**:
A reviewed vegetation area, tree row, or individual tree whose area, line, or point geometry remains distinct. Explicit structured features remain primary; individual-tree points are retained in an optional collapsed layer rather than inferred from coarse land cover.
_Avoid_: generic green area, decorative noise, vegetation pixel

**Coarse Raster Derived Candidate**:
A gap-filling Map Candidate derived from licensed Sentinel-2, WorldCover, or another approved coarse raster with its date, resolution, class, processing version, thresholds, and vectorisation lineage retained. It may indicate broad water, vegetation, or land-cover extent but never overrides structured geometry or claims individual objects or precise boundaries.
_Avoid_: detected feature truth, precise shoreline, inferred tree

**Deferred Source Observation**:
A traceable building-like Source Observation whose geometry, classification, identity, or coverage limitations make it unsuitable for normal review until its evidence conflict is resolved. It is neither deleted nor accepted; only a structured deferral associated with an acknowledged Known Feature Gap counts as a completed review disposition.
_Avoid_: Low-Confidence Structure, rejected feature, simple building

**Candidate Evidence Assessment**:
The reasoned, versioned assessment of a Map Candidate across separate geometry quality, semantic classification, entity-match, and name-match dimensions; layer coverage remains a Source Snapshot property. A review-priority label may be derived for presentation, but no single high/medium/low score substitutes for these dimensions or confirms a candidate.
_Avoid_: Candidate Confidence, source confidence, name-based geometry score

**Candidate Review State**:
The Foundation Review Ledger-derived role of a Map Candidate, including pending, selected primary, supporting evidence, rejected, or unresolved conflict. Revoking a decision returns the candidate to review without deleting its Source Observation or downstream lineage.
_Avoid_: confidence, map visibility, mutable candidate flag

**Candidate Review Styling**:
The map styling where feature color communicates Foundation Feature Category and line style or badges communicate ledger-derived review and conflict state. Detailed Candidate Evidence Assessment remains in the selected feature's review context rather than being collapsed into a confidence colour.
_Avoid_: confidence color, confidence line width, provider ranking color

**Confirmed Map Feature**:
A source-backed Map Feature admitted to the Reviewed Campus Model by an explicit ledger decision. A confirmed Building additionally requires a resolved Building Entity geometry and Building Display Name before it becomes a Reviewed Building Slot.
_Avoid_: high-confidence candidate, automatic acceptance, visible candidate

**Visual Feature Provider**:
An optional user-configured model that turns a georeferenced map image into confidence-scored Map Candidates for missing buildings, roads, water, vegetation, or sports areas. Its results remain reviewable visual evidence and never silently override stronger source geometry.
_Avoid_: automatic truth, screenshot scraper, replacement map source

**Visual Feature Recovery**:
The primary non-building gap-filling workflow after structured map retrieval: a label-free capture workspace starts fitted to the full campus, the user adjusts pan and zoom, captures the current viewport, then reviews deterministic color-and-contour extraction. Water covers ponds, lakes, and rivers; screenshot-derived roads supplement continuity rather than replace structured road data.
_Avoid_: model-first detection, full-campus screenshot scan, automatic truth

**Campus Building Annotation**:
A portable, campus-specific record that associates a traceable source object with a reviewed name or persistent suppression decision. It preserves coordinate lineage without changing source geometry.
_Avoid_: geometry patch, global POI cache, fixture

**Campus Building Suppression**:
A persistent campus-local tombstone stating that a discovered source object must not appear in future candidate loads. It may be inferred from missing school identity in an automatic name or decided by a human. Suppression removes the local directory record but preserves the minimum source identity needed to prevent re-import.
_Avoid_: rejected geometry, off-campus search result

**Reverse-Geocode Naming Attempt**:
A quota-accounted Gaode lookup for an unnamed Campus Building source object, scheduled automatically after Foundation building discovery. High-, then medium-, then low-confidence candidates run with bounded concurrency; successes, no-match results, and failures are cached by campus plus canonical source ID so the same object is not sent to Gaode repeatedly.
_Avoid_: automatic retry loop, building geometry lookup

**First Vertical Slice**:
The first end-to-end success path: Foundation Mode exports an Axiom-Compatible Schematic and Foundation Manifest for Putuo Campus, then Detailed Building Mode refines one representative Building Slot and exports an updated Axiom-Compatible Schematic.
_Avoid_: MVP, phase one

**Representative Building Fidelity Validation**:
The product outcome proving that real source evidence can produce a human-recognizable Putuo Campus Library and a successfully validated Axiom import. Completion is determined by recognizability and import evidence, not by the number of implemented modules.
_Avoid_: second phase, Arnis integration phase, generator completion

**Representative Building**:
The Putuo Campus Library selected for the First Vertical Slice because its recognizable non-rectangular shape exercises the handoff from Building Slot to Detailed Building Mode.
_Avoid_: sample building, test building

**Arnis-Derived Building Geometry**:
Detailed building shape, roof structure, and block layout derived from Arnis-generated Minecraft output, used as the primary seed for the Representative Building when it is more faithful than manually authored specs.
_Avoid_: Arnis data, imported model

**Arnis Reference Reconstruction**:
The user-confirmed result produced by upstream Arnis for the Putuo Campus Library, establishing that the target is reconstructable from Arnis's source and generation chain. It is the fidelity reference whose lineage and behavior the Campus Reconstruction Tool must reproduce without depending on full-world generation.
_Avoid_: fixture baseline, hypothetical Arnis output, current application result

**Fixture Test Asset**:
Synthetic or previously captured data retained only for deterministic automated regression tests. It is not displayed as a product baseline and provides no evidence of real-building fidelity.
_Avoid_: fixture baseline, reference reconstruction, accepted result

**Arnis Data Adapter**:
The integration layer that studies or reuses Arnis's geographic data retrieval, building interpretation, and generation rules to produce structured building geometry for this tool before Minecraft world output is created.
_Avoid_: Arnis importer, world extractor

**Arnis Exterior Rule Engine**:
The version-pinned adaptation of Arnis's complete building-exterior behavior, including category presets, material and color mapping, windows, entrances, facade depth, roofs, and exterior decoration. It generates one building through the tool's in-memory block pipeline and excludes interiors, terrain, roads, whole-world generation, and world-file writing.
_Avoid_: full Arnis fork, Minimal Arnis Adapter, Arnis world generator

**Arnis Retrieval Path**:
The first fidelity-validation capability that reuses Arnis's OSM and Overture retrieval, projection, parsing, and deduplication behavior to produce reviewable real Building Geometry candidates. It establishes trustworthy source geometry before any decision about reusing Arnis building-generation rules.
_Avoid_: Arnis data, Arnis generator, full Arnis integration

**Roof Generation Suggestion**:
A provenance-backed roof shape, height, or orientation proposed when source evidence is incomplete. It remains an explicit unconfirmed suggestion until a human accepts or edits it using visual reference evidence.
_Avoid_: detected roof, default roof, roof fact

**Registered Roof Generator**:
A developer-supplied, versioned, deterministic roof-building strategy selected by style identity and validated parameters. It extends the supported roof vocabulary without representing a user-drawn or imported roof model.
_Avoid_: custom roof model, arbitrary roof code, roof shape string

**Provisional Roof Preview**:
A visibly unconfirmed roof rendering generated from a Roof Generation Suggestion so the user can compare it with reference evidence and tune its parameters. It may support review but cannot be treated as an export-ready roof.
_Avoid_: confirmed roof, final roof, roof evidence

**Roof Zone**:
An independently reviewable roof area associated with a specific building part and its own Registered Roof Generator, parameters, material, and confirmation state. One Detailed Building may contain multiple Roof Zones even when the first editor exposes only one.
_Avoid_: global roof shape, roof layer, whole-building roof

**Model-Derived Roof**:
A roof whose geometry is retained from an accepted Eligible External 3D Model rather than rebuilt by a Registered Roof Generator. Replacing it creates new Roof Zones and requires renewed visual review.
_Avoid_: generated roof, inferred roof, immutable imported roof

**Semantic Feature Preservation**:
The fidelity rule that keeps a building's overall real-world scale while locally strengthening identity-bearing features such as entrances, window bands, frames, cornices, and ridges to at least a visible Minecraft representation. The adjustment favors feature presence over sub-block dimensional precision and remains recorded as an explicit generation decision.
_Avoid_: exact miniaturization, arbitrary exaggeration, whole-building enlargement

**Semantic Feature Annotation**:
A human-reviewed statement that identifies the type and approximate location of a recognition feature without requiring precise 3D drawing. It guides deterministic Semantic Feature Preservation while remaining distinct from measured source geometry.
_Avoid_: manual 3D model, source measurement, freehand voxel edit

**Minimal Arnis Adapter**:
The first implementation of the Arnis Data Adapter, built inside this project to extract only the data and rules needed for the Representative Building before considering a full Arnis fork.
_Avoid_: forked Arnis, embedded Arnis

**Building Geometry Source Priority**:
The first-pass order for sourcing Detailed Building Mode geometry: Overture building data first, OSM/Overpass second, and existing project data or manual correction third.
_Avoid_: data source list, source preference

**Campus Boundary Source Strategy**:
The V1.1 strategy in which Gaode confirms campus identity while a Boundary Discovery Snapshot from controlled OSM/Overture education-area data proposes source-backed polygons for vertex review. Missing candidates are reported as unavailable rather than replaced by Gaode extraction or blank-canvas boundary drawing.
_Avoid_: source priority, Gaode AOI assumption, search radius

**Foundation Feature Acquisition**:
The controlled, five-category retrieval that runs the Foundation Source Matrix against one confirmed Campus Boundary and pinned Dataset Bundle. OSM and Overture provide parallel evidence without provider priority, desktop public-Overpass fallback, or blank-canvas feature correction.
_Avoid_: Foundation Feature Source Priority, provider list, manual replacement

**Building Geometry**:
The structured output of the Minimal Arnis Adapter for one building, including footprint, height, floors, roof hints, facade hints, confidence, and provenance before any `.schem` is generated.
_Avoid_: building spec, model metadata

**Observed Building Evidence**:
The source-backed facts for an accepted building, including its footprint, interior rings, building parts, tags, and explicitly derived part aggregates. Unknown source facts remain unknown and are not replaced by generation defaults.
_Avoid_: missing model, generated interpretation, final preview

**Generated Building Interpretation**:
The deterministic construction decisions that turn Observed Building Evidence into Minecraft blocks, including fallback heights, floor spacing, roof handling, facade rhythm, and materials. These decisions explain the preview but are not presented as source facts.
_Avoid_: observed geometry, source metadata, real-world measurement

**Foundation Mode**:
The low-detail mode where a user defines or imports map regions such as campus boundary, buildings, roads, vegetation, and water, assigns Minecraft blocks to those regions, and exports a usable `.schem`.
_Avoid_: base mode, rough mode, map mode

**Detailed Building Mode**:
The higher-detail mode where a Reviewed Building Slot from Foundation Mode is expanded with height, floor, roof, facade, and other building-specific evidence, previewed as editable blocks, and exported back to `.schem`. It consumes the Slot identity and name rather than searching, reverse geocoding, or creating campus buildings.
_Avoid_: fine mode, advanced mode, detailed mode

**Local Facade Reconstruction Model**:
A versioned visual model distributed with the application that proposes reviewable Detailed Building facade structure from local photo evidence. It is trained outside end-user devices under project control; user photos and inference remain on the device.
_Avoid_: local LLM, on-device trainer, cloud photo analyzer

**Facade Reconstruction Draft**:
A versioned, confidence-scored proposal for one Building Slot facade, containing its floors, bays, openings, facade features, roof candidates, and material labels before human correction and Minecraft generation.
_Avoid_: AI result, final building, generated mesh

**Parametric Building Template**:
A versioned, license-reviewed and provenance-backed set of editable facade, roof, palette, window-pattern, and wall-articulation rules that supplements missing Detailed Building evidence without changing the Building Slot massing. Arnis Style Presets are the initial base template families.
_Avoid_: fixed style preset, finished building model, fixed schematic, untraceable asset pack

**Template-Provisional Detailed Building**:
A Detailed Building generated from a selected Parametric Building Template when no local photo evidence is available. It is eligible for preview and export but remains distinct from a fully refined building until its missing exterior evidence is reviewed.
_Avoid_: completed refinement, photo-confirmed building, final reconstruction

**Detailed Building Rule Stack**:
The ordered, non-destructive rules used to generate a Detailed Building: selected template, automated draft, and accepted photo or manual overrides. Later evidence produces proposed differences and cannot replace an accepted rule without an explicit user decision.
_Avoid_: last-write-wins state, destructive regeneration, block-patch history

**Template Match Proposal**:
An ordered set of up to three Parametric Building Templates suggested for a Facade Reconstruction Draft, each carrying a matching rationale and confidence. It has no generation effect until a user explicitly selects a template.
_Avoid_: automatic style choice, silent template application, final classification

**Building Function Classification**:
A confidence-scored classification of a Campus Building's use, such as teaching, dormitory, library, administration, laboratory, sports, dining, or service. It is inferred from names, map tags, POI/campus-directory evidence, and photo evidence; users correct it only when the evidence is uncertain or contradictory.
_Avoid_: required use form, building identity, exact building name

**Map Feature**:
A source-backed, human-reviewed geographic feature admitted to the campus model, such as a building, circulation path, vegetation region, water body, or sports facility. Its selected Review Geometry and evidence remain traceable; the editable Campus Boundary is a separate scope concept.
_Avoid_: layer item, hand-drawn replacement, candidate geometry copy

**Source Geometry**:
The immutable, fully assembled geometry obtained from one source record in its declared coordinate reference system, retaining its true geometry type, parts, and holes. Later clipping, buffering, projection, or review never rewrites it.
_Avoid_: display points, clipped candidate, generated footprint

**Source Observation**:
The self-contained project record of one provider feature or raster derivation, including its Source Geometry, original properties, Source Lineage, and required licence information. It is sufficient to reproduce existing review without embedding bulk provider responses or service caches.
_Avoid_: HTTP response archive, provider cache, display candidate

**Delivery Duplicate**:
Multiple delivered representations with the same upstream record identity, record version, and content digest. The service collapses them into one Source Observation while retaining delivery-channel lineage; changed versions or merely overlapping records are never discarded as delivery duplicates.
_Avoid_: spatial duplicate, same-name feature, refreshed observation

**Foundation Geometry**:
A typed geographic value represented as Point, MultiPoint, LineString, MultiLineString, Polygon, or MultiPolygon, with polygon holes and disconnected parts preserved. A mixed GeometryCollection is decomposed into semantically coherent candidates with shared lineage rather than flattened.
_Avoid_: points array, longest ring, connected fragments

**Review Geometry**:
The WGS-84 derivative of Source Geometry used for campus-boundary filtering and human review while retaining geometry type, disconnected parts, and holes. Every transformation from Source Geometry remains traceable.
_Avoid_: raw provider geometry, screen path, Minecraft footprint

**Review Geometry Proposal**:
The Controlled Foundation Acquisition Service's versioned WGS-84 geometry suggestion derived only after complete source assembly, accompanied by the immutable Source Geometry and full derivation lineage. The desktop persists and reviews it; the service cannot replace it with a clipped-only result or accept it for the user.
_Avoid_: accepted geometry, service-only polygon, source overwrite

**Geometry Derivation Record**:
The versioned lineage connecting one geometry state to the next through relation assembly, coordinate transformation, validation or repair, and Campus Boundary clipping, including identifiable inputs and outputs. It explains a derived shape without replacing either the source or reviewed geometry.
_Avoid_: processing log, mutable geometry flag, undocumented normalization

**Geometry Repair Derivation**:
A Geometry Derivation Record that preserves invalid Source Geometry and describes a deterministic repair used to produce reviewable geometry. Semantic changes to area, holes, or parts create a geometry conflict requiring confirmation; an unreliable repair remains a known gap.
_Avoid_: silent cleanup, source overwrite, automatically trusted polygon

**Generation Geometry**:
The reproducible project-space geometry derived from reviewed evidence, project scale and orientation, and generation rules for Minecraft output. It is not source evidence and may be regenerated without changing review decisions.
_Avoid_: measured geometry, reviewed source, editable provider shape

**Map Candidate**:
A map-derived suggestion for a Map Feature that the user can accept, reject, conflate, split, or relate before export, regardless of its supported evidence source. Its Source Geometry remains immutable; an incorrect or missing V1.1 candidate becomes a known gap rather than a hand-drawn replacement.
_Avoid_: auto result, detected area

**Source Candidate ID**:
The version-qualified identity of one source observation, derived from its dataset, source record, and record version rather than its name or display geometry. A refreshed record creates a new observation that may be compared and associated with an existing stable feature entity.
_Avoid_: Building Entity ID, geometry hash alone, candidate name

**Candidate Conflation Group**:
The set of overlapping Map Candidates believed to describe the same real feature. Review chooses one primary geometry while retaining the other source objects as supporting evidence; non-primary members are merged rather than rejected.
_Avoid_: duplicate deletion, overlapping error, rejected candidates

**Building Entity**:
The stable project identity for one real building after its source candidates have been conflated and any geometry conflict resolved. It survives name, primary-geometry, and source-version changes while retaining supporting observations, explicit building parts, and split lineage.
_Avoid_: source candidate, reverse-geocode result, overlapping footprint

**Building Entity Split**:
A reversible review decision that partitions one Building Entity's retained source candidates or building parts into two or more independently named buildings without editing or discarding their source geometry. The split preserves the former grouping as lineage rather than treating the new entities as unrelated discoveries.
_Avoid_: delete and redraw, destructive unmerge, duplicate import

**Candidate Geometry Conflict Group**:
A collapsed review item for strongly overlapping candidates whose shapes differ enough that sameness cannot be assumed. Review resolves it as one building with a primary geometry, one building with explicit parts, or multiple buildings; the system never invents an averaged or unioned source shape.
_Avoid_: automatic merge, duplicate deletion, geometry blend

**Candidate Conflation Evidence**:
The traceable reasons that candidates were treated as the same source lineage, an explicit whole/part hierarchy, a geometry conflict, or separate features. It retains upstream source links and versioned spatial measures such as overlap, containment, and centre distance rather than relying on names alone.
_Avoid_: hidden deduplication score, name equality, irreversible merge rule

**Feature Containment Relationship**:
A source-backed or human-confirmed parent/member relationship that preserves overlapping feature identities, including building whole/part and sports-container/track/pitch structures. Containment never implies duplicate removal or polygon union, and each member retains its own geometry and provenance.
_Avoid_: overlap merge, flattened complex, generation fill rule

**Primary Match Candidate**:
The single open-geodata building candidate whose identity evidence best matches the confirmed Gaode Location Anchor and current search target. It is emphasized for review but never accepted automatically.
_Avoid_: automatic selection, nearest building, first result

**Related Building Candidate**:
An additional building candidate with meaningful name or semantic evidence for the current search target, kept available behind secondary review rather than competing visually with the Primary Match Candidate.
_Avoid_: nearby building, alternate ID

**Nearby Building Candidate**:
A building geometry observation returned within the bounded search area but lacking meaningful identity evidence for the current target. It remains accessible for recovery review and is collapsed by default.
_Avoid_: related candidate, rejected candidate, search match

**AOI Candidate**:
A Map Candidate specifically sourced from a map provider's AOI data.
_Avoid_: candidate, map feature

**Building Slot**:
A Foundation Mode building identity and reviewed region that is the only valid entry into Detailed Building Mode. Every Detailed Building belongs to exactly one Building Slot; missing buildings must first become reviewed slots.
_Avoid_: placeholder, pad, free-floating detailed building

**Building Slot Refinement**:
A versioned Detailed Building revision belonging to one Building Slot. Draft revisions do not affect Foundation output; the latest confirmed revision is the current high-fidelity replacement, and older confirmed revisions remain recoverable. Revoking its Slot archives rather than deletes the refinement so later reconfirmation can reattach it.
_Avoid_: overwritten foundation, unrelated detailed export, new building

**Building Slot Work Queue**:
The campus-wide Detailed Building Mode collection of all reviewed Building Slots, ordered for refinement and carrying each slot's current work status.
_Avoid_: representative building picker, independent building search, nearby results

**Provisional Building Slot**:
A rough Building Slot expressing the user's intended building and search area before its real footprint has been confirmed. It may bound retrieval but cannot act as footprint truth or identity-overlap evidence.
_Avoid_: reviewed slot, source footprint, identity anchor

**Reviewed Building Slot**:
A Building Slot derived from one stable Building Entity whose primary Review Geometry, parts, and Building Display Name have been resolved. Only a Reviewed Building Slot enters the Building Slot Work Queue or contributes building geometry to generation; a source candidate or evidence assessment alone is insufficient.
_Avoid_: confirmed candidate, provisional slot, automatic nearest building, fixture footprint

**Foundation Manifest**:
The structured handoff file exported by Foundation Mode together with the `.schem`, containing Building Slots, reviewed Map Features, evidence assessment, provenance, block choices, coordinates, dimensions, and replacement intent.
_Avoid_: metadata, project JSON, export JSON

**Campus Style**:
A reusable set of Minecraft block and generation rules that makes output feel like a school campus rather than a generic city.
_Avoid_: theme, skin

**Foundation Feature Generator**:
A registered, versioned deterministic strategy that turns a reviewed road, water, vegetation, or sports Map Feature into Minecraft blocks and decorations.
_Avoid_: single block fill, arbitrary style script, full-world generator

**Foundation Style Pack**:
A portable declarative configuration selecting Foundation Feature Generators and their validated blocks, widths, densities, edges, decorations, and seeds. Normal selection uses the built-in Arnis Classic, Modern Campus, Historic Red-Brick Campus, or Lightweight Draft preset; custom JSON import is an advanced option and cannot execute code.
_Avoid_: executable plugin, texture pack, unversioned preset

**Foundation Source Snapshot**:
A read-only retrieval record for one Foundation Feature Category, provider, pinned dataset release, and confirmed Campus Boundary version. It retains candidates, query tiles and pagination, complete-empty/complete/partial/failed coverage, errors, gaps, and provenance so later refreshes can propose changes without mutating past decisions.
_Avoid_: live map state, candidate cache, current provider result

**Foundation Feature Category**:
One of the five independently acquired and completeness-reported kinds in V1.1: Building, Circulation, Water, Vegetation, or Sports. It is a domain category rather than a visual map layer.
_Avoid_: UI layer, provider dataset, whole-campus completion

**Foundation Layer Review Completion**:
An explicit Foundation Review Ledger decision that every candidate in one Foundation Feature Category's pinned evidence has a disposition and every remaining limitation is an acknowledged Known Feature Gap. It is bound to the current Campus Boundary, Source Snapshots, and review-rule basis, and means the review is complete rather than that real-world data is complete.
_Avoid_: data completeness, no-gap layer, mutable done flag

**Foundation Review Reopening**:
The transition caused by a semantic change to a completed Foundation Feature Category: its prior completion remains in the Foundation Review Ledger while the current revision returns to review and only dependency-linked downstream results become stale. Viewing, filtering, or inspecting completed evidence does not reopen it.
_Avoid_: visit means reopen, erase completion history, reset every layer

**Foundation Dependency Invalidation**:
The deterministic, local marking of review decisions, category completion, and downstream generation results as stale when an identified upstream evidence or geometry dependency changes. Decisions whose subject identity and dependency basis are unchanged remain applicable rather than being globally reset.
_Avoid_: reset all reviews, timestamp-only invalidation, silent stale reuse

**Review Dependency Basis**:
The immutable set of subject revisions and evidence digests on which one Foundation Review Ledger decision was made, such as boundary, source geometry, entity grouping, attributes, or rule outputs. It determines whether that decision can be carried forward or must become stale after a change.
_Avoid_: latest project state, layer version only, decision timestamp

**Foundation Source Refresh Difference**:
The explicit comparison between a project's retained Foundation Source Snapshots and newly acquired pinned snapshots, classifying evidence as unchanged, added, changed, withdrawn, or coverage-changed. Unchanged decisions remain applicable; affected dependencies reopen locally, and withdrawn evidence never silently deletes a confirmed feature.
_Avoid_: automatic refresh on open, replace-current-data, repeat every review

**Feature Classification Proposal**:
The service's versioned, reasoned mapping from one Source Observation's retained raw properties to one or more possible Foundation Feature Categories or a container role. Ambiguous records remain one observation in classification conflict for desktop review rather than being copied or forced into a category.
_Avoid_: final feature type, catch-all vegetation, duplicated cross-category record

**Foundation Source Matrix**:
The versioned mapping from each Foundation Feature Category to its relevant OSM vocabularies and Overture themes: Buildings, Transportation, Water, Land/Land Use, and sports-related land use, with WorldCover-derived land cover identified as coarse evidence. Providers run in parallel and same-lineage records deduplicate rather than acting as primary/fallback sources.
_Avoid_: OSM-only nonbuilding layers, provider priority, unlabelled land-cover truth

**Acquisition Suggestion Rules**:
The versioned, acceptance-calibrated rules that expose raw measures and reasons for classification, overlap, containment, conflict grouping, and geometry repair proposals. They influence review suggestions only, never delete, merge, accept, or reject distinct source observations, and are not user-adjustable.
_Avoid_: hidden confidence score, destructive deduplication threshold, user tuning slider

**Controlled Foundation Acquisition Service**:
The versioned hosted data plane through which V1.1 retrieves OSM and Overture Campus Boundary candidates and all five Foundation Feature Categories. It returns source records, lineage, licences, pinned dataset identity, and replayable coverage evidence without receiving projects or allowing the desktop to query public Overpass directly.
_Avoid_: Overture building bridge, desktop Overpass fallback, project cloud service

**Acquisition API Surface**:
The minimal `/v1` endpoints for health, capabilities, boundary-job and five-category job creation, job status, retry, cancellation, result manifests, and resumable chunks. It deliberately excludes project upload, review mutation, cloud generation, and export.
_Avoid_: provider-specific desktop API, project sync endpoint, monolithic query route

**Acquisition Contract Version**:
The negotiated compatibility identity for the controlled service API, Source Observation schema, and rule capabilities. A job starts only when desktop and service support the same required contract and then retains that contract for its lifetime rather than best-effort parsing a changed response.
_Avoid_: unversioned endpoint, silent downgrade, mid-job schema update

**Acquisition Contract Fixtures**:
The shared OpenAPI, JSON Schema, and canonical examples that verify Python service encoding and Rust desktop decoding for complex geometry, relations, lineage, provider outcomes, coverage, licences, manifests, and corrupt chunks. Both releases must pass the same contract suite before publication.
_Avoid_: implementation-only test, prose API contract, happy-path GeoJSON sample

**Acquisition Capability Manifest**:
The service-advertised contract limits for Campus Boundary area and vertices, tiles, observations, result size, concurrency, quota, and retention. V1.1 capacities must cover its acceptance campuses; an exceeded limit rejects the request before work and never clips, truncates, or misreports completeness.
_Avoid_: hidden MAX_LIMIT, silent boundary reduction, best-effort oversized query

**Acquisition Service Credential**:
The controlled-release, installation-scoped credential authorizing only hosted map acquisition, stored separately from Gaode credentials in Windows Credential Manager and subject to quota and revocation. It creates no project account, and failure never enables a public-provider fallback.
_Avoid_: user project login, bundled Gaode key, anonymous unlimited endpoint

**Acquisition Transport Security**:
The production rule that controlled acquisition uses validated HTTPS, redacts service credentials, and never permits certificate bypass; only developer-mode localhost may use HTTP. Content digests additionally detect corrupt downloads or caches after transport.
_Avoid_: production HTTP, ignore-certificate option, credential logging

**Foundation Data Plane Boundary**:
The ownership boundary where the controlled service manages shared OSM/Overture retrieval, caching, assembly, normalization, and review suggestions, while the desktop exclusively owns user projects, Gaode credentials, final review decisions, the Reviewed Campus Model, generation, and export. Users interact only with the desktop application.
_Avoid_: cloud project processing, second user application, desktop bulk-dataset engine

**Foundation Acquisition Job**:
A resumable service-side retrieval task pinned to one request, dataset bundle, and acquisition-rule version, with progress and retry state reported independently by Foundation Feature Category and spatial tile. Reconnection or selective retry continues the same evidence snapshot rather than silently changing data versions.
_Avoid_: long synchronous query, latest-data retry, whole-job restart

**Acquisition Request Identity**:
The client-generated idempotency identity and content digest for one confirmed Campus Boundary revision, Foundation Dataset Bundle, and five-category request. Duplicate submission returns the existing job; cancellation or retry preserves completed evidence, job identity, and pinned versions.
_Avoid_: button-click job, duplicate network retry, mutable request

**Acquisition Request Envelope**:
The minimal service input containing request identity, compatible contract, Dataset Bundle requirement, and either a bounded Campus Target anchor query or a confirmed Campus Boundary. It excludes project names, user feature names, review state, Minecraft settings, and all Gaode identifiers and credentials.
_Avoid_: project upload, Gaode request proxy, review synchronisation payload

**Foundation Dataset Bundle**:
The immutable version set assigned when an acquisition starts, identifying the OSM snapshot, Overture release, service/output schema, and classification, assembly, conflation, and derivation rule versions. Initial work uses the current supported bundle; only an explicit update check creates evidence from a newer bundle.
_Avoid_: latest alias, user-selected provider version, mixed-version retry

**Acquisition Cache Identity**:
The exact Dataset Bundle, provider, Foundation Feature Category, spatial tile, query/assembly rule, and output-schema identity required to reuse a shared acquisition result. A cache hit must reproduce coverage and content digests; changed versions or invalid content force retrieval rather than cross-version reuse.
_Avoid_: latest cache, coordinate-only cache key, stale parsed result

**Pinned OSM Snapshot**:
The immutable, service-managed OSM extract or private Overpass database revision used by a Foundation Dataset Bundle. Production acquisition never falls back to a public Overpass instance or a differently dated snapshot when this source fails.
_Avoid_: public Overpass fallback, live latest OSM, desktop OSM query

**Foundation Campus Baseline**:
The five-category set of Source Snapshots produced for one confirmed Campus Boundary revision by one Foundation Acquisition Job and Foundation Dataset Bundle. Categories complete or fail independently but are acquired and explicitly refreshed together, with selective retry confined to the same baseline.
_Avoid_: building-only project state, mixed-date initial query, provider result list

**Acquisition Coverage Report**:
The replayable proof of retrieval for every feature-category, provider, and spatial-tile combination, including stable pagination exhaustion, raw and deduplicated counts, tile membership, errors, truncation, relation completeness, and result digests. Any unexhausted page, limit, missing member, or failed tile yields partial or failed coverage rather than complete coverage.
_Avoid_: candidate count, HTTP success, silent result limit

**Acquisition Result Manifest**:
The verified index of a completed or partial job's five-category snapshots, provider outcomes, coverage, licences, counts, and compressed Source Observation chunks, with stable cursors and per-chunk and whole-result digests. The desktop commits a result only after every referenced chunk validates, while interrupted or corrupt chunks resume independently.
_Avoid_: monolithic JSON response, unverified download, project file

**Acquisition Wire Format**:
The versioned JSON job/manifest contract plus gzip-compressed NDJSON Source Observation chunks using typed GeoJSON-compatible geometry, explicit coordinate, unit, and time semantics, and SHA-256 chunk and result digests. It is provider-neutral and suitable for Rust, Python, fixtures, and manual inspection.
_Avoid_: raw provider response, monolithic GeoJSON, implicit units

**Acquisition Replay**:
The deterministic reproduction of the same canonical observations, proposals, derivations, ordering, and content digests from an identical Acquisition Request, Dataset Bundle, and Contract Version. Runtime timestamps and cache state stay outside content identity; changed evidence requires a new bundle rather than nondeterministic output.
_Avoid_: best-effort rerun, cache-dependent result, implicit data update

**Provider Acquisition Outcome**:
The independent complete, complete-empty, partial, failed, or cancelled result for one provider and Foundation Feature Category within a job. Successful evidence survives other provider failures; incomplete outcomes create retryable Known Feature Gaps and never silently overwrite reviewed work when later completed.
_Avoid_: whole-job boolean, empty-as-failure, provider fallback overwrite

**Acquisition Failure**:
A structured, scoped service error identifying its code, affected job/provider/category/tile or result chunk, retryability, explanation, and suggested action. Transient tile failures receive at most three version-preserving exponential-backoff attempts; permanent or exhausted failures remain explicit outcomes.
_Avoid_: error string only, infinite retry, silent provider switch

**Acquisition Result Retention**:
The temporary service retention of a Foundation Acquisition Job's status and results for at least 30 days so the desktop can reconnect or redownload before local persistence is confirmed. It is delivery state rather than cloud project storage and never contains user review decisions.
_Avoid_: permanent project hosting, review sync, disposable response

**Known Feature Gap**:
A persisted, spatially located absence or unresolved limitation for one Foundation Feature Category, tied to attempted Source Snapshots, related candidates or entities, generation impact, acknowledgement, and open or resolved status. Acknowledgement permits explicit review completion but does not resolve the gap; resolution and reopening retain their evidence-linked Ledger history without guessed or hand-drawn geometry.
_Avoid_: empty success, transient warning, placeholder feature

**Source Lineage**:
The immutable identity chain from a Map Candidate to its provider, pinned dataset release, source record and version, upstream records, acquisition time, licence and attribution, original classification, and geometry digest. Multiple delivery channels that resolve to the same upstream record remain one evidence lineage rather than independent support.
_Avoid_: provider label, confidence boost, candidate display source

**Acquisition Licence Manifest**:
The Dataset Bundle and Source Observation-level record of dataset identity, release and acquisition dates, licence identifier and link, required attribution, and upstream source obligations. It is retained in the project and aggregated into advanced provenance and exported documentation; unclear or incompatible sources cannot enter acquisition.
_Avoid_: footer-only attribution, provider name, undocumented derived-data right

**Attribute Provenance**:
The value-level lineage of a normalized feature property, identifying its source observation, original value and unit, direct or derived status, rule version, confirmation, and conflicts. Raw properties remain retained, while generation consumes only typed attributes with explicit provenance.
_Avoid_: whole-record source label, untyped generation tag, unexplained inferred value

**Reviewed Attribute**:
The single value selected for project use after conflicting or inferred feature-property observations are reviewed, with its alternatives and decision retained. An unresolved attribute remains unknown; a generation default stays a derived generation choice and never becomes source fact.
_Avoid_: provider-wins value, latest value, style default as evidence

**Foundation Review Ledger**:
The project-local, append-only record of acceptance, rejection, conflation, split, primary-geometry, containment, boundary, naming, repair, and gap decisions against Foundation Source Snapshots. Each atomic entry identifies its subjects, source basis, before/after state, time, and optional explanation so the reviewed campus model and whole-batch undo remain reproducible.
_Avoid_: mutable overlay list, candidate status cache, generation state

**Reviewed Campus Model**:
The deterministic project-local projection of Foundation Source Snapshots through the Foundation Review Ledger, containing only reviewed Building Entities and Map Features eligible for Foundation generation. It is regenerated from evidence and decisions rather than maintained as a competing mutable list.
_Avoid_: accepted-candidate cache, editable source copy, generation output

**Stale Foundation Generation Result**:
A retained preview or schematic whose Review Dependency Basis no longer matches the current Reviewed Campus Model. It remains traceable and may be inspected or explicitly exported as a historical revision, but it cannot serve as the project's current formal export.
_Avoid_: current result, deleted history, silent reusable export

**Schematic Previewer**:
The interactive viewer/editor for `.schem` content, including block inspection, block replacement, and export.
_Avoid_: 3D viewer, preview page

**Minecraft Block Catalog**:
The version-pinned set of official Minecraft Java block identifiers and representative textures available to style controls and block replacement. V1.1 pins this catalog to Minecraft Java Edition 26.1.2 and uses the same catalog for preview and schematic export.
_Avoid_: hard-coded block dropdown, color palette, mod block registry

**Minecraft Compatibility Profile**:
A versioned contract joining one Minecraft Java target with its block catalog, representative textures, generation compatibility, and schematic data version. V1.1 bundles one fixed profile; V2 may select among profiles whose catalog packages are downloaded or imported through a separately decided trusted workflow.
_Avoid_: project schema version, loose block list, unverified resource pack

**V1.1 Schematic Compatibility Profile**:
The release-wide contract that V1.1 previews and Sponge `.schem` exports target Minecraft Java Edition 26.1.2 for Axiom use. Projects record this profile for portable validation, but V1.1 offers no per-project Minecraft version selector.
_Avoid_: selectable Minecraft target, silent version upgrade, project schema version
**Batch Block Replacement**:
The first editing capability in the Schematic Previewer: inspect a block and replace all matching blocks of one type with another block type.
_Avoid_: block painting, voxel editing

**Modern App Shell**:
The new desktop/web application architecture for the First Vertical Slice, replacing the old static-page workflow while reusing useful data, schematic, and preview logic from the existing project.
_Avoid_: refreshed old UI, legacy web app

**Tauri React Shell**:
The superseded compatibility stack: Tauri for the desktop container, React and TypeScript for product UI, and Three.js for the Schematic Previewer. It remains source reference for the future Cloud Web Companion but is not a V1 desktop runtime.
_Avoid_: current desktop app, native workbench

**Native Slint Workbench**:
The V1 desktop product: a Rust/Slint main application with one Desktop Application State, plus isolated Gaode WebView2 and native wgpu preview tool processes.
_Avoid_: web wrapper, Tauri shell, embedded website

**V1 Release**:
The accepted 1.0.1 Native Slint Workbench release. It includes the fixed Arnis Style Presets but excludes Project Workbench, photo-evidence workflows, the Local Facade Reconstruction Model, and Parametric Building Templates.
_Avoid_: V1 rebuild, photo-guided release

**V2 Release**:
The next product release, adding Project Workbench, photo-evidence workflows, the Local Facade Reconstruction Model, Parametric Building Templates, and selectable Minecraft Compatibility Profiles above the accepted V1 baseline.
_Avoid_: first release, V1 rebuild

**First-Run Guidance**:
The skippable, reopenable V1.1.0 in-app introduction that covers language selection, Gaode credential setup, creating a Putuo Campus project, and the Foundation-to-Detailed-to-Axiom workflow. A screenshot-based quick-start document supports it, but neither replaces the other.
_Avoid_: Project Workbench, forced tutorial, help document only, control-by-control tour

**Online Map Query**:
The primary Foundation Mode data path that uses live map services and open geodata, including Arnis-style sources and the user's Gaode API access, to search, identify, and draft Map Candidates before user review.
_Avoid_: cached sample data, offline-first map import

**Gaode Location Anchor**:
The user-confirmed Gaode POI result or Gaode map location that identifies the intended real building and supplies the location around which open building geometry is retrieved. It establishes search identity and position, not construction geometry, and records how it was acquired.
_Avoid_: Gaode footprint, Building Geometry, automatic nearest building

**Gaode Map-Confirmed Anchor**:
A Gaode Location Anchor created when the public POI API cannot return an App-visible building and the user confirms its position by clicking the embedded Gaode map. It is a location fallback, never manually authored building geometry.
_Avoid_: manual footprint, guessed coordinate, extracted Gaode geometry

**Open-Geodata Query Anchor**:
The WGS-84 location derived from the original GCJ-02 Gaode Location Anchor and used to query OSM, Overture, and Arnis sources. Both coordinates and the transformation lineage remain attached so provider coordinate systems cannot be silently mixed.
_Avoid_: raw Gaode coordinate, approximate campus center, coordinate-system-neutral point

**Gaode 3D Reference**:
The interactive Gaode 3D map view used by a human to check that retrieved building location, footprint, orientation, and approximate massing correspond to the intended building. It is review evidence and is not treated as an extractable geometry source.
_Avoid_: Gaode 3D model import, mesh source, automatic geometry truth

**Official 3D Reference Capture**:
A user-supplied image from an official 3D campus map that supports review of overall architectural style and feature arrangement. It is rendered visual evidence, not a source of measured geometry or directly reusable textures.
_Avoid_: 3D model import, measured facade, authoritative material sample

**Field Building Photograph**:
A user-supplied real-world photograph that supports review of facade materials and local architectural details while retaining viewpoint, occlusion, lighting, and transient-scene uncertainty.
_Avoid_: clean elevation, measured facade, automatic texture source

**Visual Evidence Crop**:
A human-confirmed, source-linked region containing the cleanest complete view of one facade or architectural feature. Only this region contributes photo-derived suggestions; the original image remains the provenance record.
_Avoid_: destructive crop, whole-photo recognition, automatic truth

**Cross-View Visual Agreement**:
The confidence signal created when the same architectural feature appears consistently across independent Official 3D Reference Captures or Field Building Photographs. It strengthens a generated suggestion but never confirms that suggestion without human review.
_Avoid_: single-image confidence, automatic acceptance, photo majority vote

**Gaode 3D Coverage Gate**:
The current-stage eligibility rule that a Detailed Building target must have a usable Gaode 3D Reference before reconstruction work begins. Buildings without that coverage are deferred rather than reconstructed under weaker evidence.
_Avoid_: satellite-only fallback, reduced-confidence reconstruction, missing-reference override

**External 3D Model Candidate**:
A traceable 3DMR or Wikidata-linked model associated with an open map object and offered for human review as a possible Detailed Building source. It is distinct from the non-extractable Gaode 3D Reference and cannot become accepted geometry without identity and license review.
_Avoid_: Gaode 3D model, automatic model replacement, generated roof

**Eligible External 3D Model**:
An External 3D Model Candidate whose identity is accepted and whose explicit license permits the project's non-commercial voxel adaptation under recorded attribution and sharing conditions. Models with missing, unclear, or no-derivatives terms may remain review clues but cannot enter the final schematic.
_Avoid_: freely downloadable model, fair-use model, unlicensed asset

**Eligible Building Style Reference**:
A traceable, maintainer-reviewed source whose rights permit architectural features to inform an originally authored construction rule. It may be an open-source building project, lawfully reusable official image, Official 3D Reference Capture, or Field Building Photograph. It supplies evidence and lineage but is not copied as a finished building.
_Avoid_: GitHub-only source, unlicensed image, copied building

**Building Template Catalog**:
The bundled collection of maintainer-authored and community-contributed Parametric Building Templates organised for matching primarily by Building Function Classification and project-local confirmation lineage. Every entry is versioned in the project repository and retains its reference lineage, license obligations, template version, and optional university case labels; architectural period and construction language may remain descriptive metadata rather than required matching signals.
_Avoid_: live GitHub search, external template marketplace, required period classifier

**Arnis Style Preset**:
A fixed V1 exterior-generation choice mapped directly to one complete, version-pinned upstream Arnis Building Category and its material, window, wall-depth, decoration, parapet, and roof behavior. Its user-facing label may be translated for clarity but must not claim an unsupported Chinese architectural period or school-specific style.
_Avoid_: Chinese campus template, photo-trained style, renamed invented Arnis category

**Template Application**:
A Generated Building Interpretation that applies one Parametric Building Template to a Building Slot while preserving all known Observed Building Evidence. It may change block choice, block arrangement, fenestration, and wall depth, but not measured building massing.
_Avoid_: geometry replacement, footprint fitting, building rescaling

**Facade Zone**:
An independently reviewable exterior wall region bound to one or more contiguous footprint edges rather than only a cardinal direction. Official 3D camera evidence may suggest visible edges; field-photo evidence requires the user to select them. It may override the building-wide template for materials, windows, entrances, and wall articulation without changing massing. Evidence that cannot be bound to an edge remains evidence and creates no override.
_Avoid_: whole-building facade, photo direction, freehand block patch

**Photo-Guided Rule Override**:
A reviewed Generated Building Interpretation change derived from a Visual Evidence Crop. It defaults to one Facade Zone; a user may explicitly promote it to the whole building or narrow it to one local Semantic Feature Annotation. It changes rule parameters and regenerates blocks rather than directly patching the resulting model, and never automatically changes a shared Parametric Building Template.
_Avoid_: photo texture, AI block edit, source geometry correction

**Photo-Guided Appearance Proposal**:
One of two or three whole-building previews produced by applying a catalogued Parametric Building Template and adapting its permitted exterior rules to a Visual Evidence Crop. Proposals are ranked first by photo match and only then by visual quality. The user selects one primary template and may adjust material, window density, and wall depth; selected local rules may be borrowed from another proposal, but templates are never automatically blended.
_Avoid_: annotation checklist, direct block proposal, automatic final appearance

**Building Template Matcher**:
The project-distributed visual retrieval model that compares a user-confirmed Visual Evidence Crop with rendered references from the Building Template Catalog. It is fine-tuned from a permissively licensed Chinese multimodal embedding model, runs inside the desktop application without a separate model server, and ranks templates by photo match.
_Avoid_: Ollama model, online inference API, architectural style chatbot

**Cloud Web Companion**:
The independently deployed browser edition of the Campus Reconstruction Tool. It may reuse migrated web presentation code but is not embedded into, used as a fallback by, or required to operate the native desktop application.
_Avoid_: desktop WebView shell, hosted desktop UI, legacy workflow fallback

**Desktop Application State**:
The single authoritative state of the native desktop workflow, including the open Campus Reconstruction Project, review progress, selected candidates, generation results, and tool-window messages. Presentation surfaces may display it and submit user intentions but never own competing copies.
_Avoid_: React hook state, duplicated UI state, WebView-owned project

**Desktop Tool Process**:
A purpose-specific child process launched and supervised by the native desktop application to host either the Gaode 3D Reference or native schematic preview. It receives immutable work snapshots and returns typed interaction results without owning Desktop Application State.
_Avoid_: second application, independent project editor, embedded full web UI

**Contributed Training Crop**:
A Visual Evidence Crop that a user separately and explicitly submits for improving the Building Template Matcher, together with its selected template and a confirmed reuse licence. Photos remain local by default. Contribution removes EXIF metadata and masks identifiable faces and vehicle plates before upload.
_Avoid_: automatic telemetry image, implicit training consent, raw photo archive

**Uncovered Building Style Sample**:
An authorised Contributed Training Crop for which the user explicitly rejects every proposed template. It remains an unlabelled signal for discovering missing catalog coverage and is never forced into the nearest existing template class.
_Avoid_: failed match, nearest-template label, low-confidence positive

**Visual Evidence Conflict**:
A review state in which images associated with the same Facade Zone disagree about a potentially stable architectural feature. Agreement across independent views may raise confidence, but source recency alone never resolves a conflict. People, opened windows, blinds, advertisements, vegetation, and other transient states do not become construction rules. Until a user selects the trustworthy evidence, the deterministic Arnis or template baseline remains active.
_Avoid_: latest-photo wins, automatic evidence merge, transient facade feature

**Source Conflict Review**:
A required human decision when an External 3D Model Candidate disagrees materially with current open footprints or the Gaode 3D Reference. It identifies the primary geometry and records why the conflicting source was accepted only as support or rejected as outdated or mismatched.
_Avoid_: automatic source priority, highest-detail wins, silent merge

**Building Evidence Review Workspace**:
The in-application side-by-side review surface that keeps the confirmed Gaode building identity visible while comparing the Gaode 3D Reference with open footprints, external model candidates, and Minecraft previews. An external map page is a recovery aid rather than the normal acceptance path.
_Avoid_: separate browser check, automatic visual match, generic preview page

**Advanced Data and Provenance**:
The collapsed technical record behind a reviewed building, including raw observations, field derivation, conflicts, rules, coordinate lineage, validation, and correction history. It supports audit and diagnosis without competing with the normal reconstruction workflow.
_Avoid_: main result, required reading, building preview

**Building Source Exhaustion**:
The explicit outcome reached when OSM geometry, Overture supplementary footprints, and eligible 3DMR or Wikidata-linked models provide no credible candidate for the Gaode Location Anchor. It reports unavailable source evidence rather than inventing geometry or silently using a fixture.
_Avoid_: automatic fallback, manual approximation, empty success

**Axiom-Compatible Schematic**:
A gzip-compressed Sponge v2 `.schem` file that can be imported directly into the Axiom mod.
_Avoid_: schematic, sponge file
