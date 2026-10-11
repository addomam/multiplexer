---
type: verification
context: BUG-input-latency-empty-switcher; parent UI slice on feat/amux-mvp
---
# Empty-window switcher verification

## Build
`cargo fmt && cargo build --release` passed after the final guard correction. Existing Rust suite/Clippy will be run against the combined latency/UI changes; this UI increment does not claim a fresh suite run.

## Native surface proof
Ran the actual release TUI as a foreground program in a managed host Zsh, attached to a separate private daemon containing only the displayed window. Read the resulting UI through the host daemon's alacritty emulator. Separate runtime directories are required for two daemons: attempting two socket names in one runtime directory correctly returned `already-running: daemon runtime is locked`; smoke setup was corrected, not the daemon.

- Actual natural exit9 rendered its exit-code card. X removed the sole exited record and displayed a centered No windows card, New terminal and Exit buttons, plus top-right Exit and keyboard hints. No footer.
- Mouse New created a live stock-Zsh terminal. Enter and N also created terminals from the empty page.
- Empty Q, Escape, mouse-card Exit and top-right Exit each detached to a **confirmed host Zsh prompt**. The controlled daemon remained responsive with zero window records: UI exit is not daemon shutdown.
- Existing-window picker kept q as fuzzy-search input, showed the no-matches message and visible Ctrl-Q hint; Escape returned to the native terminal. Ctrl-Q detached with the child still alive.
- Smoke exposed a newly-auto-opened-window edge: its cached initial snapshot was empty briefly, despite a nonempty active ID. Empty-state behavior now requires **both no active target and no terminal records**, preventing ordinary fuzzy q from accidentally detaching before the snapshot refresh. Re-exercised that startup timing through the real TUI successfully.
- At38×22, the empty card stacked Exit/New buttons, retained the daemon-running explanation, and the native mouse New hitbox started a terminal.
- Deliberate shutdown of both smoke daemons exited0; tracked managed roots were gone and both sockets removed.

## Limits
This is actual UI/action proof, not a latency benchmark. The latency worktree remains independent until integration and full-wrapper measurement. No permanent UI wording/layout tests added; actual native interaction catches behavior rather than source text. No new dependencies, API changes, shell-adapter changes or runtime-parentage changes in this increment. Inherited independent-review cap remains exhausted; no new independent pass, Linux runtime pass or minimum-Rust-toolchain pass claimed.
