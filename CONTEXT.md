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
A named, independently resumable reconstruction of one Campus Target with its own scale, orientation, confirmed boundary, feature reviews, human geometry corrections, style choices, and generation state. One campus may have multiple projects, while reviewed campus building names and suppressions remain shared campus knowledge.
_Avoid_: saved campus, Foundation Manifest, annotation file, autosave

**Campus Target**:
The user-confirmed school campus that bounds both Foundation Mode discovery and Detailed Building Mode search. A Campus Target has one canonical name, may retain aliases, and provides the stable location scope for all later work. It does not own Campus Scale; each Campus Reconstruction Project chooses its own scale.
_Avoid_: search string, city, current map center

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
A campus-qualified building name derived from cached reverse geocoding, map tags, POI proximity, campus aliases, or cross-source matching. A qualifying match becomes the confirmed editable name without a separate naming-review step; unmatched buildings still require a manually supplied name.
_Avoid_: name suggestion, global POI name, immutable name

**Campus Building Corpus**:
The single campus-scoped set of traceable building source objects used by both pre-search naming and later Arnis candidate review. Overture, OSM, and live Arnis observations enter this same corpus; a Nearby Building Candidate must never exist outside it.
_Avoid_: naming candidates, Arnis candidate list, nearby pool

**Campus Boundary**:
The user-confirmed polygon separating the selected Campus Target from neighboring schools and surrounding city blocks. It gates all Foundation Mode feature discovery and export.
_Avoid_: search radius, map viewport, campus center

**Foundation Feature Review Map**:
The campus-scoped review surface where discovered candidates and human-drawn buildings, roads, water, vegetation, and sports geometry are shown together over the reference map. Discovered candidates are accepted or rejected without geometry mutation; missing or incorrect geometry is replaced by a separately traceable human-drawn Map Feature.
_Avoid_: detection result, candidate list, automatic truth

**Map Interaction Mode**:
The single active interpretation of clicks on the Foundation Feature Review Map: candidate review, campus orientation, or visual recapture selection. Review mode opens candidate information; capture modes make existing overlays click-through or hidden until completion or cancellation returns to review.
_Avoid_: overlapping tools, selected layer, map state

**Campus Circulation Feature**:
A campus movement feature classified as a vehicle road, pedestrian path, steps, or paved plaza while remaining part of the Road layer. Its subtype preserves distinct width and Minecraft material intent instead of flattening every route into one generic line.
_Avoid_: generic highway, uniform road, map line

**Campus Vegetation Feature**:
A reviewed vegetation area, tree row, or individual tree inside the Campus Boundary. Area and line vegetation remain normally visible, while individual-tree points are retained as an optional collapsed detail layer.
_Avoid_: generic green area, decorative noise, vegetation pixel

**Low-Confidence Structure**:
A discovered building-like source object whose size, shape, classification, context, or cross-source evidence makes it unsuitable for the normal Foundation Feature Review Map. It remains traceable and recoverable but does not appear in normal map overlays, counts, or exports.
_Avoid_: deleted building, rejected feature, simple building

**Candidate Confidence**:
The high, medium, or low review signal used only while a Map Candidate awaits human review, derived from source reliability, geometry quality, contextual plausibility, and agreement with other evidence. Confirmation moves the candidate out of confidence queues rather than changing its confidence.
_Avoid_: source confidence, geometry score, quality label

**Candidate Review State**:
The mutually exclusive pending, confirmed, or rejected state of a Map Candidate. Pending candidates appear only in their active confidence view, confirmed candidates appear only in Confirmed, and rejected candidates remain hidden but recoverable. Revoking confirmation returns the candidate to its original confidence queue without deleting associated Detailed Building drafts.
_Avoid_: confidence, map visibility, generation flag

**Candidate Review Styling**:
The map styling where feature color communicates kind and line style communicates review state: Buildings are orange, pending candidates are dashed, and confirmed candidates are solid. Candidate Confidence is shown by the active queue and popup label, never duplicated through line width or color.
_Avoid_: confidence color, confidence line width, source color

**Confirmed Map Feature**:
A Map Feature admitted to generation by explicit human geometry confirmation. A confirmed Building additionally requires either a Campus Building Name Match or a manually supplied name and becomes a Reviewed Building Slot.
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

**Campus Boundary Source Priority**:
The order for establishing a Campus Boundary: Gaode confirms campus identity and supplies the visual base map; a name-matched open education boundary may propose the polygon; human drawing on the Gaode base map is the required fallback.
_Avoid_: feature source priority, search radius

**Foundation Feature Source Priority**:
The order for discovering Map Features after a Campus Boundary is confirmed: Overture plus OSM for buildings, OSM for roads/water/vegetation/sports, Gaode for naming and visual verification, then human correction.
_Avoid_: campus boundary priority, provider list

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

**Map Feature**:
A user-editable geographic shape in Foundation Mode, such as a building, road, vegetation region, water body, sports field, or campus boundary.
_Avoid_: layer item, object, area

**Map Candidate**:
A map-derived suggestion for a Map Feature that the user can accept or reject before export, regardless of whether it came from Arnis-style open geodata, Overture, OSM/Overpass, Gaode, screenshot analysis, or another source. Its source geometry remains immutable; geometry correction creates a separate human-drawn Map Feature.
_Avoid_: auto result, detected area

**Candidate Conflation Group**:
The set of overlapping Map Candidates believed to describe the same real feature. Review chooses one primary geometry while retaining the other source objects as supporting evidence; non-primary members are merged rather than rejected.
_Avoid_: duplicate deletion, overlapping error, rejected candidates

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
A Building Slot whose real footprint has been explicitly confirmed by a human and whose editable name comes from a Campus Building Name Match or manual entry. Only a Reviewed Building Slot enters the Building Slot Work Queue or contributes building geometry to generation; Candidate Confidence alone is insufficient.
_Avoid_: confirmed candidate, provisional slot, automatic nearest building, fixture footprint

**Foundation Manifest**:
The structured handoff file exported by Foundation Mode together with the `.schem`, containing Building Slots, Map Features, source confidence, block choices, coordinates, dimensions, and replacement intent.
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

**Schematic Previewer**:
The interactive viewer/editor for `.schem` content, including block inspection, block replacement, and export.
_Avoid_: 3D viewer, preview page

**Minecraft Block Catalog**:
The version-pinned set of official Minecraft Java block identifiers and representative textures available to style controls and block replacement. The catalog version is recorded separately from a generated schematic's palette.
_Avoid_: hard-coded block dropdown, color palette, mod block registry
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

**Parametric Building Template**:
A versioned, license-reviewed set of configurable block palettes, block arrangements, window patterns, and wall articulation rules distributed in the Campus Reconstruction Tool repository. It expresses an accepted building in Minecraft without changing its known footprint, height, floors, or other Observed Building Evidence.
_Avoid_: finished building model, fixed schematic, source building clone

**Building Template Catalog**:
The bundled collection of maintainer-authored and community-contributed Parametric Building Templates organised primarily by architectural period and construction language rather than university name. Every entry is versioned in the project repository and retains its reference lineage, license obligations, template version, and optional university case labels. Users may browse it directly or retrieve photo-matched templates from a Visual Evidence Crop.
_Avoid_: live GitHub search, external template marketplace, unreviewed model folder

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
