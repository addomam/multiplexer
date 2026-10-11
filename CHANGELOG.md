# Changelog

## 0.1.0 — local MVP

- Rust daemon-owned PTYs and alacritty_terminal emulator, one-surface detachable TUI, and `amux` CLI over a versioned local API.
- Unsaved/saved projects, captured Zsh workspaces, independent terminal trees, asynchronous worktree-aware Git status, configurable recipes and bounded setup hooks.
- Native stock-Zsh interaction plus confirmed empty-prompt source execution, explicit FIFO queueing, exported-context snapshots and input-epoch protection.
- OMP 18.8.7 lifecycle extension with root/child isolation, explicit working/attention/settled state, conversation identity and crash/report-loss handling.
- Sequenced snapshots/events/waits with explicit stream gaps; bounded screen/scrollback/input; detach, retained exits and descendant-aware close/shutdown.
- Native keyboard, mouse, paste, Unicode, alternate/synchronized screens, scrolling, selection and resize validation on macOS arm64. 56 Rust and 8 OMP tests pass.

Linux runtime, Rust 1.85 minimum and other OMP versions remain unverified. No crash recovery or interpreter-state cloning. Independent review reached its five-round cap; final concurrent-removal finding was reproduced and fixed, but no post-fix independent PASS is claimed. Feature remains unmerged pending human decision. See `specs/verifications/MVP-e01.md` and `REVIEW-e01.md`.