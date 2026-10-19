---
type: impact
context: BUG-substrate-reliability; e01 runtime/project/automation; e02 endpoint acceptance
---
# Known substrate defects impact

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
