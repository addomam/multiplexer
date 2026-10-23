# Independently exposed amux terminal endpoint incompatibilities

**Status:** both fixed and verified on `fix/substrate-defects`; original e02 failures preserved below. Fresh evidence: [substrate reliability](../verifications/substrate-reliability.md).
**Scope:** amux's observable terminal endpoint. The explorer imports no amux parser/model/renderer.

## Observed evidence — original e02 run
The independent explorer ran as an ordinary Zsh application under a real daemon at 100×30, using only stdin/stdout for all queries. Final recorded output: [workspace preflight](../verifications/e02-gates.log).

| Exercise | Independent expected CPR | Actual CPR | Explorer verdict |
|---|---|---|---|
| ZWJ `👩‍💻X` starting at row 7, column 1 | 7;4 | 7;6 | Failed |
| Origin mode with margins 3..10 and CUP 2;4 | 2;4 | 4;4 | Failed |

The strict `--probe --check` command exported the complete JSON report, restored the terminal, and exited 1. The application-regression test passed because both mismatches remained objectively Failed, with their original expectations and actual replies intact. It does not certify endpoint compatibility.

## Standards / independent expectations
- Origin-mode CPR line numbering follows DECOM: [VT100 user guide, CPR and DECOM](https://vt100.net/docs/vt100-ug/chapter3.html). Relative row 2 is physical row 4 with top margin 3; CPR should report relative row 2, not physical row 4.
- ZWJ expectation uses grapheme-aware unicode-width, independently of amux. Joined woman-technologist occupies two expected cells followed by X; the observed extra two cells expose endpoint grapheme arithmetic. Font appearance remains a separate visual check.

## Reproduction
Build the workspace and run the explicit multiplexer integration command below. Alternatively execute `terminal-showcase --probe --json --check --report /tmp/amux-capabilities.json` in an attached amux shell. The tool never substitutes the observed endpoint coordinates as its oracle.

## Impact and resolution
Cursor-dependent applications could misposition after joined emoji or receive incorrect origin-relative DSR coordinates. The parsed ANSI Handler adapter now preserves native controls while composing graphemes in native cells and correcting DECOM CPR. The independent oracle is unchanged; both endpoint capabilities now verify and strict probing exits 0.

## Fresh verification

The separate bug-fix branch implements both fixes in `src/runtime.rs` and `src/runtime/compatibility.rs`, with direct cell/layout regressions, real-PTY raw CPR coverage, and a strict independent-showcase acceptance. All probes used private daemons; the live/default daemon and Herdr sessions were not changed.

```sh
cargo build --workspace
cargo test -p terminal-showcase --test multiplexer -- --ignored --nocapture
```

The integration now requires every exercise to pass, both endpoint capability statuses to be objective Verified, and strict exit 0. Workspace tests, Clippy, debug/release builds, a separate 13-case wire probe and native TUI detach/reattach pass; physical font/glyph shaping remains unverified.

### showcase-origin-cpr

With scrolling margins 3..10, origin mode enabled and CUP 2;4, CPR must report the origin-relative coordinate in the evidence table. Keep origin-disabled reporting, margin restoration and subsequent shell interaction correct.

Verified closure: the real endpoint matches the unchanged expectation, the explorer records objective Verified, and a consumer-visible regression covers origin-enabled/disabled transitions, reset, margins, resize and alternate screen.

### showcase-zwj-cursor

The joined woman-technologist grapheme followed by X must advance according to the independent expectation in the evidence table. Preserve ordinary CJK/combining-mark cursor behavior and wide-cell overwrite behavior.

Verified closure: the real endpoint matches the unchanged expectation, the explorer records objective Verified, and regressions cover chunked UTF-8/ZWJ, family/flags/modifiers/keycaps, CJK/combining, wide overwrite, right-edge wrapping and variation-width replay. Font/glyph appearance remains a separate visual check.

### Verification and closure

- Keep the explorer's expected coordinates unchanged. Fix amux behavior rather than accepting its current replies, suppressing failures or sharing its terminal model as the oracle.
- One fix can leave strict probing nonzero while the other bug remains. Require no objective failures before claiming the combined compatibility smoke passes.
- Run workspace tests, clippy with warnings denied, build, and the explicit integration command. Record fresh output and update each registry entry only after its own acceptance criteria pass.
- Preserve historical failing evidence. Do not rewrite it to imply the original showcase run found no defects.
