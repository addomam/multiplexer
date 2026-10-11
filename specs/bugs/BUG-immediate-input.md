---
type: bug-fix-plan
context: e01 native terminal; second input-latency pass after0d42de5
---
# Immediate native input

## Reproduce
User requests further improvement beyond measured10.81/17.49/30.34ms medians at120/300/500-column viewports. Previous actual-PTY measurements are accepted, not rerun to confirm them. New diagnosis uses the current release TUI in an isolated500×163 controlling PTY with continuously drained output and native Zsh keys.

## Isolate / hypothesize
1. Native rendering repeatedly indexes every retained JSON cell/style, despite receiving only damaged rows. Falsification: sample the actual foreground TUI during individual-key echo; most active work should be in view projection/JSON lookup, not IPC or PTY waits.
2. Eight-millisecond periodic presentation adds avoidable delay after raw input. Falsification: sample must show frame polling significant alongside projection; shortening only active-input scheduling should not create constant high-rate idle work.

## Verify root cause
macOS `sample <actual-tui-pid> 3 1` captured2115 main-thread samples:1332 under Ratatui try_draw's projection path, predominantly view::draw→serde_json string-index/BTreeMap comparisons;111 under diff,573 under poll/kevent,82 under refresh_screen. This establishes repeated retained-cell decoding plus periodic polling as remaining costs. No fake echo, adapter/readiness suppression, screen scraping or physical-monitor claim.

## Design / implementation
- Client-only native styled Buffer cache. Decode only validated changed rows; full frames rebuild the cache. Retained screen JSON stays authoritative for text selection/input modes. Normal drawing copies bounded prepared Cell rows instead of re-decoding all JSON styles. Preserve clipping of wide glyphs, combining text, selection overlays and cursor/focus behavior.
- Validate deltas before updating either native cache or retained JSON; rejected stale/out-of-viewport frames leave both unchanged. Same full/delta contract, no daemon/API/runtime changes.
- Request an immediate presentation opportunity after native input; poll at1ms briefly after interactions and return to8ms idle pacing. Drain already-queued input ahead of rendering with a bounded event count, so burst typing does not render once per key or starve presentation. No hidden input replay or local synthetic echo.
- Small scope: src/tui.rs and src/tui/view.rs; one renderer cache, not a new framework/module. Update the existing merge contract tests for cache invariants; real UI smoke for wide/combining glyphs, mode changes, selection, picker/exit actions, resize and partial-input authority.

## Acceptance / validation
Measure actual TUI PTY key→emitted native render with continuously drained output,100 keys per size/source including separate noisy output. Record p50/p95/max; aim for low-single-digit milliseconds, report measured limits rather than an instant guarantee. Full suite, Clippy/release/Zsh/OMP checks, native wrapper smoke and owned-resource cleanup. Full-frame JSON/terminal diff/OS/compositor costs remain possible limits. Existing independent review cap is not reset.

## Observed outcome
Fixed in the client-only cache/pacing slice. Actual native Zsh medians are 2.01 / 3.94 / 6.77 ms at 120×40 / 300×103 / 500×163; 1,000 single-key samples and the 3.98 ms 24-byte burst are recorded in [verification](../verifications/immediate-input.md). Cold full-refresh maxima around 130 ms and the 42.22 ms idle outlier remain visible. Existing 66 Rust / 8 OMP tests, Clippy/release/Zsh and actual native/Herdr UI smoke pass; first-pass daemon processes survive TUI replacement. No independent review-cap reset or physical-paint guarantee.
