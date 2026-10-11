# Changelog

## Unreleased — Ratatui UI

- Ratatui 0.30.0 rendering with the existing Crossterm 0.29 input/backend and Rust minimum raised to 1.86.
- Three-row title/tab bar and top-right Switch button; native terminal fills every remaining row with no footer.
- Full-screen hierarchical fuzzy window list with two-line rows, direct click/Enter selection and retained launch/tree shortcuts.
- Actual exit-code view with Close window and New terminal buttons, keyboard alternatives, compact stacked layout and empty-list recovery.
- Native wrapper smoke verified Unicode/palette colors, application cursor, mouse coordinates, safe bracketed paste, focus, literal prefix, alternate-screen restoration, picker/rapid resize, exit codes 0/7/9, close/new/detach and shutdown. Existing 56 Rust tests, Clippy and release build pass.
- Explicit New/Exit empty-window card and top-right Exit; Enter/N starts a terminal, Escape/Q detaches to the host without stopping the daemon. Visible picker Ctrl-Q hint and fresh-window guard preserve fuzzy search before snapshot refresh.
- Native input hot path now reuses a deadline-bound connection, serializes borrowed input bytes and transfers damage-row screen updates with atomic generation publication; eight-millisecond polling keeps UI dirtiness separate from native damage.
- Actual TUI PTY measurements: typical120×40 shell median10.81ms;500×163 median30.34ms versus direct baseline428.85ms. Separate noisy output/50,000-row scrollback keeps large raw echo median30.39ms. Full-frame redraws and physical compositor paint remain explicit limits.
- Integrated66 Rust tests,8 OMP tests, Clippy, release build, Zsh syntax and actual Herdr empty-page recovery pass; one-shot EOF, persistent framing/error continuation and bounded idle expiry verified. Paused/undrained clients may disconnect after five seconds without requests; no hidden replay.

## 0.1.0 — local MVP

- Rust daemon-owned PTYs and alacritty_terminal emulator, one-surface detachable TUI, and `amux` CLI over a versioned local API.
- Unsaved/saved projects, captured Zsh workspaces, independent terminal trees, asynchronous worktree-aware Git status, configurable recipes and bounded setup hooks.
- Native stock-Zsh interaction plus confirmed empty-prompt source execution, explicit FIFO queueing, exported-context snapshots and input-epoch protection.
- OMP 18.8.7 lifecycle extension with root/child isolation, explicit working/attention/settled state, conversation identity and crash/report-loss handling.
- Sequenced snapshots/events/waits with explicit stream gaps; bounded screen/scrollback/input; detach, retained exits and descendant-aware close/shutdown.
- Native keyboard, mouse, paste, Unicode, alternate/synchronized screens, scrolling, selection and resize validation on macOS arm64. 56 Rust and 8 OMP tests pass.

Linux runtime, Rust 1.85 minimum and other OMP versions remain unverified. No crash recovery or interpreter-state cloning. Independent review reached its five-round cap; final concurrent-removal finding was reproduced and fixed, but no post-fix independent PASS is claimed. Feature remains unmerged pending human decision. See `specs/verifications/MVP-e01.md` and `REVIEW-e01.md`.