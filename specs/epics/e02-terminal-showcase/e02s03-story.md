# Story e02s03: Advanced graphics and input

**type:** feat
**context:** domain
**risk:** P0

## Requirements
ADDED: Bounded Kitty/Sixel/iTerm direct procedural rasters with explicit legacy opt-in, image lifecycle, enhanced keyboard/mouse/focus/paste inspectors and explicit-consent integration actions.

## Steps
1. Implement this complete slice, preserving standalone endpoint boundaries (ref: PLAN.md) → verify: `cargo test -p terminal-showcase`

## Acceptance
Given graphics support advertisement or explicit G consent, images are transmitted using direct terminal bytes and only owned IDs are deleted. Pasted hotkeys cannot execute clipboard/title/notification/cwd actions. Received key kinds, modifiers and pointer coordinates are decoded independently.

## Verification script
Follow PLAN.md and the terminal-showcase usage document. Automated live integration command: `cargo build --workspace && cargo test -p terminal-showcase --test multiplexer -- --ignored`. Record actual output and manual screen captures separately from automated fixtures.

## Out of scope
No amux implementation changes, no bypass of unsupported endpoint features through internal integration, no automatic clipboard modification, no release or merge.

## Risks
P0: terminal cleanup, response ambiguity, untrusted control strings, font-dependent glyph rendering; explicit evidence and independent expectations mitigate false conclusions.
