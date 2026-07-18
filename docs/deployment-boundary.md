# Deployment boundary

| Capability | Local desktop | Hosted service |
|---|---:|---:|
| Project files and review decisions | Required | Never required |
| API credentials | Required, OS credential store | Never stored |
| Arnis building generation | Required | No |
| Foundation/detailed schematic export | Required | No |
| Building template catalog (V1 fixed styles) | Cached and versioned | Release distribution |
| Gaode 3D reference | Isolated map surface | Provider infrastructure |
| OSM/Overture Foundation acquisition | Authenticated typed `/v1` client; no public-provider fallback | Required controlled service with pinned Dataset Bundle |
| Shared campus annotations | Local cache | Optional |
| App/model update manifest | Local cache | Required for updates |

Hosted service failure must not block opening, editing, regenerating, or exporting an existing project.
It pauses new acquisition and refresh. Service rollback may deploy only a contract-compatible
`/v1` implementation and must preserve the pinned provider and Dataset Bundle identities.

## Size policy

Rust `target/` is build cache, not application payload. It is excluded from Git and installers. V1 Windows installers have a 50 MB budget, checked by `npm run size:release`. Large datasets, Python environments, training data, and model weights are not bundled.
