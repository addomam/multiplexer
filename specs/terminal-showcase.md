# Terminal Capability Explorer

## Build and run

```sh
cargo build -p terminal-showcase --locked
cargo run -p terminal-showcase
cargo install --path terminal-showcase --locked
terminal-showcase --list
terminal-showcase --demo unicode
terminal-showcase --screenshot
terminal-showcase --screenshot --demo colors
terminal-showcase --probe --json --report /tmp/terminal-capabilities.json
terminal-showcase --probe --check --json --report /tmp/compatibility.json
terminal-showcase --headless --json
```

This is a separate package with no dependency on amux, alacritty_terminal, ratatui or crossterm. Terminal bytes travel exclusively through stdin/stdout. POSIX termios/poll/winsize manage the local TTY; there is no endpoint file/socket/shared-memory graphics path. Root `cargo run`, `cargo build` and `cargo test` retain amux as the default package; use `--workspace` for both packages.

## Inside amux, nested multiplexers and SSH

Start amux, attach an ordinary shell, and invoke `terminal-showcase` or the absolute `target/debug/terminal-showcase` path. No integration extension, daemon introspection or special renderer path is used by the application. Over SSH, run the binary on the remote host; consider `--fps 6` on slower links. Nested wrappers that deliver quoted tmux DCS replies are parsed to a bounded depth; the explorer does not bypass a multiplexer using environment-driven passthrough guesses.

**The endpoint is whatever communicates with the application's stdin/stdout.** Inside amux, that is amux's emulated terminal, not necessarily Ghostty/Kitty/WezTerm underneath it. A physical terminal's graphics support does not imply a multiplexer implements or forwards it. These emulators and Linux/SSH combinations have not all been visually verified by this change.

Live interactive and `--probe` modes require both stdin and stdout to be TTYs. `--probe --json` displays JSON after completing terminal I/O; captured raw stdout also contains query/cleanup escape bytes. Use `--report PATH` for clean machine-readable live JSON, not stdout redirection. Piped/headless JSON contains only JSON.

## Dashboard and controls

The persistent matrix has seven categories and a six-status legend. Large viewports label individual badges with compact feature abbreviations; medium viewports retain ordered per-feature status ribbons; small viewports keep all categories as conservative aggregate badges. Tab exposes named evidence for the current demonstration, with `[` / `]` for paging. An aggregate mixed category remains Unknown (or Failed if any feature failed), never an inferred PASS.

| Key | Action |
|---|---|
| Space | Pause/resume animation and automatic cycling |
| Left / Right | Previous / next demonstration |
| Tab | Toggle expanded capability evidence |
| D | Toggle detailed diagnostics and unmatched-event information |
| S | Toggle deterministic screenshot mode |
| Q / Escape / Ctrl-C | Exit and restore terminal state |
| P / F | Record visual pass/fail only for the current group's specimens/actions actually emitted this session |
| E | Export JSON to `--report PATH`, or `terminal-showcase-report.json` |
| `[` / `]` | Previous/next overflowing specimen/evidence page; works while paused |
| G, graphics demo | Explicitly cycle Kitty → Sixel → iTerm image experiments |
| X, input demo | Toggle discovered cell / pixel SGR reporting; unknown/unavailable formats remain disabled |
| R, control demo | Re-run sequential objective cursor-response exercises |
| C, integration demo | Explicitly replace clipboard with the static showcase marker |
| N, integration demo | Explicitly request a notification |
| T, integration demo | Explicitly set a demonstration title (title stack push/pop) |
| W, integration demo | Explicitly report the demonstration working-directory hint |

Paste payloads have a distinct parser event type and never enter hotkey decoding, even when bounded queues drop opening/closing boundary events. One persistent parser survives probe/exercise/interactive transitions; escape-looking paste contents stay opaque. Key release/repeat events are shown but do not double-trigger toggles; Caps Lock/Num Lock do not disable hotkeys. Classic CSI and SS3 arrows, Kitty enhanced input and modifyOtherKeys keys share the independent input decoder, including allocation-free normalization of traditional control bytes.

Malformed or expired reply frames quarantine their remaining payload through the frame terminator, so response text cannot become consent hotkeys. Ctrl-C remains an emergency exit outside paste/opaque legacy mouse payloads; Q/Escape inside a broken unterminated reply may be quarantined until its terminator.

Decoded tmux transport fragments share persistent semantic framing, including partial replies and bracketed-paste provenance across wrappers and ordinary reads. Bare paste never interprets wrapper-looking bytes as transport and has no doubled-ESC quoting: only an actual closing delimiter ends it, including when the payload ends in ESC. UTF-8 continuation bytes cannot masquerade as C1 delimiters. Unknown privacy/intermediate controls are not keyboard receipts; malformed SS3 parameters are discarded through their final byte rather than replayed as commands.

Demonstrations cycle every twelve seconds. `overview` combines color, Unicode and type; `colors` emits ANSI16, all indexed palette cells, RGB foreground/background and defaults; `unicode` shows aligned glyph rails and expected cell/grapheme counts; `typography` emits actual attributes, underline styles/colors and selective resets; `graphics` combines always-available braille art with bounded raster lifecycle experiments; `control` exercises raw canvas operations and separate objective CPR transactions; `input` logs key/mouse/focus/paste events; `integration` presents query evidence and real OSC8 links, with disruptive actions only on the explicit keys above.

