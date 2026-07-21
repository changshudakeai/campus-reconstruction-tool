# Licensed imagery for Chinese university feature recovery

Date: 2026-07-14

## Question

Is there an imagery source that covers arbitrary Chinese universities, is sufficiently detailed for campus feature recovery, and permits machine analysis plus storage of derived geometry?

## Conclusion

No verified source simultaneously provides nationwide Chinese-university coverage, zero-cost access, permission to create and retain derived geometry, and approximately 0.3–1 m spatial detail.

V1.1 should therefore use the controlled five-layer OSM/Overture service as the nationwide default. Visual recovery can accept either user/university-authorised orthophotos or commercial 30–50 cm imagery whose contract explicitly permits machine analysis, derived-vector retention, and the intended distribution. It must not depend on unlicensed Gaode screenshots.

The 0.3–1 m target is an engineering inference for resolving campus road edges, small water bodies and vegetation patches, pitches, and running tracks. Products at 10–30 m can support coarse land-cover evidence but cannot reliably restore those geometries.

## Open sources

### Copernicus Sentinel-2

Sentinel-2 provides four bands at 10 m and is free and openly usable under the Copernicus terms. Official STAC confirms Shanghai coverage, including the Putuo sample area, but 10 m pixels are too coarse for precise campus feature recovery. Sources: [Sentinel-2 collection](https://dataspace.copernicus.eu/data-collections/copernicus-sentinel-missions/sentinel-2), [Sentinel licence](https://cds.climate.copernicus.eu/licences/ec-sentinel), [Putuo-area sample item](https://stac.dataspace.copernicus.eu/v1/collections/sentinel-2-l2a/items/S2A_MSIL2A_20260713T024141_N0512_R089_T51RUQ_20260713T061409).

### ESA WorldCover

WorldCover is a global 10 m classified land-cover product under CC BY 4.0. It may contribute coarse vegetation or land-cover evidence, but it is not high-resolution source imagery. Source: [ESA WorldCover data access](https://esa-worldcover.org/en/data-access).

### Landsat 8/9

Landsat 8/9 provides 30 m multispectral and 15 m panchromatic data. USGS distributes the data at no cost and describes it as public domain, but its resolution is insufficient for campus geometry. Sources: [Landsat 8/9 OLI/TIRS archive](https://www.usgs.gov/centers/eros/science/usgs-eros-archive-landsat-archives-landsat-8-9-operational-land-imager-and), [Landsat Collection 2 Level-1](https://www.usgs.gov/landsat-missions/landsat-collection-2-level-1-data).

### OpenAerialMap

OpenAerialMap imagery is published under CC BY 4.0 and is suitable for download, analysis, and derivatives when adequate imagery exists. Coverage is contributed rather than continuous. An official API query around Putuo returned three records with roughly 1000–1180.7 m GSD, which is unusable for campus recovery. Sources: [OpenAerialMap legal terms](https://openaerialmap.org/legal/), [Putuo-area API query](https://api.openaerialmap.org/meta?bbox=121.39,31.21,121.42,31.24).

## Commercial sources

### Planet

PlanetScope is approximately 3.7 m; SkySat products are sampled at 50 cm. SkySat is technically relevant, but Planet's current terms distinguish permitted derivative products from restrictions on imagery caching, extraction, and multi-user distribution. A desktop product needs a contract that expressly covers its workflow. Sources: [Planet imagery products](https://www.planet.com/products/satellite-imagery-of-earth/), [SkySat documentation](https://docs.planet.com/data/imagery/skysat/), [Planet terms](https://www.planet.com/terms-of-use/).

### Airbus Pléiades Neo

Pléiades Neo provides 30 cm imagery. Airbus publishes country archive coverage and a standard Living Library licence that can allow derivative works irreversibly separated from source pixels, but the actual order licence and Chinese-region delivery still require contractual confirmation. Sources: [Pléiades Neo coverage](https://space-solutions.airbus.com/imagery/our-optical-and-radar-satellite-imagery/pleiades-neo/country-coverage-imagery-archive/), [standard Living Library licence](https://www.intelligence-airbusds.com/files/pmedia/public/r51461_9_standard-licence-livinglibrary-210319.pdf).

### Maxar/Vantor Vivid Mosaic

Vivid Mosaic advertises 30 cm HD and global land coverage except Greenland and Antarctica. Rights to analyse, retain derived vectors, and redistribute results depend on the customer agreement and must not be inferred from product availability. Source: [Vivid Mosaics availability](https://pro-docs.maxar.com/en-us/VividMosaics/VividMosaics_available.htm).

## Chinese map imagery

Gaode's ordinary platform terms restrict scraping, screenshots, caching, extraction, and derivative works; it is not a supported visual-recovery input without separate written authorisation. Source: [Gaode platform terms](https://lbs.amap.com/pages/terms/).

The Shanghai Tianditu copyright page states ownership and citation expectations but does not expressly grant machine extraction and retention or distribution of derived geometry. That permission must not be assumed. Source: [Shanghai Tianditu copyright statement](https://shanghai.tianditu.gov.cn/map/views/about.html?type=3).

## User or university-authorised imagery

A user-owned orthophoto or university-authorised aerial/planning image can be licensed specifically for machine analysis, local or hosted storage, derived-vector retention, and sharing with project users. Drone capture also carries separate flight, registration, airspace, surveying, and privacy obligations. Source: [Civil Aviation Administration of China UAV rules](https://www.caac.gov.cn/XXGK/XXGK/FLFG/202401/t20240115_222642.html).

## V1.1 recommendation

1. Make structured OSM/Overture layers the only nationwide default.
2. Keep visual recovery optional and source-gated.
3. Accept user/university-authorised orthophotos through an explicit rights confirmation record.
4. Support commercial imagery only after a reviewed contract covers analysis, derived geometry, retention, and distribution.
5. Use coarse Sentinel-2/WorldCover only as low-confidence land-cover context, never as precise geometry.
6. Preserve imagery date, resolution, licence, provider, model/algorithm version, and derived-feature confidence in the Campus Reconstruction Project.
7. When no eligible imagery exists, retain structured-data residuals as known feature gaps rather than using unauthorised imagery or invented geometry.
