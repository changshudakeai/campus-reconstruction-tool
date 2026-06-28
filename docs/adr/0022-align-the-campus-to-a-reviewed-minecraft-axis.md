# ADR-0022: Align the campus to a reviewed Minecraft axis

## Status

Accepted

## Decision

The user draws a campus direction line and the application derives one Campus Orientation shared by Foundation and Detailed Building output. It defaults to the smallest rotation that aligns the line with Minecraft X or Z, while allowing axis selection, reversal, and angle adjustment; source coordinates and provenance remain unchanged.

Campus direction drawing is an exclusive Map Interaction Mode. Campus Boundary and feature overlays are click-through, so the direction line may start and end anywhere on the campus map rather than only on uncovered background.

Campus Orientation must be confirmed before the user begins feature-candidate review. Until the direction line is confirmed, building, water, sports, vegetation, and road review controls stay locked with a prompt to draw the main campus direction first.
