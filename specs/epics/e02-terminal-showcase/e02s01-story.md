# Story e02s01: Evidence-based terminal explorer

**type:** feat
**context:** domain
**risk:** P0

## Requirements
ADDED: Independent workspace package, bounded incremental CSI/OSC/DCS/APC parsing, read-only queries with exact correlation and conservative classifications, POSIX RAII state and signal safety.

## Steps
1. Implement this complete slice, preserving standalone endpoint boundaries (ref: PLAN.md) → verify: `cargo test -p terminal-showcase`

## Acceptance
Given fragmented/malformed/interleaved endpoint responses, parser events remain bounded and query matches consume only valid timely replies. Silence is Unknown, not Unsupported. Normal exit/signals restore termios, queried modes, keyboard stack and image IDs.

## Verification script
Follow PLAN.md and the terminal-showcase usage document. Automated live integration command: `cargo build --workspace && cargo test -p terminal-showcase --test multiplexer -- --ignored`. Record actual output and manual screen captures separately from automated fixtures.

## Out of scope
No amux implementation changes, no bypass of unsupported endpoint features through internal integration, no automatic clipboard modification, no release or merge.

## Risks
P0: terminal cleanup, response ambiguity, untrusted control strings, font-dependent glyph rendering; explicit evidence and independent expectations mitigate false conclusions.
