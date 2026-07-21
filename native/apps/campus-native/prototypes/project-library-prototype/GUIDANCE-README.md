# First-run Guidance, Settings, and Shortcuts prototype

Throwaway UI prototype for Wayfinder ticket **Prototype first-run guidance, settings, and shortcuts**.

## Run

From this directory:

```powershell
.\serve.ps1
```

Open:

- `http://127.0.0.1:4174/guidance.html?variant=A`
- `http://127.0.0.1:4174/guidance.html?variant=B`
- `http://127.0.0.1:4174/guidance.html?variant=C`

Use the bottom switcher or left/right arrow keys to cycle variants. All state is in memory.

## Variants

- **A — 分步聚焦引导**: five anchored coach marks over the real campus-first screen; skippable and reopenable with `F1` or `?`.
- **B — 任务清单＋就地提示**: a non-blocking first-run checklist plus contextual shortcut discovery.
- **C — 一页式快速开始**: a complete journey overview with screenshot quick-start, save/recovery explanation, and shortcut summary.

Every variant shares the same Settings prototype. The Shortcuts page shows live availability and disabled reasons for simulated workflow stages, text focus, modal windows, active map tools, and selected vertices.

Delete this prototype after the decision is captured and the winning interaction is rewritten for production.