## Detection semantics

| Status | Evidence |
|---|---|
| Verified | A meaningful objective transaction or actual decoded input event succeeded. Evidence states exactly what was tested. |
| Advertised | Valid correlated claims, recognized modes, capability/color reports or graphics acknowledgment; correct rendering is not inferred. |
| Visual | An actual visual specimen/action was emitted or a user marked a visual pass; glyph/image correctness remains inspection-dependent. |
| Unsupported | Explicit negative evidence establishes the individual capability; a missing response never suffices. |
| Unknown | Insufficient evidence, missing/late response, ambiguous negative or unattempted operation. |
| Failed | Objective coordinate/protocol failure, or explicit user visual failure; `objective_status` and `visual` disambiguate these. |

Queries include DA1/DA2, XTVERSION, XTGETTCAP Co/RGB/TN, OSC10/11/4, Kitty graphics query/keyboard flags, safe modifyOtherKeys inquiry, DECRQM and cursor-style DECRQSS. Scroll margins are queried separately before drawing. A read-only Kitty query uses a nonplacing/nonstoring pixel and a dedicated query ID. Sixel is advertised only when DA1 includes its attribute. No universally reliable iTerm image detection is assumed; G explicitly opts into its visual experiment.

Responses must have valid syntax, matching parameters/IDs and a live pending query. Reply order need not match query order. The default discovery deadline is 1000 ms; late replies remain diagnostic events, never upgrade timed-out requests. SGR mouse status depends on mode 1006, not legacy-only tracking recognition. Cursor visibility (mode 25) is separate from cursor movement. Query-only and unexercised input/resize features remain Unknown until actual evidence exists. Environment identity is not support proof.

An SGR mouse event verifies the reporting format, not its coordinate units. Pixel-mode recognition remains Advertised; the inspector persistently identifies requested units and requires visual confirmation that reported coordinates actually use pixels.

CPR exercises are single-flight: absolute/relative movement, save/restore, CJK width, multiple combining marks, joined emoji, wide-cell overwrite cursor arithmetic, origin coordinates and autowrap. A timeout stops the suite because an untagged late CPR cannot safely be assigned to a subsequent request. Observed geometry changes or SIGWINCH notifications invalidate the suite as Unknown, including resize round trips back to the original size. R is blocked whenever an issued response remains outstanding, including after invalidation; a late valid CPR is discarded before another suite is permitted. Cursor coordinates do not prove erase, shaping or cell contents. Expected glyph widths use independent grapheme-aware Unicode width; font shaping and ambiguous-width policy can disagree and are deliberately visible.

## Screenshot mode

`--screenshot` freezes frame 42 and page 0, disables cycling, hides live controls/debug information and does not emit blinking typography. The matrix, terminal identity and viewport remain visible. S toggles the same mode interactively. Protocol-derived status is endpoint-dependent; reproducibility means identical endpoint evidence, viewport, demonstration and selected options produce identical frame bytes, not that every terminal gets an identical report.

For deterministic offline ANSI fixtures:

```sh
terminal-showcase --headless --screenshot --demo colors --columns 120 --rows 36 > /tmp/colors.ansi
terminal-showcase --headless --screenshot --demo unicode --columns 80 --rows 24 > /tmp/unicode.ansi
```

Offline identity is explicitly `synthetic endpoint (not live detection)`. Fixture rendering contains no live image/protocol operations. Actual live graphics screenshots use a stable populated placement phase; appearance still depends on endpoint graphics implementation and fonts.

## Graphics lifetime and safety

Kitty uses direct RGB payloads, chunks no larger than 4096 base64 bytes, and only owned image IDs 77332/77333. Lifecycle phases show multiple images, replacement, placement changes, crop/clipping, surrounding-text overwrite, bounded scrolling and deletion. Resize rebuilds geometry and clears owned resources. Sixel uses a bounded procedural palette; iTerm uses an actual independently encoded PNG with stored DEFLATE, Adler-32 and CRC-32. Raster dimensions are capped at 64×32; repeated unchanged phases emit no image bytes. Parent scheduling limits phase changes to at most one per second.

Sixel/iTerm lack addressable deletion: their clear/replace operation is a visible text-erase experiment and alternate-screen exit, **not a guarantee of endpoint raster storage reclamation**. No image storage/rendering verification follows from acknowledgment. Legacy image experiments require G even when Sixel is advertised; each opt-in permits at most sixteen raster updates, then freezes rather than accumulating resources indefinitely. Unknown Kitty support also requires G.

Visual image eligibility follows actual raster transmission, not a nonempty text/placement/deletion frame. The colors demonstration exposes the emitted default-color reset for P/F independently of OSC palette-query evidence.

