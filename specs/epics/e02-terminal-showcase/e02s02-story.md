# Story e02s02: Visual dashboard and demonstrations

**type:** feat
**context:** domain
**risk:** P0

## Requirements
ADDED: Responsive categorized matrix, overview and color/Unicode/type/control specimens, navigation and deterministic screenshot frame, row-differential output.

## Steps
1. Implement this complete slice, preserving standalone endpoint boundaries (ref: PLAN.md) → verify: `cargo run -p terminal-showcase -- --headless --json`

## Acceptance
Given a viewport including 40x12, 80x24 and 120x36, all seven categories remain visible, styled rows fit cell bounds, unchanged frames emit no redraws, screenshots are reproducible and contain no blinking SGR.

## Verification script
Follow PLAN.md and the terminal-showcase usage document. Automated live integration command: `cargo build --workspace && cargo test -p terminal-showcase --test multiplexer -- --ignored`. Record actual output and manual screen captures separately from automated fixtures.

## Out of scope
No amux implementation changes, no bypass of unsupported endpoint features through internal integration, no automatic clipboard modification, no release or merge.

## Risks
P0: terminal cleanup, response ambiguity, untrusted control strings, font-dependent glyph rendering; explicit evidence and independent expectations mitigate false conclusions.
