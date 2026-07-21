# Detailed Building Mode Upgrade Backlog

## Product rule

The Local Facade Reconstruction Model proposes a **Facade Reconstruction Draft**;
it never silently produces the final Minecraft building. A human corrects or
accepts every material generation decision before it becomes confirmed output.

## First release

- Accept local building photographs and associate them with one Reviewed Building
  Slot and a selected facade.
- Allow a zero-photo start: infer Building Function Classification, propose
  templates, and generate a Template-Provisional Detailed Building for preview
  and export. It must never be presented as a complete refinement.
- Produce a confidence-scored, editable Facade Reconstruction Draft: floors,
  bays, windows, doors, facade features, roof candidates, and material labels.
- Retain the 19 fixed Arnis Style Presets as the initial base template families
  and add versioned Parametric Building Templates above them. Photo-confirmed
  rules override templates; templates fill only missing facade and roof evidence.
- Return at most three Template Match Proposals with matching rationale and
  confidence. Applying a template always requires explicit user selection.
- Infer a Building Function Classification from building names, structured map
  tags, POI/campus-directory evidence, and photo evidence. Use it as a
  high-weight template-ranking signal. Show high-confidence classifications as
  editable labels without interrupting the workflow; request a user correction
  only for low-confidence or contradictory evidence.
- Rank a confirmed project-local template from another Building Slot with the
  same Building Function Classification ahead of catalog templates and generic
  Arnis Style Presets. It remains a Template Match Proposal and is never
  applied automatically.
- Do not require architectural-period, construction-language, or subjective
  photo-style classification in the first release. Building Function
  Classification and project-local confirmation lineage provide the matching
  signals; future releases may add further optional ranking signals only after
  their value is demonstrated.
- Preserve the source photos, calibration/scale inputs, model version,
  corrections, selected template, and accepted generation rule as local project
  evidence.
- Use a non-destructive Detailed Building Rule Stack. When later photo evidence
  arrives, create a difference draft against the selected template and accepted
  overrides; never overwrite an accepted rule. The user accepts, rejects, or
  defers each proposed difference with its evidence source retained.
- Generate a deterministic Minecraft preview from the corrected draft; compare
  it with the reference photo, retain versions, and export only a confirmed
  Detailed Building.

## Automation upgrade path

1. **Assisted annotation** — improve local recognition of individual facade
   elements while retaining per-element review.
2. **Multi-photo consistency** — associate oblique and side photographs with a
   facade and highlight contradictions between views.
3. **Rule prediction** — propose repeated bays, floor rhythm, feature depth,
   and Minecraft material/rule parameters from corrected training examples.
4. **Cross-facade and roof proposals** — reconcile several facades with the
   Building Slot massing and propose a complete roof/facade rule set for review.
5. **Selective automation** — permit users to auto-accept only specific,
   high-confidence draft fields under explicit project policy; preserve the
   evidence, model version, threshold, and override trail.
6. **Opt-in training contribution** — allow a user to submit selected,
   rights-cleared corrected photo evidence to the controlled training set only
   after explicit consent and privacy processing (EXIF removal and face/plate
   masking). This is deferred from the first release; photos remain local by
   default.

Each level requires a held-out evaluation set, recorded failure cases, and
explicit human acceptance before it may replace the prior level in the shipped
default workflow.
