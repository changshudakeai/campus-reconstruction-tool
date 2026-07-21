# V1.1 overall prototype verdict

Status: decided on 2026-07-18

## Question

Which overall application shell makes the campus-first route, project durability, evidence review, honest blocked outcomes, and Minecraft/Axiom export feel like one coherent product?

## Selected direction

Variant A — route-first studio.

The explicit horizontal route, one current task, and compact right-side project context made the full V1.1 flow easiest to understand.

## Elements to keep

- The horizontal five-stage route from campus confirmation through generation/export.
- One dominant current-task workspace rather than a persistent multi-task Workbench.
- The compact right-side project summary, next incomplete task, and Minecraft compatibility summary.
- The accepted two-step Campus Target → campus-scoped project library flow.
- The three-column Boundary evidence desk and list-first five-category review within the current-task workspace.

## Elements to change

- In the Chinese locale, use clear Chinese domain terms in the primary interface. For example:
  - `Campus Target` → `校区目标`
  - `Campus Project Library` → `校区项目库`
  - `Known Feature Gap` → `已知地物缺口`
  - `Reviewed Campus Model` → `已审核校园模型`
  - `Foundation Manifest` → `校园基础清单`
- Keep unavoidable third-party product names and file/API identifiers only where needed. Technical English codes belong in advanced diagnostic/provenance details rather than primary task copy.
- The lower-left `PROTOTYPE STATE` panel and the A/B/C switcher are prototype-only evaluation controls. They must not appear in production.
- The prototype state panel is hidden by default after review feedback and can be opened only through the prototype switcher.

## Spec and ticket impact

- The PRD now records Variant A as the selected overall shell.
- Ticket 16 must implement the route-first Variant A shell without expanding into the V2 Project Workbench.
- Ticket 17 must enforce Chinese-first domain copy and exclude all prototype/debug controls from production.
- Tickets 12 and 13 retain their previously selected Boundary and Foundation Review layouts inside Variant A.
