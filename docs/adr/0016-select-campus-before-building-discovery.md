# ADR-0016: Select a Campus Target before building discovery

## Status

Accepted

## Context

The application previously embedded Putuo Campus into query types, Gaode search expansion, map centers, and mode copy. This made a building query appear to succeed while returning candidates associated with the same fixed campus. It also left Foundation Mode and Detailed Building Mode with separate notions of geographic scope.

Users reconstruct a specific school campus. Campus names can have provider and colloquial aliases, while renamed unnamed buildings must remain reusable only inside the selected campus.

## Decision

The application requires a user-confirmed Campus Target before either reconstruction mode opens. The Campus Target owns its canonical name, aliases, confirmed Gaode position, and discovery radius. Foundation and Detailed Building queries derive their scope from it.

Human-assigned names are stored in a Campus Building Directory keyed by the traceable source object. They improve later search and display but do not mutate source geometry or provenance.

## Consequences

- Switching campus resets mode-local discovery state.
- Campus aliases resolve to one target instead of producing parallel projects.
- Building searches no longer receive an implicit Putuo prefix.
- A source object can have a useful local name even when OSM or Overture does not provide one.
- Existing Putuo fixtures remain test assets and are not a product fallback.
