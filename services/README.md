# Hosted data service

This directory is deployed separately from the desktop installer. It provides bounded Overture building queries and a shared GeoParquet row-group cache. It does not receive project files, generate schematics, or store desktop credentials.

```powershell
docker build -t campus-reconstruction-data-service services
docker run --rm -p 8765:8765 -v overture-cache:/var/cache/overture campus-reconstruction-data-service
```

Endpoints:

- `GET /health`
- `GET /overture/buildings?lng=...&lat=...&radius_m=...&limit=...`

Production deployment should terminate TLS in front of the container, restrict request rates and response sizes, persist the cache volume, and set the current `OVERTURE_RELEASE_ID`.
