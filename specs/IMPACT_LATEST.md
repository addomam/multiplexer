# Responsive Menu and shortcut help

## Target
Native Menu geometry/rendering/input in `src/tui/view.rs`, `src/tui.rs` and the alternate ownership header in `src/tui/control.rs`, plus saved-project dialog button geometry in `src/tui/project.rs`. Continue on `feat/cli-projects-help`, preserving the uncommitted Menu/project and prior UI/CLI repairs. Pre-change baseline: 456 Rust tests and actual installed Menu proof in `specs/verifications/menu-projects.json`.

## Dependents
- `src/tui.rs`: painted geometry drives mouse action/list hit targets and paging. Modal input must remain consumed, never forwarded/replayed to native/ACP work.
- `src/tui/view.rs`: Menu and native/exited views share header/body geometry. Keep native PTY dimensions and full repaint on return unchanged.
- `src/tui/project.rs`: saved-project list/save/edit/forget dialogs share responsive footer layout, retaining drafts, confirmation, Tab navigation and original connection mutations.
- `src/tui/control.rs`: alternate locked/pending header uses the same responsive search and help hint, retaining state/conditional warnings. Help occupies the body only, below the warning header; authority/consent logic and clipped-warning admission checks remain unchanged.
- `tests/pty_control/native.rs`: actual controlling-PTY ownership/menu regressions.

LSP references for `view::geometry` again return no references despite the existing attached/draw calls. Use existing inspected call sites and compilation; this known tool limitation does not imply no dependents.

## Decisions
- Remove menu keyboard legends and shortcut suffixes on action buttons; retain a short help hint only when it fits completely.
- Help is an Escape-dismissable, wrapping/scrollable local overlay below the ownership header. Menu `?`, Ctrl+/ (legacy byte0x1f decoded by Crossterm as Ctrl-7) and distinct enhanced Ctrl+? open it. Never steal Backspace/DEL: legacy Ctrl+? cannot be distinguished from Backspace. Plain `?` in project text editors remains text.
- Horizontal buttons when full labels fit; vertical buttons on narrow screens. Use compact one-line buttons when height cannot afford borders, preserving content rows. Shared paint/hit geometry; zero-size buttons are not hit targets.
- Apply the footer rule to Menu and saved-project dialogs, including only active buttons. Help does not execute actions, mutate projects, acquire ownership, resize background applications or replay input.
- No daemon/protocol/storage changes, no global daemon/config/auth changes, no providers, commit/merge/push or release.

## Affected stories
E01 native Menu/project workflow and E04/E06 native/ACP input ownership boundaries. Preserve historical E03 unknown-settlement failure, parked E05 and exhausted live/review budgets.

## Test coverage
Existing 456-test baseline covers native repaint, ownership fences, saved-project transitions/rollback and CLI behavior. Add only boundary coverage for responsive non-overlapping usable hit regions and help/backspace/release discrimination. Actual continuously drained controlling-PTY smoke must prove wide, narrow/tall and short layouts, help wrap/scroll/Escape, physical stacked button clicks and no application input replay, followed by normal process/termios/drainer cleanup.

## Risk: Medium
Shared layout affects mouse routing and modal ordering; native body dimensions and authority remain unchanged. One integration owner implements the tightly coupled UI slice inline. Completion requires actual terminal proof and regression checks, not a rendered-buffer-only claim.

## Completed verification
Final 458-test/strict/install and actual local/locked controlling-PTY proof, preserved findings/failures and normal owned cleanup: `specs/verifications/responsive-menu.json`. No new provider/reviewer acceptance or historical cap changes. The sole disk cleanup was the explicitly approved rebuildable incremental cache; normal builds resumed and completed.
