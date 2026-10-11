type: verification_report
context: input-latency scoped handoff on perf/input-latency; baseline 9c57ecb

# Input latency: diagnosis and integration handoff

## Evidence and root cause

The user's roughly half-second real TUI echo report is ground truth. Profiling used the existing `/Users/addomam/projects/multiplexer/target/release/amux`, private socket/config/projects directory and private ZDOTDIR, actual `amux attach` inside an outer PTY, and stock `/bin/zsh`. Input was written to the outer PTY, not submitted through the CLI input API. Visible echo was the actual TUI's emitted terminal output decoded with pyte, not a daemon screen query. Physical display/Herdr compositor paint is outside this measurement.

Direct observations, without an IPC proxy (milliseconds; p95 is the floor-index empirical percentile for these small samples):

| Scenario | Keys | p50 | p95 | Max |
| --- | ---: | ---: | ---: | ---: |
| Fresh stock Zsh, 300×103 outer viewport | 12 | 128.19 | 183.28 | 186.58 |
| Fresh stock Zsh, 500×163 outer viewport | 10 | 428.85 | 431.86 | 431.95 |
| Raw foreground echo application, 300×103 | 12 | 190.85 | 231.98 | 239.06 |

A direct raw 21-byte burst took 522.00 ms to reach the TUI output. The raw application restored its saved termios state on exit.

Separate instrumented socket-proxy observations (not directly comparable to unproxied latency; proxy response JSON parsing adds CPU/GIL overhead):

| Scenario | Keys | Echo p50 ms | Input accept roundtrip p50 ms | Screen roundtrip p50 ms | Screen response bytes, approximately |
| --- | ---: | ---: | ---: | ---: | ---: |
| Stock Zsh 120×40 | 24 | 46.99 | 2.94 | 13.44 | 702,104 |
| Fresh stock Zsh 300×103 | 12 | 162.76 | 3.69 | 71.55 | 4,740,927 |
| Fresh stock Zsh 500×163 | 10 | 707.50 | 20.61 | 212.94 | 12,641,201 |
| Raw foreground 300×103 | 12 | 168.61 | 3.26 | 70.57 | 4,740,896 |
| Zsh with separate noisy terminal, 300×103 | 12 | 192.83 | 5.47 | 72.52 | 4,740,971 |

A proxied raw 24-byte burst took 1,343.48 ms and produced 24 separate input requests. The noisy terminal printed a line every 10 ms. The noisy run followed an earlier raw application which did not restore termios; use the fresh/unproxied rows for clean baseline latency claims. One later profiling marker wrapped across rows and was not recognized by the original single-row matcher; that attempt was excluded, inspected, and not rerun to reconfirm the failure. Future recipes must join soft-wrapped rows or keep unique markers within one row.

Source and stage evidence:

- Original TUI frames wait 34 ms after completion. Every input marks UI dirty; dirtiness nulls `since`, forcing a complete native grid even when native content did not change. Each key makes a new socket connection and waits for its input response. Drawing/polling/JSON decoding all share the TUI event thread.
- The daemon serializes a JSON object for every cell while holding the emulator mutex. At large viewports this blocks native advancement and makes the TUI process megabytes to show one changed row.
- The nonblocking daemon accept loop sleeps 5 ms on WouldBlock. Per-key fresh connections and sequential acknowledgements compound burst cost. The PTY queue's ordinary input operation acknowledges queue admission, not foreground echo; writer barriers are reserved for source-execution authority checks. Those barriers must not be removed.
- Original generation publication occurred after dropping the emulator lock on PTY advance and sync flush; the screen fast-path also sampled generation twice. That permits inconsistent snapshot/generation tagging. Changed rows and generation must be published together.
- `_amux_redraw` already reports ready only when the partial-input bit changes. Ordinary subsequent keystrokes do not spawn/report per redraw; `_amux_schedule` does no external epoch/wake work without pending source. Large delays persist on subsequent nonempty-buffer keys and in the raw foreground app. ZLE readiness/epoch/wake checks are not bypassed or modified. Their independent process cost was not timed.

## Chosen semantics

