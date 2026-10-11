---
type: verification
context: BUG-input-latency-empty-switcher; Herdr worktree de14ddc integrated as d413016 after UI checkpoint ab0917b
---
# Native input latency and empty-page verification

## Verdict
Final contiguous `cargo fmt && cargo test -- --nocapture && cargo clippy --all-targets -- -D warnings && cargo build --release && bun test integrations/omp && zsh -n integrations/zsh/adapter.zsh` exited0: **66 Rust tests**, one intentionally ignored subprocess fixture; **8 OMP tests**, Clippy denying warnings, release build and Zsh syntax pass. Exact output: `input-latency-preflight.log` (artifact293). The first combined run passed tests but found a collapsible-if Clippy error; corrected without suppression. Final error-stage context was then included in the complete passing batch.

## Direct actual TUI measurements
macOS arm64, Rust1.95 release, stock Zsh5.9; private HOME/ZDOTDIR/config/socket and daemon, plain native prompt. Actual `amux attach` ran with a controlling outer PTY, sized before spawn or through SIGWINCH. Keys were written **to the TUI PTY** and timed until their characters appeared in emitted Ratatui terminal output. No CLI-input substitute, socket proxy or fake local echo. Physical monitor/compositor paint is not timed. Each row contains100 sequential individual keys; p95 is the empirical floor-index percentile. Background load first produced55,000 lines (retained scrollback capped50,000), then one line every10ms in a separate terminal.

| Foreground | Outer viewport | Background load | p50 ms | p95 ms | Max ms |
|---|---|---|---:|---:|---:|
| Stock Zsh | 120×40 | No | 10.81 | 11.16 | 19.84 |
| Stock Zsh | 300×103 | No | 17.49 | 17.80 | 31.33 |
| Stock Zsh | 500×163 | No | 30.34 | 31.09 | 53.71 |
| Raw native echo | 120×40 | No | 10.66 | 10.93 | 10.98 |
| Raw native echo | 300×103 | No | 17.44 | 17.86 | 18.58 |
| Raw native echo | 500×163 | No | 30.35 | 31.10 | 31.95 |
| Raw native echo | 120×40 | Yes | 10.67 | 11.00 | 11.21 |
| Raw native echo | 300×103 | Yes | 17.50 | 17.94 | 21.34 |
| Raw native echo | 500×163 | Yes | 30.39 | 31.07 | 31.29 |

A direct24-byte raw burst at500×163 finished rendering in25.61ms. These are900 observed single-key samples, not an instant-echo guarantee. Agent's pre-change **direct**, unproxied baselines: stock Zsh300×103 p50/p95 128.19/183.28ms(n12); stock Zsh500×163 428.85/431.86ms(n10); raw300×103 190.85/231.98ms(n12); raw21-byte burst522ms. Baseline used a pyte decoder while candidate measured emitted character bytes; samples differ, so comparisons are indicative, not an identical-observer benchmark. Proxy timings and their observer overhead remain explicitly separate in `input-latency-agent.md`.

## Runtime and actual Herdr proof
- Full-wrapper foreground probe saved/restored termios, used alternate screen, custom palette, app cursor, mouseSGR, bracketed paste and focus. Captured actual Up→ESC O A, paste with embedded ESC removed but Unicode界é retained, mouse `(10,5)` after subtracting three header rows, focus ESC[I, literal Ctrl-B from prefix twice, and CLI raw text. Foreground shell-source request returned `busy`; after raw-process exit native readiness returned and the same TUI remained alive.
- Partial native line rejected structured execution as **`partial-input`**, not busy or a silent replacement. Clearing it restored confirmed readiness. Promote/reparent kept the noisy process PID unchanged. Rapid80×24→180×56→40×18→300×103→120×40 resize survived. Detach exited0 and preserved both live processes.
- Real `amux` launched in an **unfocused owned Herdr tab**, preserving caller focus. Herdr's decoded native surface showed the three-row header, edge-to-edge terminal, native typed `print HERDR_NATIVE_UI` output, full-screen hierarchical picker and visible Ctrl-Q hint. CLI screen read observed that same command output. Actual synchronized foreground output appeared and settled back to native prompt.
- Native exit7 showed its actual exit card. X removed the sole record and displayed explicit New/Exit empty-page actions. N created a fresh live Zsh. Subsequent exit0→X→Q returned to the original **Herdr host shell**, not a frozen list, and left the daemon responsive. Mouse/Escape/Enter/compact coverage also passed in the separately recorded parent UI increment (`UI-empty-switcher.md`). The owned smoke tab was closed; the latency-agent worktree/workspace remains available.
- Closing the noisy terminal removed both its shell and foreground Python descendant. All owned smoke daemons shut down0, tracked roots and sockets disappeared. Smoke scripts/startup files were removed; user files and user Herdr panes were untouched.
- Actual API socket smoke verified one-shot EOF, three buffered persistent requests, explicit error followed by a valid next response, and idle EOF after5.0s. Shutdown remained responsive.

## Observer failures and limits
A repeated X after shrinking a raw alternate screen could overwrite an identical last-column glyph, so the observer incorrectly awaited a newly emitted X. That attempt was excluded; alternating unique keys prevented this false wait. Separate harness sessions left PTY output undrained between tools, stalling client presentation long enough to encounter the deliberate five-second persistent idle expiry. Controlled draining kept the client alive; undrained output led to disconnect, attributed through `setting control response timeout`. On Darwin the expired socket surfaced EINVAL. A continuously draining bounded observer and real Herdr eliminated those harness stalls. No reconnect/replay or error suppression was added; raw daemon interaction remained available throughout.

Full-damage/scroll/resize/palette/alternate-screen frames still fall back to full JSON; rendering still traverses the native surface. Large/full-changing applications can cost more than these small-echo numbers. Persistent clients must issue requests within five seconds; paused/blocked clients may detach with a visible error, without killing managed processes. No Linux runtime, Rust1.86 toolchain or physical-display latency claim. Original independent-review cap remains exhausted, not reset/passed by this revision.

## Self-audit and upgrade
- Scope/security: no dependencies, shell patches, screen-scraping, retries, protocol version change or generic framework. Same-user private-socket trust and request/response/input caps remain; row bookkeeping is bounded, per-terminal and independent of clients.
- Interfaces: LSP references for the removed public generation accessor identified its daemon callsite and declaration; its callers now use locked screen-update publication. CLI/extension EOF verified rather than assumed. UI geometry/draw contract unchanged.
- Behavioral coverage: new delta/framing tests catch stale/missing rows, cursor/mode/style loss, multi-client gaps and malformed frames; native UI proof covers actual actions rather than wording. Baseline large runtime/TUI modules are intentionally not refactored merely for size.
- Error context distinguishes control timeout/encoding/write/read and input poll/draw failures; no suppression or hidden replay.
- Existing daemons need an explicit restart to load changed runtime code. Shutdown terminates managed jobs; no user's live daemon was restarted. Run daemon/TUI from the same build. No crash/process restoration was added.
- Original independent-review cap remains exhausted; no fresh independent PASS or default-branch merge.
