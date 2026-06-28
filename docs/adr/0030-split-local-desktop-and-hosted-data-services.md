# Split local desktop responsibilities from hosted data services

V1 uses a local-first desktop application with a narrow hosted data plane.

The local application owns user projects, reviewed geometry, annotations, API credentials, deterministic Arnis generation, schematic preview, block editing, provenance, and all exports. These functions must continue to work without a project account and must never require uploading a campus project.

Hosted services own large or frequently changing shared datasets: bounded Overture queries and cache, application/model release manifests, and optional reviewed community annotations. Services return source records and provenance rather than generated schematics. OSM/Overpass and user-configured Gaode access may remain direct fallbacks where provider terms permit.

This boundary keeps private work and compute-heavy generation local while removing the Python/GeoParquet environment and shared cache from end-user installation. Service APIs are versioned; loss of service degrades discovery but does not prevent opening, editing, generating, or exporting an existing project.
