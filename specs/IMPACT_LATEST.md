---
type: impact
context: BUG-immediate-input; e01 native-terminal
---
# Immediate-input impact

## Target
Client-native projection/cache and input/frame pacing in src/tui.rs and src/tui/view.rs. No daemon, local API, PTY, Zsh, OMP or process-lifetime change.

## Dependents
- LSP references: view::draw has one production caller, tui::run, plus its declaration.
- refresh_screen and merge_screen retain native JSON/viewport state for tui::run and existing delta-contract tests. Their private signature gains a projection cache.
- Selection/text extraction still uses retained JSON; mouse/key/paste translation still uses complete screen metadata. Client-local cursor/focus and renderer geometry remain intact.

## Affected stories
The existing e01 native-terminal presentation and validation slice; no deferred client/tiling/protocol feature.

## Test coverage
Existing row-delta tests cover untouched styles, changed rows/cursor/modes, metadata-only updates and rejection of stale/out-of-bounds frames. Extend that contract to the cached native projection. Real controlling-PTY and Herdr UI smoke covers clipping, wide/combining text, selection, picker/empty/exit actions and native input. Runtime and shell-race suites intentionally unchanged because their contracts do not change.

## Risk: Medium
Cache invalidation can leave stale styled cells or wide-glyph residues; scheduling can reorder/starve UI events. Preserve validated full/delta boundaries, bounded queued-event priority and old idle cadence. No independent review-cap reset.

## Recommended action
Proceed with the measured projection hotspot and interaction-only pacing; profile/measure the actual wrapper afterward. No new dependencies or generic abstractions.
