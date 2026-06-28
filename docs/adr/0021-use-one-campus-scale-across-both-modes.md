# ADR-0021: Use one Campus Scale across both reconstruction modes

## Status

Accepted

## Decision

Campus Scale is chosen inside a Campus Reconstruction Project as a real-world meters to Minecraft blocks ratio and is shared by that project's Foundation Mode, every Building Slot, Detailed Building refinement, and generation style. It is not owned by the Campus Target, because one campus may have multiple reconstruction projects with different scale choices. It may change freely before the first confirmed refinement or export; later changes are explicit project rescaling operations that regenerate outputs and mark refinements for review.
