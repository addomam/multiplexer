# Story e02s04: Independent compatibility regression and delivery

**type:** feat
**context:** domain
**risk:** P0

## Requirements
ADDED: Headless JSON regressions, real PTY state/resize/signal regression, ordinary amux shell compatibility exercise, visual verdict export, documentation and quality evidence.

## Steps
1. Implement this complete slice, preserving standalone endpoint boundaries (ref: PLAN.md) → verify: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace`

## Acceptance
Given a real amux daemon shell, this binary runs without internal Rust integration and independently compares cursor response coordinates; a grapheme mismatch is exposed as Failed rather than silently accepted. Reports distinguish objective and visual evidence.

## Verification script
Follow PLAN.md and the terminal-showcase usage document. Automated live integration command: `cargo build --workspace && cargo test -p terminal-showcase --test multiplexer -- --ignored`. Record actual output and manual screen captures separately from automated fixtures.

## Out of scope
No amux implementation changes, no bypass of unsupported endpoint features through internal integration, no automatic clipboard modification, no release or merge.

## Risks
P0: terminal cleanup, response ambiguity, untrusted control strings, font-dependent glyph rendering; explicit evidence and independent expectations mitigate false conclusions.
