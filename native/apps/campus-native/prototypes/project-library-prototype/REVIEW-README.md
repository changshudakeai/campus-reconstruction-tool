# Foundation Review prototype

Throwaway UI prototype for the Wayfinder ticket **Prototype Foundation review without feature drawing**.

## Run

From this directory:

```powershell
.\serve.ps1
```

Then open:

- `http://127.0.0.1:4174/review.html?variant=A`
- `http://127.0.0.1:4174/review.html?variant=B`
- `http://127.0.0.1:4174/review.html?variant=C`

The bottom switcher and the left/right arrow keys cycle variants. All state is in memory. The prototype does not call Gaode, OSM, Overture, Sentinel-2, WorldCover, or the controlled service.

## Question under test

How should five independently completed Foundation Feature Categories expose candidate review, provenance, batch decisions, provider failure, coarse evidence, Known Feature Gaps, and explicit completion without reintroducing blank-canvas feature drawing?

## Variants

- **A — 五层证据台**: persistent category rail, central map, evidence panel, and always-visible completion bar.
- **B — 待办队列工作台**: list-first review queue with map context and a progress footer.
- **C — 全屏地图审核**: map-first review with floating category/gap panels and a bottom decision sheet.

Delete this prototype after the decision is captured and the winning interaction is rewritten for production.