- TUI reuses one deadline-bound connection. Requests opt in with `persistent:true`; existing one-shot clients retain EOF-after-response behavior. Input uses borrowed typed serialization instead of constructing a per-byte JSON Value tree. No reconnect, replay, retry, or new protocol version.
- TUI polls on an 8 ms frame interval. UI dirtiness no longer invalidates the native screen. Terminal/offset/limit changes force a full refresh. Drawing remains parent-owned and unchanged.
- `screen` with `row_updates:true` can return a damage-row delta only for the live viewport, with a suitable generation. Delta metadata includes `delta:true`, `base_generation`, `row_indices`, and only those rows in `cells` and `lines`; normal cursor/modes/dimensions/history/offset/generation remain complete. The TUI moves retained rows into the new snapshot, preserving the existing `view::draw` contract.
- The daemon keeps bounded row-generation bookkeeping, not unbounded per-client screen histories. Old generations, resize/full damage, palette/alternate-screen/scroll changes, and history-offset views fall back to full native snapshots. Generation and damage publication use the same emulator lock.
- Source-vs-raw separation, native empty-buffer/typeahead authority, input epoch/gate safety, PTY queues, IDs, version 1, event gaps, shell reporting, and cleanup remain authoritative. Cached display state never authorizes execution.

## Risks and verification status

No candidate build, lint, tests, formatter, or candidate runtime smoke was run: the task explicitly reserves them for the parent's final verification. No after-latency value or instant-echo guarantee is claimed. Regression tests added to owned files are unexecuted. The independent review cap remains exhausted; this is not a reviewer PASS.

Important final checks: damage reset/publication across skipped generations and multiple clients; sync-output release/timeout; metadata-only changes; full fallback for scroll/alt-screen/palette/resize/history; TUI delta reconstruction; error framing and buffered persistent requests; one-shot EOF; idle expiry/shutdown; input ordering while source barriers hold the gate; native bytes/control/Unicode/key/mouse/paste. Persistent connections expire after five seconds without a request; TUI normally polls at most every 500 ms even in the picker, but a long local stall can surface a disconnect (no hidden replay). Full-damage screens remain expensive; this change primarily removes whole-grid cost for small native updates. `view::draw` still traverses the full displayed surface. Herdr adds host parsing/paint and scheduling costs not included here.

## Before/after measurement recipe

1. Preserve the baseline executable before replacing it. After integration, run the final checks once, then compare that baseline to the candidate release artifact. Give each run a fresh mode-0700 temp directory, private `AMUX_SOCKET`, `AMUX_RUNTIME_DIR`, `XDG_CONFIG_HOME`, projects directory, and ZDOTDIR. Set a known plain prompt only in that temporary `.zshrc`; do not modify user/global startup or readiness hooks.
2. Launch `<binary> --socket <private-socket> daemon --projects-dir <private-projects>` in a neutral temp cwd. Use API `snapshot` and native `wait shell-ready` only to set up the chosen terminal. Launch `<binary> --socket <private-socket> attach <stable-terminal-id>` as an actual process connected to an outer PTY. Set outer PTY winsize before spawning (120×40, 300×103, 500×163). Continuously drain/decode its emitted ANSI output; do not time CLI input-to-screen.
3. Type a unique marker through the outer PTY, observe it, then inject one next ASCII key at a time using `os.write(master, key)`. Measure monotonic time immediately before that write through arrival/render decoding of the exact extended marker in the TUI output. Use at least 100 keys per scenario with fixed spacing; keep markers within a row or join soft wraps. Include empty-to-partial transitions separately. Do not let an old scrollback marker satisfy the observation.
4. Repeat with a foreground Python/C raw echo process launched through the actual shell; it must save and restore termios in a finally/RAII block, emit a unique readiness sentinel, and echo bytes without sleeps. Repeat single keys and a 24-byte burst. Include control/non-ASCII bytes and bracketed paste checks separately, without pretending they are ordinary character echo samples.
5. Repeat with a separately opened terminal producing lines every 10 ms; record viewport, key rate, samples, p50/p95/max, CPU/load and whether running inside Herdr. Perform a final real Herdr wrapper smoke and visually observe echo/switching/resizing. This host-compositor observation is separate from PTY-output timing.
6. For stage attribution only, add an owned Unix-socket proxy recording request arrival, request op, response completion, bytes and generation. It must forward multiple newline frames on persistent connections and preserve read-ahead. Avoid parsing complete grids on the observer hot path; offload recorded parsing until after the run. Keep proxy timings separate from unproxied end-to-end values.
7. Close only owned noisy/raw terminals, request shutdown on the private daemon, wait for the daemon/TUI processes, and delete only the run's temp files. Never target the user's live socket or main checkout.

Suggested final verification batch, once after integration:

```sh
cargo test && cargo clippy --all-targets -- -D warnings && cargo build && cargo build --release
```

Then exercise the actual candidate `target/release/amux` with the private-resource PTY recipe above and the parent-owned Herdr wrapper smoke. Do not report these deferred commands as passing before observing their results.
