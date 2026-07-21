# PROTOTYPE — Campus-first Project Library

Throwaway UI prototype for the Wayfinder ticket **Design the campus-first project library**.

Question: what should the campus-first launcher and local Campus Project Library look and behave like so users can confirm or switch a Campus Target, resume the last campus, create or open a project, import a portable project, understand save state, and export a portable copy without file-management ambiguity?

Three variants are available on one prototype page and switch with `?variant=A`, `?variant=B`, or `?variant=C`.

Run from this directory:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\serve.ps1
```

Then open `http://127.0.0.1:4174/?variant=A`.

This prototype is not production code. All changes are in memory and reset on reload.

## Verdict

Variant A was selected on 2026-07-16 because its explicit Campus Target confirmation followed by a campus-scoped project table made the ordering and file model clearest. Variant B's persistent sidebar and Variant C's resume-first hero are not part of the V1.1 decision.
