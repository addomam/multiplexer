---
type: bug-fix-plan
context: e01 native-terminal; Ratatui cutover9c57ecb; Herdr parallel worktree
---
# Input latency and empty-window navigation

## Reproduce
User reports ~500ms input latency in the real TUI and a seemingly frozen/inescapable window list after closing a window. These observations are accepted, not re-run merely to confirm them. Platform: macOS arm64, Herdr, amux Ratatui 0.30.0 cutover at 9c57ecb.

## Isolate
- Empty-list keyboard dispatch currently has no chosen row for Enter; Escape leaves the picker enabled when active is empty. Both are deterministic no-ops in that state.
- Empty-list rendering shows only a Ctrl-N message; Ctrl-Q detach exists but is absent from the visible hint, and Return cannot return to a removed window.
- Latency is a separate full-wrapper path: TUI input, control IPC, PTY write, foreground echo, emulator generation, screen read and client presentation. Assign isolated investigation to Herdr agent amux-latency, workspace w11, branch perf/input-latency, worktree .worktrees/input-latency forked from current feature HEAD. Preserve main-checkout UI work and user focus.

## Hypothesize
1. Empty-list navigation is inescapable through the advertised actions because the UI has no active target and no explicit empty-state actions. Falsification: native TUI after last removal; New/Exit mouse buttons and Enter/N/Escape/Q keys must perform visible actions and detach must restore the host terminal.
2. Full-wrapper latency may be stale screen generation, synchronous screen/IPC overhead, or adapter overhead. Worker must profile stage costs, not assume the existing CLI-to-emulator benchmark explains TUI latency.

## Verify / design decisions
Code inspection establishes the empty-state no-op branches and hidden existing detach shortcut; the reported interaction follows directly. Fix only presentation/actions: explicit centered New/Exit card, top-right Exit, Enter/N to create, Escape/Q to detach when there are no windows. Keep ordinary picker fuzzy q search and Escape return semantics; show Ctrl-Q detach first in the ordinary picker hint. Zero search results with existing windows are distinct from having no windows. Detach leaves daemon running. Shared geometry controls rendering and mouse hitboxes; no footer, background-PTY resize, or runtime-tree changes.

## Implementation increments
1. Delegate isolated latency diagnosis/implementation. Worker owns IPC/runtime/adapter and TUI hot-path helpers/scheduling, not view.rs or picker action branches. Parent integrates/reconciles; no mid-flight builds/lints/tests/formatters.
2. Implement local empty-state keyboard/mouse/rendering changes while worker runs.
3. Review/integrate scoped optimization; run mechanical checks once after completed changes and full TUI keystroke-to-visible-echo smoke under foreground/background load.
4. Exercise empty-window mouse/keyboard recovery/exit, ordinary fuzzy search, native application input and resizing. Record actual measurements and limits, update usage/changelog. Existing independent review cap remains exhausted; no claim of Linux/minimum-Rust validation or a new independent gate pass.

## UI increment resolution
Actual native smoke passed explicit empty New/Exit mouse and keyboard actions, compact layout, host prompt recovery, daemon preservation and shutdown cleanup; see `specs/verifications/UI-empty-switcher.md`. Smoke also caught a stale initial snapshot misclassifying a freshly opened active window: empty behavior now requires no active ID as well as no records. The ordinary fuzzy-q startup scenario then passed. Latency was subsequently integrated and verified below.

## Latency resolution and final verification
Integrated de14ddc as d413016 after parent UI checkpoint ab0917b. Root cause: input dirtiness forced full native grids on the event thread, compounded by per-key socket accept/roundtrip. Bounded row damage, atomic generation publication, same-viewport reconstruction, borrowed input serialization, persistent framing and eight-millisecond polling remove that repeated whole-grid path without changing source authority or native echo.

Final66 Rust/8 OMP tests, Clippy, release build and Zsh syntax pass. Actual900 single-key measurements and native/Herdr action, input-mode, partial-buffer, tree, resize, detach, framing and process cleanup proof are in `specs/verifications/input-latency-final.md`. Added consumer-visible delta/framing regressions cover skipped generations, multi-client independence, modes/cursor/Unicode/styles, full fallbacks, sync flush and rejected stale deltas. No permanent UI wording tests or suppressed diagnostics. Five-second persistent idle expiry/full-damage traversal/physical monitor paint remain explicit limits; inherited review cap unchanged.
