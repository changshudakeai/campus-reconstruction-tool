# ADR-0033: Acquire Foundation evidence through a controlled service

## Status

Accepted

## Context

The current hosted bridge queries only Overture buildings, defaults to a drifting `latest` release, and silently limits results to 200, while the desktop directly calls public Overpass instances. That shape cannot prove complete five-category coverage, preserve one data version across retries, or reliably resume a nationwide campus query.

## Decision

V1.1 retrieves OSM and Overture boundary and Foundation evidence only through the Controlled Foundation Acquisition Service. Production uses a service-managed Pinned OSM Snapshot or private Overpass database and an exact Overture Release; public Overpass remains developer-only and is never a desktop or production fallback. The service receives no project file, review state, Minecraft settings, Gaode identity or credentials, and users interact only with the desktop application.

A separate boundary job uses the Campus Target's name, aliases, WGS-84 anchor, and bounded search envelope to return OSM/Overture education-area candidates in a Boundary Discovery Snapshot. Confirming one candidate starts a resumable five-category Foundation Acquisition Job using the same immutable Foundation Dataset Bundle. Initial acquisition and explicit update checks request Building, Circulation, Water, Vegetation, and Sports together; provider/category/tile work completes and retries independently within that bundle.

The service owns adaptive tiling, stable pagination, complete OSM relation assembly, Overture parts and `sources[]`, provider-neutral typed Source Observations, Review Geometry Proposals, delivery-duplicate removal, raw spatial measures, classification and conflation suggestions, and replayable coverage. It collapses only identical upstream record/version/content deliveries. Changed versions and merely overlapping records remain separate evidence for desktop review.

Coverage is complete only when every required provider/category/tile page reaches an explicit end and every required relation member is present. Limits, timeouts, missing pages or members, cancellation, and provider failure produce complete-empty, partial, failed, or cancelled outcomes with structured errors; no count cap or HTTP success implies completeness. Successful provider and category evidence survives independent failures and may enter a local baseline with retryable Known Feature Gaps.

The versioned `/v1` API exposes health, capabilities, boundary-job and acquisition-job creation, job status, scoped retry, cancellation, manifests, and resumable chunks. Requests are idempotent, jobs retain their contract and bundle, and identical replays yield identical canonical content digests. Results use versioned JSON manifests and gzip NDJSON Source Observation chunks with explicit coordinate/unit/time semantics and SHA-256 chunk and whole-result verification. Temporary job results remain available for at least 30 days; they are delivery state, not cloud projects.

Production transport uses validated HTTPS and an installation-scoped Acquisition Service Credential stored separately from Gaode credentials. Capabilities advertise contract/schema versions, supported bundles, quotas, retention, and area, vertex, tile, observation, and result-size limits. Oversized or incompatible requests fail before work and never clip or downgrade silently. Structured transient failures receive at most three version-preserving exponential-backoff attempts.

Every result carries complete Source Lineage and an Acquisition Licence Manifest. The Foundation Source Matrix queries relevant OSM and Overture themes in parallel, while WorldCover-derived land cover remains labelled coarse evidence. Classification, geometry repair, overlap, and grouping thresholds are versioned suggestions calibrated on V1.1 acceptance campuses; they never automatically accept, reject, merge, or delete distinct source records.

The contract is published as OpenAPI and JSON Schema with shared Python/Rust fixtures covering complex geometry, relations, lineage, provider outcomes, completeness, licences, corrupt chunks, and replay. Both service and desktop releases must pass the same contract suite.

## Consequences

- The existing building-only synchronous endpoint and desktop public-Overpass path are migration inputs, not V1.1 release architecture.
- Shared caches may be reused only under an exact bundle/provider/category/tile/rule/schema cache identity and must reproduce the same coverage and content digests.
- The desktop remains authoritative for local Source Snapshots, all review and naming decisions, the Reviewed Campus Model, generation, and export.
- Service loss prevents new acquisition or refresh but never authorizes an unpinned provider fallback.
