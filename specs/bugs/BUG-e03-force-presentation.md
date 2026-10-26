---
bug_id: e03-force-presentation
type: bug
context: e03 scoped Build review round4
status: fixed
severity: medium
independent_review: round5-no-findings-overall-gate-failed
review_evidence: specs/verifications/e03-build-review-round4.json
---
# Native force can commit before its warning is visible

Round4 B found a non-waived native requirement violation: Enter submits force from matching captured facts without draw eligibility. The event loop drains queued shortcut/Enter events before drawing; an80x7 viewport cannot draw the modal at all. A compares-only daemon cannot establish native warning presentation. General A/security PASS do not override B's must-fix. Round4 remains FAIL; only round5 of the existing max5 remains.

## Correction plan

Bind native force to the captured target/observation and a complete visible warning draw. Reuse the existing successful-frame, drained-input after_draw boundary, not a timer or queued action. A private hidden/drawn/confirmable state distinguishes widget construction from safe confirmation. Consume pre-draw Enter with no submission/replay. Clear eligibility on changed warning facts, resize/scroll and whenever the full warning cannot fit, including clipped as well as undrawable viewports; preserve it across an unchanged still-fully-visible repaint. Preserve the existing stale-observation guard and exact captured owner/reservation; no cancellation/force semantics or process behavior changes.

Use the pinned existing Ratatui Paragraph rendered-line measurement through its opt-in feature if needed; do not replace/update terminal libraries, add a text-layout dependency or reimplement wrapping. It measures the same content and interior width used to render. Force stays unavailable until its complete warning fits and the native input queue has drained after a successful frame.

## Verification

Extend the existing actual controlling-PTY cancel/stale-force regression with queued pre-draw confirmation and undrawable/clipped viewport boundaries. Drive real encoded keys, inspect rendered captures plus actual daemon ownership and unchanged application bytes/process. Resize fixture capture dimensions with the actual viewport; no mock UI or fixed sleep as proof of presentation. Observe failure before source correction, retain test-only RED checkpoint, then focused/full approved checks and separate actual TUI smoke, all owned resources cleaned. Fresh dual Build/security round5 is the final capped review; do not reset prior rounds or claim full Codex acceptance.

## Runtime result

RED23a7016/artifact558: real queued shortcut/Enter changed owner before warning,0.98s. Corrected existing native regression PASS1/1.81s, then all34 ownership cases/8.77s and full307 Rust tests/10ignored; strict Clippy/check/build/fmt, exact workspace-package showcase, OMP8/49, stock-Zsh syntax and actual CLI help pass. Native captures use actual dimensions. Separate continuously drained controlling-PTY smoke proves blocked pre-draw/80x7/clipped80x10, old human control retained, fresh visible takeover, same live shell and FRESH-only application bytes; detach0/shutdown0 in0.244s, owned processes/root/fds/reader gone without fallback. All initial failed attempts remain disclosed, including one callback timeout whose cause is not established. See specs/verifications/e03-force-presentation-runtime.json. Runtime verified; independent final round5 remains required.

## e04 post-suspend regression repair

The e04s01 regression run exposed an unchanged-repaint race: a complete warning was followed by a no-cell-change frame, then a legitimate fresh Enter was rejected because repaint demoted its completed eligibility. Preserve eligibility only for unchanged, still-fully-visible warning facts; first draw, changed facts, resize, scrolling, clipping and stale observation still invalidate. Recorded ANSI analysis is in `e04s01-force-repaint-diagnosis.json`. The original actual controlling-PTY case passed after repair; full workspace335 and actual ACP CLI/PTY force takeover passed (`artifact://1036`, `e04s01-resume-cli-pty-smoke.json`). Historical e03 review counters/acceptance are unchanged; new e04 code review remains separate.

## e04 round2 repair verification — resize observation

`artifact://1069`: the first round2 repair run passed9 ACP library and21 s01 cases, then the native target passed33/34 and timed out waiting for restored physical size after the captured80x7 warning. This is not a force grant or a passing regression run. Retain its exact failure/captures; no established cause is claimed.

The resize helper now preserves the same exact dimension assertion and12s deadline while recording requested/observed geometry and attachment exit state on failure. The instrumented original case passed1/1 (`artifact://1074`); the subsequent complete workspace passed340/30suites/10ignored with all34 native cases (`artifact://1080`). Native busy-rejection status retains its previous behavior; the new busy notice is ACP-only. These results do not prove a cause for the earlier resize timeout. The intermittent observation remains logged, and e03 complete native acceptance remains failing.

