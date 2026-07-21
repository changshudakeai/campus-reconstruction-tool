# PROTOTYPE — V1.1 overall product route

Throwaway UI prototype that connects the accepted V1.1 product decisions into one clickable route.

## Question under test

Which overall application shell makes the campus-first route, project durability, evidence review, honest blocked outcomes, and Minecraft/Axiom export feel like one coherent product?

This is a UI prototype, not an implementation. All state lives in memory. It does not call Gaode, OSM, Overture, the Controlled Foundation Acquisition Service, project persistence, generation, or Axiom.

## Run

From PowerShell:

```powershell
.\serve-overall.ps1
```

Then open:

- `http://127.0.0.1:4175/overall.html?variant=A`
- `http://127.0.0.1:4175/overall.html?variant=B`
- `http://127.0.0.1:4175/overall.html?variant=C`

The floating bottom switcher and the left/right arrow keys cycle variants. Arrow keys are ignored while an input is focused.

The header switches between:

- the normal Putuo full-flow route; and
- a simulated NYU Shanghai Qiantan `boundary-unavailable` route.

## Variants

- **A — Route-first studio:** horizontal stage route, one current task, and a compact sticky Project Context.
- **B — Map-first field notebook:** a dark vertical route rail with the active task occupying the main field workspace.
- **C — Evidence cockpit:** mission rail, active work surface, and persistent evidence/durability/gate dock.

All three variants preserve the already accepted local decisions:

- explicit Campus Target confirmation before the campus-scoped project table;
- three-column automatic Boundary evidence review with selection-first vertex editing;
- list-first five-category Foundation review;
- coarse evidence as a gap detail rather than precise geometry;
- five-step reopenable First-Run Guidance;
- Minecraft Java Edition 26.1.2 from preview through `.schem` export.

## Feedback to capture

1. Which variant makes the full route easiest to understand without explanation?
2. At each screen, is the primary next action obvious?
3. Is Project Context visible enough without becoming a V2 Project Workbench?
4. Does `boundary-unavailable` feel like an honest, recoverable product result?
5. Does Foundation Review clearly distinguish completed work, pending candidates, and acknowledged-but-open Known Feature Gaps?
6. Does completion/export communicate the current project revision and Minecraft 26.1.2 contract clearly?
7. Which parts should be combined across variants?

Record the verdict in `OVERALL-NOTES.md`, then delete the losing variants and rewrite the selected decisions in production code rather than promoting prototype code.

## Verdict

Variant A was selected on 2026-07-18.

The production implementation should use Chinese domain terms throughout the Chinese locale and must not include the prototype state panel or variant switcher. See `OVERALL-NOTES.md` for the accepted wording and ticket impact.
