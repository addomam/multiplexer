# e02 terminal-showcase verification

type: verification
context: native-terminal

## Deliverable and boundary

Standalone `terminal-showcase` Cargo package, independent of amux implementation/model/parser/renderer. Endpoint bytes use stdin/stdout; POSIX TTY management owns input modes, signals and winsize.

Existing Cargo defaults and historical e01 evidence are preserved. The user subsequently requested the local showcase commit on `feat/terminal-showcase`; merge, push and release are not requested.

Usage, controls, detection semantics, graphics lifetime, screenshot fixtures, JSON and limitations: [terminal-showcase.md](../terminal-showcase.md).

## Gates

| Gate | Observed result | Evidence |
|---|---|---|
| Workspace regression | 177 passed; four intentionally ignored | e02-gates.log |
| Workspace clippy / build / rustfmt | Passed, warnings denied | e02-gates.log |
| Headless regression | Six checks passed; deterministic fixtures and JSON | e02-gates.log |
| Ordinary amux-shell integration | Passed; two endpoint coordinate defects remain honestly Failed | [BUG report](../bugs/BUG-showcase-endpoint-compatibility.md); e02-gates.log |
| Native interactive surface | 21 recorded stages; actual Chromium/xterm.js consuming native PTY bytes | showcase-browser-screen.json |
| Independent review | Fifth blind pair PASS; both 100%, no findings | REVIEW-e02.md; ../reviews/e02-round-5-a.md and e02-round-5-b.md |

One contiguous post-review gate command, exit 0, artifact 186, wall time 72.95 seconds:

```sh
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=commit.gpgsign GIT_CONFIG_VALUE_0=false cargo test --workspace &&
cargo clippy --workspace --all-targets -- -D warnings &&
cargo build --workspace &&
cargo test -p terminal-showcase --test multiplexer -- --ignored --nocapture &&
cargo run -p terminal-showcase -- --headless --json &&
cargo fmt --all -- --check
```

The command-local signing override isolates an existing amux fixture from unavailable 1Password signing; repository/global configuration was not changed. Four ignored entries are the two isolated controlling-PTY fixtures, the explicitly invoked optional amux integration, and an existing amux fixture.

## Named completion criteria

| # | Criterion | Observable proof |
|---|---|---|
| 1 | Existing toolchain builds/runs | Rust 1.95 macOS arm64 workspace build; actual binary launches |
| 2 | Standalone terminal application | Native PTY→ordinary stdin/stdout→real browser terminal; no internal imports |
| 3 | Polished dashboard | Actual 120×36 overview/typography, 80×24 and 40×12 surfaces; all seven categories |
| 4 | Accurate evidence states | Conservative query/negative/deadline/precedence regressions; emitted-only visual eligibility |
| 5 | Active feature demonstrations | Eight demos; actual SGR/Unicode/raw control/input/integration and bounded procedural protocol images |
| 6 | Cycling/manual navigation | Native arrows, pause/resume and twelve-second automatic transition observed |
| 7 | Deterministic screenshot | Fixed frame/page, hidden live controls/blink; deterministic ANSI regression and observed first-start overview |
| 8 | Correct restoration | Native q/Ctrl-C/signals, echo/termios, normal buffer, inherited DRAG/SGR_PIXELS, signal-disposition regressions |
| 9 | Resize safety | Actual viewport changes, capped presentation versus physical dimensions, CPR invalidation and transient SIGWINCH round-trip regression |
| 10 | Headless checks | Six executable checks plus synthetic fragmented/malformed/correlated/bounded protocol and rendering regressions |
| 11 | JSON reporting | Headless JSON, live --report/E, independent visual verdicts and strict objective failure |
| 12 | Runs inside amux | Public CLI/private fixture shell launches ordinary showcase probe and screenshot; shell usable afterward |
| 13 | Exposes real compatibility bugs | ZWJ expected [7,4], actual [7,6]; origin expected [2,4], actual [4,4]; strict probe exits 1 |
| 14 | Applicable workflow/quality gates | Scope/capsule preserved; native gates/audit/security/F.I.R.S.T and final dual review passed; all four stories verified |

## Runtime and regression evidence

The native browser harness intercepts OSC52, never writes the host clipboard. Paste/mouse/malformed/nested reply bytes emitted no consent action; only the explicit C key emitted the authored marker in its dedicated stage. Final Unicode continuation, unclosed literal wrapper-looking header and trailing-ESC pastes remained opaque until the genuine closing delimiter, then E/q worked and termios/normal buffer returned.

Typography's combined SGR underline colors became eligible for an authored F/E export (`visual: false`, effective Failed). This exercises the verdict mechanism; it is not a newly asserted appearance defect. Palette/query evidence remains independent. Parser regression corpora include read splits, bytewise feeds, idle boundaries, C1-looking UTF-8 continuations through nested wrappers, malformed SS3 and bounded queue loss.

The disposable browser context once showed an incomplete capture; a fresh page rendered and passed the final scenarios. A native process profile showed idle poll, and a direct ordinary-PTY first-frame smoke emitted the dashboard. No speculative application fix or precise unproven harness cause was claimed.

## Performance evidence

Five ordinary debug-binary launches per viewport, fixed headless colors frame; elapsed includes process launch, rendering and captured ANSI output, not live discovery/graphics/transport or animation FPS. Details: NFR-e02.json.

| Viewport | Mean launch/render ms | Maximum ms | ANSI bytes |
|---|---:|---:|---:|
| 40×12 | 5.17 | 9.94 | 2644 |
| 80×24 | 6.97 | 7.16 | 15758 |
| 120×36 | 8.15 | 8.26 | 20445 |

A one-second native idle sample reported footprint/peak 1968K, blocked in poll with zero on-CPU samples; this is a snapshot, not a memory-leak/CPU-load certification. Changed-row updates, no frozen idle delta, phase deduplication, bounded buffers/canvases/rasters and configurable 1–60 FPS provide the verified resource boundaries.

## Limits and workflow provenance

Physical font/raster appearance still requires P/F inspection. Ghostty/Kitty/WezTerm graphics, Linux/SSH and Rust minimum-toolchain combinations were not certified. Browser relay query latency can leave CPR queries Unknown; no missing reply was normalized into Unsupported or PASS. Sixel/iTerm experiments are explicit and finite, with no addressable deletion/storage-reclamation guarantee.

Repository BigPowers helper scripts are absent; native commands and scoped manual evidence substitute. No scripted consistency/churn/compliance/coverage execution, coverage percentage, mutation score, RED/GREEN commits or human signoff is fabricated. [AUDIT-e02.md](AUDIT-e02.md), [e02-test-quality.md](e02-test-quality.md), [security review](../security/REVIEW-e02.md) and preserved independent review records document accepted fixes and limits. Historical e01's exhausted review gate is not waived by this enhancement.

## Completion bookkeeping
All four stories passed verification and independent review. Native validation parsed thirteen operational YAML artifacts; their story/task/execution/verification records agree. The JSON runtime record contains twenty-one stages.

The local commit includes the [fresh-session bug handoff](../bugs/BUG-showcase-endpoint-compatibility.md). Next skill: `investigate-bug`; no compatibility fixes are included.

The owned browser tab and disposable PTY bridge were closed, and their temporary script/report/reproduction/profile files removed after proof. Built binaries and permanent behavioral/evidence artifacts remain. The user's unrelated root `script` was untouched.