Terminal RAII restores termios/echo, cursor visibility, queried cursor style/modes/margins, alternate-screen ownership, mouse/focus/paste modes, keyboard stack, modifier reporting and synchronized output. Owned Kitty IDs are deleted even on recoverable output errors. SIGINT/SIGTERM/SIGHUP/SIGQUIT/SIGPIPE/SIGTSTP request an orderly exit; SIGTSTP exits rather than leaving a suspended raw terminal. SIGKILL, power loss and a permanently blocked output transport cannot be recovered. Unqueryable modes use normal shell defaults. A preexisting alternate screen is not forcibly exited; its existing content is not snapshotted/restored. Explicit clipboard/cwd actions are intentionally not undone, and title restoration depends on the endpoint honoring the title stack.

SIGWINCH is tracked without requesting exit; the previous signal disposition is restored with the other owned signal handlers.

Mouse tracking is enabled only with a discovered settable SGR format; displaced tracking (9/1001/1000/1002/1003) and encoding (1005/1006/1015/1016) groups are queried and restored by clearing first, then enabling saved selections last. Defensive legacy byte/UTF-8 framing prevents coordinate bytes becoming consent keys; ambiguous legacy high-byte pairs are conservatively consumed rather than dispatched. Legacy receipt is not proof of SGR support.

## CLI reference

| Option | Meaning |
|---|---|
| `--demo NAME` | overview, colors, unicode, typography, graphics, control, input, integration |
| `--screenshot` | Fixed screenshot frame, no animation/cycling/debug controls |
| `--probe` | Live queries and objective exercises, export/display then exit |
| `--check` | With probe, exit 1 on objective Failed; Unknown is not failure or support proof |
| `--json` | JSON output in probe/headless modes |
| `--report PATH` | Live report on E and normal/signal exit |
| `--headless` | Synthetic deterministic regression mode; no TTY required |
| `--responses PATH` | Headless binary response fixture, maximum 1 MiB |
| `--list` | List demonstrations without TTY setup |
| `--fps 1..60` | Animation rate, default 24; lower for SSH |
| `--duration SECONDS` | Timed live exit, positive finite value up to 86400 |
| `--columns / --rows` | Headless viewport, defaults 120×36; bounded 20..500 and 10..200 |

## Automated regression

```sh
cargo test -p terminal-showcase
cargo run -p terminal-showcase -- --headless --json
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test -p terminal-showcase --test multiplexer -- --ignored --nocapture
```

The live multiplexer test is explicitly ignored in the default package suite because it requires a separately built amux binary and Zsh. `AMUX_BINARY` can select another installed build. It controls the daemon only through the public CLI, launches the showcase as an ordinary shell application, checks independent expectations/report classification/strict exit, runs screenshot mode, and confirms the shell remains usable afterward. It never uses amux's parser/model/screen as the oracle.

The PTY regression checks actual process interaction, paste-safe controls, resize, signal cleanup, local echo/input restoration and JSON visual verdict export. Headless checks and Rust regressions cover encoding, fragmented/C1/nested framing, boundaries, malformed sequences, query correlation/deadlines, conservative classification, grapheme widths, deterministic ANSI specimens, redraw/resize and bounded image lifecycle. Raster/glyph appearance requires P/F visual inspection, not a passing unit test.

## JSON format

Live output is `{ "mode": "live", "report": {...}, "exercises": [...] }`. Headless output has `mode: headless`, the same report, `checks`, decoded diagnostic `events`, and `queries_hex` for protocol generation. `report.schema_version` is 1; `identity`, `columns`, `rows` describe the endpoint/viewport. Every capability has a stable `id`, human `name`, `category`, effective `status`, independent `objective_status`, bounded sanitized `evidence[]`, and `visual` (`null`, `true`, `false`). No user verdict is silently upgraded to objective Verified. Objective failure survives a later visual pass; a visual failure affects displayed status while preserving objective evidence.

Each exercise exports `name`, `capability`, `expected: [row,column]`, `actual: [row,column] | null`, and `outcome` (passed/failed/timeout/invalidated). Invalidated means observed geometry/resize notification changed during a transaction; it supplies no compatibility verdict. An internal, nonserialized pending-response flag maintains single-flight safety independently of that outcome. JSON survives terminal cleanup and is written after the guard restores input state. E exports an immediate snapshot during the session.

## Known limitations and observed incompatibilities

The [independent amux run](bugs/BUG-showcase-endpoint-compatibility.md) found joined-emoji cursor advancement and origin-mode CPR defects. They remain Failed and make strict probe exit nonzero; application tests passing does not certify all endpoint features.

Text/Unicode/attribute and integration correctness may need font/emulator policy inspection. Advanced raw canvases require at least 72×18; the compact dashboard remains useful at 40×12, while smaller windows cannot show every category at once. Presentation is bounded to 500×200; reports and objective cursor tests retain physical viewport dimensions. Queries, OSC52, notifications, image lifetime, keyboard and pixel mouse support vary by endpoint policy. Kitty acknowledgments do not prove image display; traditional CSI F3/CPR ambiguity is resolved conservatively in favor of a reply. Full visible-row updates (not cell-level deltas) trade bandwidth for predictable, independently controlled output. macOS arm64 was exercised; no blanket Ghostty/Kitty/WezTerm/Linux/SSH compatibility certification is claimed.
