---
type: impact
context: e03 agent interaction planning; prior substrate impact retained below
---
# Impact assessments

Current initiative: e03, in the final section below. The substrate assessment and its executed checks are retained historical evidence, not a new e03 verification run.

## Targets
Terminal ANSI/emulator semantics; configuration serialization; hook process lifetime; daemon admission/diagnostics; client startup readiness; failed integration/PTY setup cleanup.

## Dependents
- LSP: `client::ensure_daemon` has two production callers, default CLI and attach; preserve both.
- LSP: `projects::run_hooks` has one production caller in the daemon and seven existing test callsites; migrate the cancellation argument everywhere.
- LSP: `Store::persist` has four mutation callers; each must retain error rollback without silent stale overwrites.
- LSP: `Config` references are internal to projects; the daemon serializes/reads Store.config. Old disk files need defaulted terminal-limit compatibility.
- `Store::load` has one daemon caller and ten existing test callsites. Read/parse/expected snapshot must derive from the same bytes.
- Terminal changes affect PTY output, screen/delta metadata, native rendering and replies to applications. The independent showcase must not share the emulator oracle.
- Integration follow-up: partial preparation can fail before registration, and PTY descriptor/identity setup can fail after spawning. Removal validates IDs and clears partial resources; uninstalled children retain kill/reap ownership until the normal wait-thread handoff.

## Affected stories
e01s01 runtime/process cleanup; e01s02 native submissions; e01s03 project/config/hooks; e01s05 CLI/TUI; e01s06 acceptance; e02s04 independent compatibility testing.

## Test coverage
Preserve runtime screens/CJK/combining/alternate/sync/palette/wrap, shell races, project persistence/rollback, Git worker limits, noisy/slow-client and concurrent shutdown coverage.

Add behavior regressions for origin/margin transitions and streamed joined clusters; stale config writers and reload; hook cancellation/descendant cleanup; concurrent admission/refund; diagnostic retention; initialized startup.

## Risk: High
These are shared execution paths. Keep existing native interaction and same-user trust boundaries; no orchestrator, restoration, remote backend or generic plugin scope.

## Verification
Baseline workspace tests (177 passing; four ignored), Clippy and workspace build passed before changes. Root-cause probes confirmed a four-second active hook delayed shutdown by 3.994 seconds and an inert listener left baseline attachment blocked beyond five seconds.

`verify: cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace --release`

Executed the independent real-daemon strict probe, native controlling-PTY smoke and private-daemon config/admission/cancellation scenarios. Final workspace verification passed 210 tests, with five ignored fixture/integration entries and the independent multiplexer check run explicitly. Clippy, debug/release builds, 8 OMP tests and Zsh syntax passed. [Exact evidence and retained observations](verifications/substrate-reliability.md) includes the initial queued-execution timeout, whose cause was not established. Preserve existing review-cap history; no new independent PASS is inferred.

## Current initiative — e03 agent interaction

### Target and risk

**Risk: High.** The shared local mutation API, native input routes, delayed shell continuations and PTY writer consumption all change together. The lock is coordination between trusted same-user clients, not a new security sandbox. Plan only: no source/dependency change or runtime probe was performed.

### Language-server fan-in

Parent rust-analyzer references, excluding each definition:

| Symbol | Production reference sites | Test reference sites |
|---|---:|---:|
| `Terminal::input` | 4 | 13 |
| `Terminal::input_confirmed` | 3 | 0 |
| `Terminal::spawn` | 1 | 1 |
| `Integration::execute` | 2 | 7 |

These are reference-site counts, not unique callers. `Terminal::input` includes the runtime paste wrapper and daemon text/bytes/key branches. Confirmed input has immediate-execute and delayed shell-control callers. Spawn has daemon creation and the direct runtime fixture. Integration execute has two daemon calls and seven direct integration-test calls. Refresh references before actual exported-symbol edits.

Two read-only scouts additionally mapped private dispatch, raw queue producers and test fixtures. Their restricted environments lacked LSP; the parent supplied the actual language-server queries above. Their source-search observations are not mislabelled LSP results.

### Module purpose, callers and contracts

- `daemon.rs`: public CLI/persistent native requests converge through serve/dispatch; open startup and shell-epoch/wake also cause input. Carry original connection identity, classify reports, bind launch/queued work, preserve event sequence/gap behavior and never hold global state locks across waits.
- `runtime.rs` / `runtime/compatibility.rs`: daemon and direct transport tests depend on PTY creation/input/paste/resize, emulator replies and idempotent identity-checked process cleanup. Consumption must revalidate queued authority; writer acknowledgement is not application settlement. Automatic query replies remain independent progress.
- `integration.rs` / Zsh adapter: daemon plus direct integration fixtures depend on source FIFO, native readiness and accepted/rejected/completed outcomes. Preserve submitted authority when execution JSON is replaced; delayed wake/accept may not inject revoked work. Old settlement reports still account for prior work.
- `client.rs` / `cli.rs`: native attach and one-shot commands share framing. Preserve structured error codes and buffered persistent responses, raw API null behavior and no automatic uncertain replay. Existing input/execute producers gain explicit control.
- `tui.rs` / `tui/view.rs`: all key, doubled-prefix, paste, app mouse, local activity, focus, selection and fallback branches need one ownership decision. Preserve three-row header/body geometry, selection/copy and independent viewing. Authority loss is recoverable UI state.
- `projects.rs` / OMP reporter: callers retain disk/runtime separation, process/lease/lifecycle distinctions and launch behavior. Do not persist capabilities, use OMP idle as settlement or repurpose disposable hook supervision for a long-lived Codex backend.

### Affected stories and caller migration

All e03 stories touch these shared seams; baseline owners are e01s01/s02/s04/s05/s06 and e02s04. The e03 manifest maps every interaction criterion to a primary slice and final acceptance.

Mandatory migration: CLI Input/Execute and their argument tests; TUI send_input plus separate paste; `tests/acceptance.rs` input/execute producers; `tests/shell_races.rs` Host::execute and native typeahead paths; all three execute calls in `terminal-showcase/tests/multiplexer.rs`. Maintain valid control so prior busy/partial/latency tests still reach their original boundary. No naked-input compatibility shim. Direct runtime/integration fixtures migrate signatures when needed, but remain explicitly lower-level evidence.

### Existing regression anchors and gaps

Preserve `queued_wake_cannot_follow_typeahead_into_foreground_application`, `immediate_submission_cannot_deadlock_a_pending_queued_wake`, `host_context_child_partial_busy_tree_detach_and_shutdown`, `screen_queries_slow_subscriber_and_noisy_terminal_do_not_block_input`, `sequenced_waits_replay_transitions_and_report_stream_gaps`, process cleanup/autostart regressions, and `independent_showcase_runs_inside_amux_and_checks_real_cursor_replies`.

No existing test proves the new lock or structured Codex contract. e03s01 creates `tests/pty_control.rs`; e03s05 creates `tests/agent_interaction.rs` and opt-in `tests/codex_native_smoke.rs`. New tests target uncertain stale-authority, competing-transition, event/lifecycle, timing/view and cleanup behavior, not wiring/source/default assertions. Deterministic protocol fixtures cannot establish real native/harness settlement.

### Recommended action

Proceed with the bounded vertical plan and its independent quality gate, then request Build confirmation. Do not reopen a separate native-buffer experiment: revisit it under e03s08's complete broker if unresolved. An unobservable application boundary must remain unknown and block safe grant/first acceptance, not become a sleep, force-only acceptance or unapproved upstream fork.
