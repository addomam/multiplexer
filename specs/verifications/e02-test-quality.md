# e02 behavioral coverage and F.I.R.S.T review

type: verification
context: terminal-capability-explorer

This is a retrospective implementation/evidence map, not a claim that a separate test architecture document existed before implementation. The original PLAN.md defined the boundaries and runnable gates. Installed project test-planning/coverage scripts are unavailable; no line-coverage percentage or mutation score is fabricated.

| Requirement / risk | Automated consumer-visible boundary | Actual runtime / visual limit |
|---|---|---|
| Independent architecture / P0 oracle independence | Workspace/package boundary; separate protocol and Unicode expectations; ordinary amux-shell strict probe and screenshot run | Actual public CLI shell remains usable; no internal parser/model oracle |
| Framing/correlation / P0 false support or consent | Fragmentation, C1/UTF-8, multiple/out-of-order replies, exact IDs, malformed/overflow/deadline quarantine; persistent paste provenance; deferred overflow at three phases | Real PTY reproductions; nested transport, opaque bare paste, partial wrapped replies and SS3 consumer regressions |
| Capability states / P0 false evidence | Unknown on silence, explicit individual negatives, objective/visual precedence, mode-family separation, pixel-unit conservative evidence | Actual P/F/E exports; native glyph/image correctness still inspection-dependent |
| State management / P0 damaged TTY | Stateful mutually exclusive mode oracle, physical viewport, keyboard/mouse gating, termios, actual signals/resize/input/child cleanup | Browser inherited DRAG/SGR_PIXELS restored; normal buffer/echo restored after q/Ctrl-C |
| Dashboard/screenshots / P0 visible correctness | Grapheme/cell clipping, bounds, deterministic fixed frame, no idle delta, compact categories, actual text/style encoding | 120x36,80x24,40x12 surface observed; pause/navigation/cycle/details/diagnostics/screenshot controls exercised |
| Raster/input/integration / P0 unsafe operations | Independently decoded PNG/CRC/payload bounds, owned IDs/lifecycle, typed keyboard/modifiers/mouse/focus/paste and explicit integration commands | Native Ctrl-A fields and pointer inspector; intercepted OSC52 only after C; actual emulator rasters not certified |
| Resize during objective checks / P0 false failure | Independent physical PTY geometry invalidation, issued/consumed metadata, oversized/undersized viewport, resize-notification round trip regression | Startup/manual suite uses untagged CPR; outstanding responses remain single-flight after invalidation |
| Default-color / raster visual verdict / P0 misleading diagnostics | Actual colors-PTY query→emitted reset→F/E export; phase/protocol raster-transmission metadata, clear/no-op/resize bounds | Palette query stays independent of visual verdict; actual raster appearance still inspection-dependent |
| CLI/headless / P1 regression usability | Argument/error bounds, deterministic fixtures, JSON and strict probe semantics; six executable headless checks | Actual ordinary program launches; live clean JSON via --report, not redirected protocol stdout |

## F.I.R.S.T
- Fast: framing/render/encoding tests are in-memory; real PTY/daemon cases have bounded deadlines rather than indefinite waits. Full workspace run is measured separately from test CPU summaries.
- Isolated: fixtures own child processes and scoped temporary paths. No physical clipboard changes are needed; the actual browser intercepted the authored marker. No auth/service/network dependency in the product suite; the browser bridge is a disposable visual-verification aid.
- Repeatable: fixed frame/page/evidence, explicit synthetic endpoint identity, independent coordinate expectations and bounded protocol payloads. Host signing is disabled only for the verification command because an existing amux fixture invokes git commit.
- Self-validating: protocol/geometry/status/resource/error assertions, not screenshots labeled automatically successful. Visual correctness is deliberately not inferred from acknowledgment or nonempty output.
- Timely: reproduced-before/passing-after tests retained for batching, framing, queue overflow, late replies and transient geometry/control misclassification. No RED/GREEN commits were requested/performed. Final acceptance requires fresh independent review; isolated subprocess fixtures are explicitly ignored, not counted as bare no-op passes.

Catalog-copy, string-wording, source-text and mock-forwarding tests were removed. New tests cover plausible consumer bugs, boundaries, transitions, precedence or errors. Generic per-function/line-count heuristics do not justify adding tautological tests or splitting cohesive protocol/demo tables into indirection.
