# Story e01s02: Safe shell automation and context

## Requirements
ADDED: Deliver safe shell automation and context according to SCOPE_LATEST.yaml and DESIGN_PLAN_LATEST.md.

## Steps
1. Implement the smallest working path, preserving public boundaries → verify: `cargo test --test integration`

## Acceptance
Given a managed local terminal, when this slice is used, then native interaction and independent lifecycle ownership remain intact.
See the design and the user vertical slice for boundary, failure and cleanup scenarios.

## Out of scope
Explicit exclusions in SCOPE_LATEST.yaml.

## Risks
P0: process lifetime, readiness races, integration fidelity.
