# e03s01–02 Build evidence

Date: 2026-10-21. Implementation: `/Users/addomam/projects/amux-e03-build`, branch `feat/e03-agent-interaction`, substrate base `6318845`. Explicit user Plan-to-Build authorization; approved Plan reused without reopening requirements. Parent is integration owner. No commits, merge, push, release, live-model runs or global daemon/config/auth changes.

## Isolation and baseline

The original checkout retained its source and user scratch files. Saved planning/specification files were copied into the feature worktree. Initial workspace tests passed 210 tests, 5 ignored; Clippy with warnings denied, workspace build and all-target check passed. Isolated `cargo run -- --help` passed. Original-checkout state/ledger points to the build worktree; historical review records/caps were preserved.

## Implemented and exercised

- `src/control.rs`: opaque kind/lifetime-bound controller/control/claim/offer capabilities; per-PTY dispatch/revocation ordering; request/reservation/yield/settlement/explicit accept; ordinary cancel grants nobody and invalidates old control; fresh prior-owner resumed acquisition; public compare-and-force; private current/historical recovery and three-second blocker progress.
- `src/runtime.rs` and compatibility replies: authority retained through queue consumption; revoked undispatched input is dropped without stopping protocol progress. One native unknown batch per generation; write acknowledgements never settle application input.
- `src/integration.rs`, Zsh adapter and daemon: source authority survives epoch/wake/reports; shell dispatch is registered before the atomic FIFO write, native acceptance/rejection settles that exact call, timeout retains unknown accounting/identity. Delayed revoked source is rejected, not replayed. Private automatic launch authority retires only when dispatch is settled or definitively rejected/undispatched.
- CLI/TUI/test consumers: required serialized `--control` and exact terminal lifetime IDs; concrete wire error type; one original-connection native attachment, private cached controls, keys/literal-prefix/paste/application mouse gated; view focus is daemon-classified protocol progress, not human activity. Native accepted activity is recorded only after successful enqueue and only with original connection provenance. CLI fixtures explicitly acquire; automatic-launch readiness observes actual authority retirement, not retries/force.
- Stock Zsh/OMP and independent showcase boundaries retained. Full ownership UX/conditional policy and concrete Codex tracks are not yet delivered by these slices.

## Observed verification

| Command/scenario | Observed result |
|---|---|
| `cargo test --lib` | 77 passed, including deterministic queued-byte revocation/protocol-progress proof |
| `zsh -n integrations/zsh/adapter.zsh` | exit0 |
| `cargo test --test acceptance --test shell_races --test integration --test readiness --test runtime` | 40 passed, 2 ignored |
| `cargo test --test pty_control -- --nocapture` | 13 passed after one incorrect test expectation was corrected to prove successful waiting-settlement is not a grant |
| `cargo test --workspace` | 241 passed across 27 suites, 5 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit0 |
| `cargo build --workspace` | exit0 |
| `cargo check --workspace --all-targets` | exit0; also passed after `cargo fmt --all` |
| `cargo test -p terminal-showcase --test multiplexer independent_showcase_runs_inside_amux_and_checks_real_cursor_replies -- --ignored --exact --nocapture` | exactly 1 executed test passed |
| Private real CLI/daemon smoke | actual source marker; A→B yield/offer/explicit safe acceptance; stale A input rejected; B private recovery returned the same current control; real native input marker applied while its batch remained explicitly unknown |

The cancellation test originally incorrectly required an unrelated unsettled acquisition to be an error. The contract permits a successful waiting state, not authority. It now asserts waiting-settlement, no owner, unchanged unresolved calls and no marker effect, explicitly cancels that reservation, then verifies the prior owner's fresh resumed token. No runtime safety condition was weakened.

`tests/pty_control.rs` proves real socket/CLI marker effects, reservation contention, ordered cancel/accept winners, force preserving a running process/output, missing/wrong/stale/forged capabilities, public privacy, unread observers, event gaps, lost accept reply recovery, original attachment provenance/departure, retained native unknown batches and a held real shell wake whose stale source cannot execute. The runtime unit test holds the native input queue until release, then exercises the same writer-consumption path and observes only the independent protocol reply.

## Privacy and cleanup

The CLI smoke used private mode0700 HOME/config/runtime/startup/projects roots, no real credential copies and no model. Graceful shutdown completed; daemon and shell PIDs and the owned daemon process group were independently observed absent. Socket and private root removal were asserted. No smoke script or marker was left in the repository. Permanent fixtures own their private daemon roots and bounded cleanup; no user/global daemon was stopped.

## Self-check and acceptance limits

Pre-implementation threat model: `specs/security/epics/e03/THREAT_MODEL.md`. Secret-pattern/security spot-check found no embedded credentials or new ownership path that invokes process stop. Provider/auth surfaces remain for the Codex track. No dependency was added by s01–02. Runtime state stays in memory and slow observers/provider I/O do not hold ownership/topology locks. Public snapshots/events were checked against private reference leakage in the real-daemon suite.

No repository churn helper is bundled; audit priority followed the approved impact map (daemon/runtime/integration/control). Generic audit recommendations for 300-line files/20-line functions and one test per function do not override this repository's approved deep-module boundaries or behavioral-test policy. No independent code-review PASS or historical-cap reset is claimed.

These are passing s01–02 deterministic/real-daemon slice verifications, **not whole-e03/runtime acceptance**. Native controlling-PTY ownership UX proof belongs to s03–04, concrete same-instance/live question/permission/interrupt proof to s05–07, and all 32 scenarios plus the unresolved native-buffer/application fence to complete-broker s08. Unknown native settlement still blocks safe grant; force-only is not final acceptance. Live checks require separate explicit approval.

Post-milestone native-harness attempt: forking a controlling-PTY driver inside the retained Eval Python kernel timed out before the first owner marker. Its cleanup wait also stalled. This is **not a native UI PASS**; no causal application defect is inferred. The private daemon was gracefully shut down from the external CLI; external process inspection confirmed the fixture daemon, attachment PID and shell absent. Parent closed descriptors and removed the private root. Tool anomaly reported; actual s03–04 surface verification will use an external driver with continuous PTY draining and bounded cleanup rather than fork/wait inside Eval.

External-driver resolution (ownership milestone executable, before post-ownership module integration): `python3 /private/tmp/amux-e03-herdr-1tcaq3pa/native_surface_smoke.py /Users/addomam/projects/amux-e03-build/target/debug/amux` exited0. Two actual controlling PTYs rendered amux. Owner keys created the real marker; the second attachment visibly rendered `Observer`; observer key input and separate bracketed paste created no markers and were not replayed when fresh owner input landed. Public state retained two connected views and one unknown native batch. Both native attachments detached with exit0, left the alternate screen and restored canonical/echo modes. Owned daemon was reaped, shell observed absent, socket/private root removed. Captured runtime output: artifact59; retained evidence here excludes private temporary paths and control references. This validates the s01 native admission path, not the still-pending s03–04 modal/conditional UX.
