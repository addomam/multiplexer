# Independently exposed amux terminal endpoint incompatibilities

**Status:** open; detected by e02 standalone explorer, not modified by this enhancement.
**Scope:** amux's observable terminal endpoint. The explorer imports no amux parser/model/renderer.

## Observed evidence
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
Build the workspace and run the explicit multiplexer integration command above. Alternatively execute `terminal-showcase --probe --json --check --report /tmp/amux-capabilities.json` in an attached amux shell. The tool never substitutes the observed endpoint coordinates as its oracle.

## Impact and next action
Cursor-dependent applications can misposition after joined emoji or use incorrect origin-relative DSR coordinates. Investigate the existing emulator/runtime boundary in a separate bug-fix story; preserve these independent regressions. No claim of fixing amux or of compatibility PASS is made here.

## Fresh-session pickup

These are two separate open bugs, not unfinished showcase implementation. Both are registered in [registry.yaml](registry.yaml). Root causes have not been investigated; do not treat suspected parser/runtime boundaries as confirmed causes.

1. Start with `survey-context`, then `investigate-bug`. Read CONVENTIONS.md, specs/state.yaml, this report, and terminal-showcase/tests/multiplexer.rs.
2. Preserve the showcase commit. Create a separate bug-fix branch before changing amux; do not merge or push as part of investigation without a request.
3. Pick up `showcase-origin-cpr` first, then `showcase-zwj-cursor`. This is a suggested sequence, not evidence that either root cause is simpler.
4. Use the existing private-daemon integration fixture. Never stop or replace the user's live/default daemon to reproduce these bugs.

```sh
cargo build --workspace
cargo test -p terminal-showcase --test multiplexer -- --ignored --nocapture
```

The integration test passes when report classification and strict exit behavior match the independent oracle. A green test alone does not mean these endpoint defects are fixed; inspect its printed failures and actual cursor replies.

### showcase-origin-cpr

With scrolling margins 3..10, origin mode enabled and CUP 2;4, CPR must report the origin-relative coordinate in the evidence table. Keep origin-disabled reporting, margin restoration and subsequent shell interaction correct.

Close only when a real amux endpoint reports the expected value, the explorer records objective Verified, and a consumer-visible regression covers the transition between origin-enabled and origin-disabled modes.

### showcase-zwj-cursor

The joined woman-technologist grapheme followed by X must advance according to the independent expectation in the evidence table. Preserve ordinary CJK/combining-mark cursor behavior and wide-cell overwrite behavior.

Close only when a real amux endpoint reports the expected value, the explorer records objective Verified, and a regression catches the incorrect joined-emoji advancement. Font/glyph appearance remains a separate visual check.

### Verification and closure

- Keep the explorer's expected coordinates unchanged. Fix amux behavior rather than accepting its current replies, suppressing failures or sharing its terminal model as the oracle.
- One fix can leave strict probing nonzero while the other bug remains. Require no objective failures before claiming the combined compatibility smoke passes.
- Run workspace tests, clippy with warnings denied, build, and the explicit integration command. Record fresh output and update each registry entry only after its own acceptance criteria pass.
- Preserve historical failing evidence. Do not rewrite it to imply the original showcase run found no defects.
