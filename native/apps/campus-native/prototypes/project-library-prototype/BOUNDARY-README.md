# PROTOTYPE — Automatic Campus Boundary review and editing

Throwaway UI prototype for **Prototype automatic boundary review and editing**.

Question: how should users compare ranked source-backed Campus Boundary candidates, enter an explicit adjustment mode, select and drag a vertex, select an edge and insert a vertex, delete only a selected vertex, confirm the result, and recover from unavailable or invalid candidates without blank-canvas drawing?

Three structurally different variants share the existing campus/project shell:

- `boundary.html?variant=A` — three-column evidence desk.
- `boundary.html?variant=B` — guided four-step review.
- `boundary.html?variant=C` — full-screen map with contextual controls and a bottom confirmation sheet.

Run from this directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\serve.ps1
```

Then open `http://127.0.0.1:4174/boundary.html?variant=A`.

All state is in memory and resets on reload. This prototype does not call Gaode, the controlled acquisition service, or project persistence.
