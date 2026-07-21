# Campus AOI automatic boundary research

Date: 2026-07-14

## Question

Which supported data sources can V1.1 use to propose a Campus Boundary automatically, and what should happen when no trustworthy campus polygon is available?

## Findings

### Gaode

The normal Gaode POI Search 2.0 response describes POI identity and location; it does not document a campus boundary polygon. Gaode's separate Enterprise Intelligent Map APIs document AOI queries and WKT geometry, so AOI geometry must be treated as an separately entitled enterprise capability rather than assumed to work with an ordinary Web Service or Web JS key. Sources: [POI Search 2.0](https://lbs.amap.com/api/webservice/guide/api/newpoisearch), [Enterprise API guide](https://lbs.amap.com/api/me-api/guide), [AOI fence query](https://lbs.amap.com/api/me-api/documents/fence/AOI), [point-to-AOI query](https://lbs.amap.com/api/me-api/documents/other/other/aoi).

Gaode coordinates use the Gaode coordinate system. Any Gaode AOI geometry must retain its original coordinate system and be transformed explicitly before comparison with WGS-84 open data. Source: [Gaode coordinate conversion](https://lbs.amap.com/api/javascript-api-v2/guide/transform/convertfrom).

The product must not infer that a rendered Gaode campus fill or screenshot may be scraped into a reusable boundary dataset. Without a separately confirmed licence or written permission, screenshot-derived boundary extraction is not a supported V1.1 source. Source: [Gaode platform service agreement](https://lbs.amap.com/pages/terms/).

### OpenStreetMap and Overpass

OSM can represent a school or university as a closed way or a multipolygon relation. A usable acquisition path must assemble complete relation rings, retain inner rings, and reject broken or self-intersecting geometry rather than treating individual relation members as boundaries. Sources: [amenity=university](https://wiki.openstreetmap.org/wiki/Tag:amenity%3Duniversity), [amenity=school](https://wiki.openstreetmap.org/wiki/Tag%3Aamenity%3Dschool), [multipolygon relations](https://wiki.openstreetmap.org/wiki/Relations/Multipolygon).

Overpass can query those ways and relations, but generated Areas are a query convenience rather than a substitute for retaining the source object's identity and geometry. Public Overpass instances have shared-resource quotas and may reject or shed expensive requests, so a public instance should not be V1.1's only production boundary backend. Sources: [Overpass QL](https://wiki.openstreetmap.org/wiki/Overpass_API/Overpass_QL), [Overpass Areas](https://wiki.openstreetmap.org/wiki/Overpass_API/Areas), [Overpass commons and quotas](https://dev.overpass-api.de/overpass-doc/en/preface/commons.html).

OSM geometry is WGS-84 and carries ODbL attribution/share-alike obligations that must remain visible in provenance and distribution decisions. Sources: [OSM WGS-84](https://wiki.openstreetmap.org/wiki/WGS84), [OSM copyright and licence](https://www.openstreetmap.org/copyright), [OSMF licence FAQ](https://osmfoundation.org/wiki/Licence/Licence_and_Legal_FAQ).

### Overture Maps

Overture Places are point-like place records and Divisions describe administrative divisions; neither should be treated as a campus boundary source. Overture Base `land_use` can contain polygonal `school`, `college`, or `university` land use and is the relevant Overture theme for a boundary candidate. Sources: [Place schema](https://docs.overturemaps.org/schema/reference/places/place/), [Divisions guide](https://docs.overturemaps.org/guides/divisions/), [LandUse schema](https://docs.overturemaps.org/schema/reference/base/land_use/), [LandUseClass](https://docs.overturemaps.org/schema/reference/base/types/land_use_class/).

Overture land-use records may inherit OSM lineage, so an Overture polygon must not automatically count as independent cross-source agreement with the matching OSM object. The application must retain dataset version, source lineage, and required attribution. Sources: [Overture getting data](https://docs.overturemaps.org/getting-data/), [Overture attribution and licensing](https://docs.overturemaps.org/attribution/).

## Recommended V1.1 policy

1. Use an authorised Gaode Enterprise AOI polygon when the deployment has explicit entitlement; ordinary Gaode keys do not imply this capability.
2. Query complete OSM school/university ways and relations through a controlled service, with Overture Base `land_use` as a versioned supplementary snapshot rather than independent confirmation when its lineage is OSM.
3. Normalize candidate geometry to WGS-84 while retaining the original geometry, coordinate system, provider identity, source object identity, licence, and transformation lineage.
4. Rank eligible candidates by canonical campus name and aliases, containment or proximity of the confirmed Gaode anchor, plausible area, valid topology, and source lineage. Do not merge same-lineage OSM and Overture records as independent votes.
5. Preload the strongest candidate for human confirmation. Allow switching candidates and editing vertices, but do not require blank-canvas drawing as the normal path.
6. Do not use Gaode screenshot or rendered-fill extraction in V1.1 without explicit permission that covers derivative boundary data.
7. When no trustworthy polygon exists, report `Campus Boundary unavailable` with provider-specific retry information. Whether this blocks the project or permits a constrained correction fallback remains a product decision.

## Implication for the existing ADR

ADR 0018 is directionally correct about OSM ring assembly and human confirmation, but V1.1 should make the automatic path the primary workflow, explicitly model Enterprise Gaode entitlement, remove unsupported Gaode screenshot extraction as a boundary source, and avoid relying on a public Overpass instance as its sole production backend.
