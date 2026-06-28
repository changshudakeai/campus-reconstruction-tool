# Campus Reconstruction Tool

Native-first desktop tool for the ECNU Putuo Campus Minecraft reconstruction workflow.

> Architecture transition: the current Tauri/React UI is the compatibility implementation. V1 is migrating application workflow to a Rust-native Slint shell; only Gaode 3D and the voxel renderer remain isolated web graphics surfaces. See [`docs/adr/0031-use-a-native-desktop-shell-with-isolated-web-graphics.md`](docs/adr/0031-use-a-native-desktop-shell-with-isolated-web-graphics.md).

## Current Slice

- Foundation Mode and Detailed Building Mode are the primary app structure.
- The first target is ECNU Putuo Campus.
- The first representative building is the Putuo Campus Library.
- The app uses React, TypeScript, Three.js-ready dependencies, and Tauri configuration.
- The Minimal Arnis Adapter contract outputs Building Geometry before `.schem` generation.
- The product starts by confirming a Campus Target through Gaode; canonical names and aliases share one campus scope used by both modes.
- Foundation Mode runs campus-scoped live map queries that produce editable Map Candidates with source, confidence, geometry, and provenance.
- Foundation Mode can accept or reject Map Candidates, add a manual closed boundary, and build a reviewed Foundation Manifest with Building Slots.
- Detailed Building Mode can generate a Putuo Campus Library schematic model from Building Geometry, preserving a non-rectangular footprint and hipped roof hint.
- The schematic path writes a minimal Sponge v2 NBT byte structure with palette and varint block data for the later Axiom export flow.
- Detailed Building Mode includes a first Three.js Schematic Previewer with orbit controls, block inspection, batch block replacement, and updated `.schem` export.
- The web UI has a language selector with English and Simplified Chinese labels for the main Foundation Mode and Detailed Building Mode flows.
- Online Map Query has live Gaode POI and OSM/Overpass providers; fixtures are available only through the explicit offline test flag.
- Overpass queries retry against alternate nodes with progressively smaller bounded radii and cache successful results.
- Foundation Mode can export a reviewed Foundation Manifest into an Axiom-Compatible `.schem` by rasterizing campus, building, road, vegetation, water, and sports Map Features.
- Foundation Mode includes a first SVG Geometry Editor for loading candidate geometry, adding points, nudging selected points, and closing a draft into a reviewed manual Map Feature.
- Foundation Mode includes road-width and per-feature block style controls that affect the reviewed Manifest and Foundation `.schem` export.
- Online Map Query includes provider-level in-memory caching plus a provenance debug panel for cache hit/miss, provider role, raw IDs, and notes.
- Foundation Mode previews estimated export dimensions, total block volume, non-air block count, palette size, and size risk before downloading the `.schem`.
- Foundation Mode exports the reviewed Foundation Manifest JSON alongside the Foundation `.schem` so Detailed Building Mode has a stable handoff file.
- Foundation Manifest handoff records selected blocks, coordinates, approximate dimensions, confidence, and provenance for Map Features and Building Slots.
- The Foundation Manifest explicitly marks the Putuo Campus Library as the representative Building Slot, including Chinese `图书馆` name matching, so Detailed Building Mode does not depend on slot order.
- Detailed Building Mode now consumes the selected Building Slot from the Foundation Manifest, converts it into a Building Target, and shows the slot handoff summary before generation.
- The Minimal Arnis Adapter path now includes a Foundation Manifest Slot Handoff provider, so Building Geometry provenance records the selected slot, source feature, selected block, raw ID, and fallback footprint.
- Detailed Building Mode now presents an explainable Building Geometry summary with footprint, height, floors, roof and facade hints, per-field confidence, source priority, used sources, missing fields, provenance notes, and Foundation Manifest handoff details.
- Detailed Building Mode queries the local Overture GeoJSON bridge and OSM/Overpass for live candidates. If both sources fail, generation is blocked with an explicit recoverable error; it never silently substitutes a fixture.
- Detailed Building Mode begins with a campus-building naming stage. It loads campus-bounded Overture footprints, shows ten source objects per page, and lets users focus each footprint in Gaode 3D before saving a recognizable name.
- Automatic naming uses four concurrent Gaode reverse-geocode calls for the current page. Every success, no-match, and failure is cached by campus and source ID so repeated visits do not consume another API call.
- Reverse-geocoded names must begin with the selected school, canonical campus, or campus alias. Other objects are stored as local suppression tombstones and removed from future campus candidate loads; users may also delete incorrect candidates locally.
- Campus Building Directory records merge bundled/shared JSON, the desktop app-data annotation file, and browser-local records. Saved names immediately participate in campus building search and can be exported as a GitHub-ready campus annotation file.
- OSM and Overture candidates with different IDs reuse reviewed names or exclusions through a bounded WGS-84 center match, so nearby Arnis cards do not lose names solely because the provider changed.
- Gaode and nearby Arnis candidates are paged at ten items. Arnis cards can focus their location in Gaode 3D and save a human name into the campus-scoped Building Directory.
- Campus selection filters Gaode results to campus-level POIs only; parking lots, gates, canteens, departments, and individual buildings are excluded.
- Building search results are constrained by both the confirmed campus radius and campus/school identity text, so nearby off-campus POIs cannot enter the confirmation list.
- Candidate acquisition shows an elapsed-time, provider-stage progress panel while Rust queries OSM and the local Overture cache, avoiding an ambiguous blank wait.
- Building identity uses the confirmed Gaode POI name first, then OSM/Overture names. When providers omit names, the single nearest footprint within 120 metres is promoted as a location-based review candidate, never auto-accepted. Duplicate OSM/Overture footprints are collapsed.
- The evidence workspace stacks an enlarged Gaode 3D reference above Arnis candidates, and the Foundation Manifest sidebar can collapse so the main workspace expands.
- Foundation and Detailed exports open a native folder chooser and save `.schem` plus the matching manifest/provenance JSON through the Rust backend. Exported `.schem` files are gzip-compressed Sponge v2 NBT.
- Detailed Building Mode now queries OSM/Overpass after Overture and before Foundation Manifest data, filling missing height, floors, roof, facade, or footprint fields without overriding higher-priority values.
- Detailed Building Mode now supports explicit manual overrides for height, floors, roof and facade hints, plus switching to the reviewed Building Slot footprint; applying a correction regenerates the schematic and records `manual_correction` provenance.
- The current Detailed Mode handoff from reviewed Building Slot through Building Geometry into a fresh previewable `SchematicModel` is covered by an end-to-end regression test, including regeneration after manual correction.
- The Three.js preview renders generated models with palette-grouped `InstancedMesh` blocks and on-demand redraws, capped device pixel ratio, low-power WebGL preference, orbit controls, selection highlighting, and complete GPU resource cleanup.
- Clicking a visible preview block shows its Minecraft block type, integer `x/y/z` coordinates, and palette index in an accessible live inspection panel.
- Batch Block Replacement shows the current match count, replaces every instance immutably, permits replacing a solid block with air, and blocks expensive air-source or same-type no-op operations.
- A completed Batch Block Replacement immediately feeds the new model identity back into the Three.js preview, clears stale inspection state, and announces the replacement count as a status message.
- Detailed Building Mode exports the current edited model through a validated Sponge v2 `.schem` contract and reports the filename, dimensions, palette size, and NBT byte count.
- Detailed export preserves source priority, used providers, missing fields, notes, Foundation Slot handoff, manual corrections, and Batch Block Replacement history in compact NBT metadata plus a readable `.provenance.json` companion file.
- PRD stories 31-35 are closed with documented legacy lineage, live/offline provider paths, typed architecture seams, and a complete offline First Vertical Slice acceptance test.

