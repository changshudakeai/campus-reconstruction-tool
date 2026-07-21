# Coarse Raster Supplement prototype

Throwaway UI prototype for Wayfinder ticket **Define the coarse raster supplement contract**.

## Run

From this directory:

```powershell
.\serve.ps1
```

Open:

- `http://127.0.0.1:4174/raster.html?variant=A`
- `http://127.0.0.1:4174/raster.html?variant=B`
- `http://127.0.0.1:4174/raster.html?variant=C`

The bottom switcher and left/right arrow keys cycle variants. All state is in memory. Example thresholds, dates, classes, digests, and vectorisation rules are illustrative contract fields rather than final algorithm choices.

## Question under test

How should Sentinel-2 and WorldCover observations be constrained to structured-source gaps, shown as coarse evidence rather than precise geometry, reviewed with fixed warnings, and persisted with reproducible lineage?

## Variants

- **A — 证据对照台**: queue, one contextual map, full contract details, and a persistent decision footer.
- **B — 左右证据对比**: side-by-side structured-only and structured-plus-raster views to emphasize what the supplement actually adds.
- **C — 四步契约检查**: guided gate sequence from structured gap through pinned data and simplification to human review.

Delete this prototype after the decision is captured and the winning interaction is rewritten for production.
