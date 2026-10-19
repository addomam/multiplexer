# Changelog

## Unreleased — Execution substrate reliability

- Correct DECOM-origin-relative CPR through the parsed ANSI Handler boundary; preserve native control behavior and independent expected replies.
- Compose joined graphemes in native narrow/wide cells, including chunked UTF-8, flags/modifiers/keycaps, CJK/combining, overwrite, wrapping and variation-width replay. Final-cluster working text is limited to 4 KiB; oversized input starts a new native scalar segment without dropping text.
- Prevent shared-config lost updates with canonical file identity, stable nonblocking writer locks, exact snapshot comparison and explicit `config-busy` / `config-conflict` errors. Sync both private replacement file and containing directory before success.
- Add daemon-wide terminal admission: default 64, configurable 1–256, counting retained exits and pending opens. Refund failed opens; clean partial integration resources and uninstalled PTY children.
- Centralize latest-20 hook/launch/enqueue diagnostics, with UTF-8-safe 16 KiB strings and explicit command/source/output truncation.
- Require bounded initialized API-v1 readiness for existing/new daemons, serialize automatic starts and clean startup sockets on failure without replacing unresponsive peers.
- Cancel active hook process groups on shutdown, reap their owned workers and skip pending hooks instead of waiting for configured timeouts.
- Breaking Rust-library change: `projects::run_hooks` now requires an explicit `&AtomicBool` cancellation argument. CLI/API framing remains version 1.
- Verify 210 workspace Rust tests, the strict independent-showcase integration, 8 OMP tests, Clippy, debug/release builds, Zsh syntax and private native CLI/TUI lifecycle probes. Retain the initial queued-execution timeout observation and exact limits in `specs/verifications/substrate-reliability.md`; no independent review PASS or live-daemon upgrade is implied.

## Unreleased — Terminal capability explorer

- Independent `terminal-showcase` Rust workspace package; amux's default binary and implementation remain unchanged.
- Fullscreen capability matrix with explicit protocol/visual evidence, eight cycling demonstrations, deterministic screenshot mode and bounded row-differential redraws.
- Independent incremental CSI/OSC/DCS/APC parser and correlated terminal queries; missing replies remain Unknown rather than Unsupported.
- Color/Unicode/typography/control specimens, Kitty/Sixel/iTerm procedural image lifecycles, advanced input inspector and explicit-consent integration actions.
- POSIX terminal/signal RAII cleanup, resize handling, bracketed-paste-safe controls, headless JSON regressions and visual pass/fail export.
- Ordinary-shell amux compatibility regression exposes origin-mode CPR and joined-emoji cursor mismatches without sharing the multiplexer oracle. Strict `--probe --check` preserves failures and exits nonzero.

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
- Client-only second latency pass caches prepared native styled cells, prioritizes bounded queued-input bursts and presents immediately with brief 1 ms interaction pacing; idle polling remains 8 ms. Stock-Zsh medians now 2.01 / 3.94 / 6.77 ms at 120 / 300 / 500 columns, and a large 24-byte burst takes 3.98 ms. Cold full-refresh outliers around 130 ms remain documented.
- Existing 66 Rust tests, 8 OMP tests, Clippy, release build and Zsh syntax pass; actual Herdr selection, Unicode/colors, picker, exit/new/empty-page detach pass. Compatible with the first-pass daemon without another process-disrupting restart. See `specs/verifications/immediate-input.md`.
- Daily-use checkpoint verifies a private locked installation outside the checkout, cold auto-start, native detach/reattach, captured context, process-preserving reparenting, busy/partial refusal, descendant cleanup, hook failure recovery and config-only restart. Retired latency workspace/checkout and empty root artifact removed with approval; committed evidence and usable build retained.

## 0.1.0 — local MVP

- Rust daemon-owned PTYs and alacritty_terminal emulator, one-surface detachable TUI, and `amux` CLI over a versioned local API.
- Unsaved/saved projects, captured Zsh workspaces, independent terminal trees, asynchronous worktree-aware Git status, configurable recipes and bounded setup hooks.
- Native stock-Zsh interaction plus confirmed empty-prompt source execution, explicit FIFO queueing, exported-context snapshots and input-epoch protection.
- OMP 18.8.7 lifecycle extension with root/child isolation, explicit working/attention/settled state, conversation identity and crash/report-loss handling.
- Sequenced snapshots/events/waits with explicit stream gaps; bounded screen/scrollback/input; detach, retained exits and descendant-aware close/shutdown.
- Native keyboard, mouse, paste, Unicode, alternate/synchronized screens, scrolling, selection and resize validation on macOS arm64. 56 Rust and 8 OMP tests pass.

Linux runtime, Rust 1.86 minimum and other OMP versions remain unverified. No crash recovery or interpreter-state cloning. Independent review reached its five-round cap; final concurrent-removal finding was reproduced and fixed, but no post-fix independent PASS is claimed. The user explicitly authorized integration into main and publication to `addomam/multiplexer` after daily-use validation. No release tag, registry publication or deployment requested. See `specs/verifications/MVP-e01.md`, `REVIEW-e01.md` and `daily-use-checkpoint.md`.