## Environment

Create a local `.env` from `.env.example` when using live Gaode POI search. Configure Overture on the Tauri process so its endpoint and release stay outside browser code:

```bash
VITE_GAODE_WEB_SERVICE_KEY=your_gaode_web_service_key
VITE_GAODE_POI_ENDPOINT=https://restapi.amap.com/v3/place/text
VITE_GAODE_REGEOCODE_ENDPOINT=https://restapi.amap.com/v3/geocode/regeo
VITE_OVERTURE_BUILDING_ENDPOINT=http://127.0.0.1:8765/overture/buildings
VITE_CAMPUS_ANNOTATION_BASE_URL=/campus-building-annotations
OVERTURE_BUILDING_ENDPOINT=https://your-local-service.example/overture/buildings
OVERTURE_RELEASE_ID=2026-05-20.0
```

`VITE_GAODE_POI_ENDPOINT` can point to a local or Tauri-backed proxy if direct browser calls are blocked by CORS or key exposure policy.

The repository includes a local Overture bridge. It reads the current official Overture Parquet release with HTTP range requests, stores only the intersecting row groups under `.cache/overture`, and returns GeoJSON to Tauri. It does not download a complete release. The first query in a new region can take about 1–3 minutes depending on the network; repeated queries in that region use the local cache.

The Python bridge is a development/reference service, not part of the V1 installer. Production Overture querying and shared caching belong to the hosted data service described in [`docs/deployment-boundary.md`](docs/deployment-boundary.md).

On Windows, configure or run the development bridge separately with:

```powershell
npm run overture:setup
npm run overture:serve
```

Its health endpoint is `http://127.0.0.1:8765/health` and its building endpoint is `http://127.0.0.1:8765/overture/buildings`. The desktop launcher starts this service automatically. The bridge accepts `lng`, `lat`, `radius_m`, `bbox`, `limit`, and optional `release` parameters, and exposes read-only CORS headers for the local development UI. Tauri bounds requests to 250 metres and 200 features, applies response-size limits, and records release, query bounds, feature IDs, MultiPolygon parts, and interior rings in export provenance.

Shareable campus building names are stored in `public/campus-building-annotations/`. Add one campus JSON file and register it in `index.json`; after a Campus Target is confirmed, the matching file is loaded automatically. Desktop edits are also persisted under the app-data `campus-building-annotations` directory and can be exported from the naming stage for later review and contribution.

## Commands

```bash
npm install
npm test
npm run build
npm run dev
npm run desktop:dev
```

On Windows, double-click `start-app.cmd` for the same desktop startup. It starts the local Overture bridge first, injects the backend endpoint, and then starts Tauri. Keep its terminal window open while using the desktop application.

Open the dev server at:

```text
http://127.0.0.1:1420
```

Tauri desktop startup also requires the Rust toolchain (`rustc`, `cargo`, and `rustup`) to be installed locally.
