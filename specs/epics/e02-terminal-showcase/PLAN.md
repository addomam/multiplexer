# Terminal Capability Explorer — implementation plan

## Scope and boundaries
Standalone Rust workspace package `terminal-showcase`, independent of amux. Ordinary stdin/stdout only; POSIX termios, poll and winsize for local TTY management. No amux dependency, parser, renderer, or terminal model. Existing amux behavior and default cargo run remain unchanged. Preserve the daily-use checkpoint and untracked `script`.

## Requirements (ADDED)
Deliver all eleven user requirement groups: BigPowers workflow; independent architecture; polished categorized persistent matrix and active cycling dashboard; deterministic screenshot overview and CLI; color/text, Unicode geometry, controls, graphics, input and integration coverage; bounded incremental protocol engine; state restoration; independent headless and live regressions, visual marking/export; responsive configurable animation and resize; documentation. Completion is defined by the fourteen user acceptance criteria. Missing responses never mean unsupported; acknowledgment never proves rendering. Disruptive clipboard/title/notification/cwd actions require an explicit key action.

## Contracts / Reason for Depth
- protocol: bounded incremental CSI/OSC/DCS/APC framing separates transport from evidence; handles fragmented/interleaved input without regex.
- capability: explicit status, protocol evidence, and independent visual verdict prevent false claims.
- presentation: deterministic row frames and demos allow screenshot output and differential redraw without parser assumptions.
- terminal: one RAII owner restores termios and modes on errors/signals, preventing leaked state.
- graphics: bounded procedural payloads and owned image IDs constrain image lifetime independently from rendering.

## Prior Art and dependency review
Build independent protocol engine rather than reuse alacritty or crossterm's event parser (would hide endpoint incompatibility and filter responses). Adopt existing [OK] clap 4, serde 1, serde_json 1, anyhow 1, libc 0.2, unicode-width 0.2. [OK] unicode-segmentation 1 for grapheme boundaries; [OK] base64 0.22 for inline payloads. No heavyweight TUI dependencies.
Sources: https://sw.kovidgoyal.net/kitty/graphics-protocol/ (APC G, query action a=q, direct RGB f=24, bounded chunks); https://invisible-island.net/xterm/ctlseqs/ctlseqs.html (CSI parameter/intermediate/final framing, OSC BEL/ST; DECRQM reports mode state); https://iterm2.com/documentation-images.html ; https://sw.kovidgoyal.net/kitty/keyboard-protocol/ . Environment identity is a hint only.

## Impact
Cargo workspace membership is the only runtime-adjacent existing change. Preserve `default-run = amux` and root default membership. No existing exported Rust symbols change; amux callers/contracts untouched. Existing full-suite baseline and workspace checks cover manifest compatibility. Risk: low to amux; high for terminal state safety and response correlation.

## Ordered steps and verification
1. Independent package with bounded parser, query matching, evidence model and safe guard → verify: `cargo test -p terminal-showcase`
2. Row-diff dashboard, overview/colors/unicode/typography/control demos, screenshot and CLI → verify: `cargo run -p terminal-showcase -- --headless --json`
3. Graphics lifecycle, input inspectors and consent-bound integration operations → verify: `cargo test -p terminal-showcase`
4. Real PTY and amux ordinary-shell regression, visual verdict export, docs and full gates → verify: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace`

## Verification script
Run --list, --headless --json, --probe --json in a PTY; launch full dashboard, exercise arrows/space/tab/d/s and q; resize while active; confirm termios restored. Launch identical binary via amux shell API, relay only endpoint replies, compare independently expected cursor results, and record endpoint failures honestly. Visual P/F marks apply to active demo, E exports report. Screenshot mode uses fixed frame and no event/debug changes. Inspect actual captured screen independently, not by amux parser.

## Risks and limits
Absent endpoint responses, font availability, emulator image policy and SSH latency are not support negatives. SIGKILL cannot be recovered. Visual shaping/graphics correctness needs human verdict; fixtures prove reproducibility, not glyph rendering. Existing exhausted review gate remains historical, not erased. Installed skills' repository helper scripts are absent: use actual Rust commands and record unavailable helpers; user explicitly authorizes autonomous routine stages (no approval checkpoints or commits requested).
