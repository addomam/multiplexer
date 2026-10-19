---
type: bug_fix
context: amux terminal compatibility and execution-substrate reliability
bug_id: BUG-substrate-reliability
status: fixed
severity: medium
scope: terminal-and-daemon
---

# Known substrate defects — fix scope

Status: fixed, verified and committed on `fix/substrate-defects`; no merge or push requested. Evidence: [substrate reliability](../verifications/substrate-reliability.md).

## Problem and accepted evidence

The readiness assessment records terminal CPR/grapheme failures and a reproduced shared-config lost update. Source inspection also identifies incomplete resource/diagnostic bounds, startup-readiness ordering, non-cancellable running hooks and incomplete save durability.

Security impact: LOW for terminal/config/readiness correctness; MEDIUM for resource exhaustion by a trusted same-user automation client. The local API already grants that user process-control authority; no cross-user privilege escalation was identified. Unbounded retained resources can exhaust the daemon through repeated explicit opens or failed launches.

Existing terminal failures are related to, not recurrences of, earlier input-latency fixes. Preserve their original independent expectations and historical output. Do not rerun accepted failures merely to establish them again.

## Isolation and hypotheses

- Terminal parsing delegates to a pinned emulator whose CPR handler uses physical cursor coordinates and whose printable input advances per Unicode scalar. Inspect the parser/handler boundary for a general correction; do not special-case the showcase's text or expected replies.
- Configuration serializes each in-memory store independently. Socket locking does not serialize or detect stale configuration writers. Protect read/compare/write under a per-config lock and reject stale writers, or establish an explicit single-writer contract. Successful saves must sync the renamed directory entry.
- Launch-failure and hook-enqueue-failure diagnostics bypass retention pruning. Centralize bounded insertion across all producers.
- Opens reserve no aggregate capacity before spawning. Reserve retained-record capacity atomically, count in-flight opens, and return capacity on failure/removal without silently discarding exited records.
- A connectable socket precedes initialization. Readiness must require a valid initialized API response, including the existing-daemon path.
- Hook workers observe timeout but not daemon stopping. Propagate cancellation into the owned process-group loop; pending hooks must not start after cancellation.

## Fix plan and shared contracts

1. Preserve existing working changes and use a dedicated fix branch. Baseline `cargo test --workspace`, workspace Clippy and workspace build passed before implementation (177 tests; four ignored helpers/integration checks).
2. Add focused consumer-visible regression coverage for origin-relative reports/mode transitions and joined clusters/CJK/combining/wrap/overwrite. Keep raw wire expectations independent of screen projection. Generalize terminal changes through the emulator boundary, not byte-stream substitution.
3. Make global `terminal_limit` configurable with a default of 64 and a supported range of 1..256, counting live, retained-exited and reserved/in-flight records. Lowering the limit never deletes existing records; it refuses new admission until capacity is available.
4. Keep existing config disk shape versioned; add a defaulted terminal-limit field. Canonicalize file identity, use a stable private nonblocking lock (`config-busy`), reject stale snapshots (`config-conflict`), and sync the containing directory after rename. Test independent stores/daemons and saved-project state, not only one field. Never wait on an external config lock while holding the daemon state mutex.
5. Change hook execution to accept `&AtomicBool` cancellation, updating every production and test caller. Keep Git process execution behavior unchanged. Report cancellation separately from timeout/error; kill/reap the owned process group before returning.
6. Require a bounded versioned API readiness handshake for daemon auto-start and existing-socket attachment. Do not replay mutations. Use bounded diagnostics for every producer and bounded command/output text.
7. Upgrade the independent showcase integration acceptance to require zero objective failures after the fixes. Exercise actual binaries/private daemons, full workspace tests, Clippy, release build, OMP tests and Zsh syntax.

Each code owner adds regressions before implementation where practical. Child tasks do not run builds/tests/formatters mid-flight; the integration owner runs checks after the shared tree is complete. Existing accepted failures and preserved baseline runtime probes provide failing-before evidence without repeating known errors. No unrequested commits or unavailable helper-script claims.

## Impact and verification

Risk: HIGH: terminal parser semantics, daemon/client startup, shared configuration and hook process lifetime affect all local CLI/TUI/OMP callers. No exported operation is removed; terminal-limit admission and stale-config errors become explicit API outcomes. Public hook signature callers are migrated together.

Affected stories: e01 runtime, project/hooks, integration and acceptance; e02 independent endpoint compatibility check. Existing concurrency, native input, row-delta and cleanup regressions remain required.

`verify: cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace --release`

Additional verification: explicit ignored independent-multiplexer integration, native PTY attach/input/detach, private-daemon config conflict, bounded admission, real startup-readiness and running-hook cancellation/descendant cleanup. Parent directory fsync is source-verified; no power-loss simulation claim.

## Resolution

All eight registered defects meet their acceptance criteria on the verified macOS arm64 branch. The independent showcase now requires unchanged expected CPR values, objective Verified for both capabilities, and strict exit 0. A separate 13-case raw-terminal probe and release-TUI attach/input/detach/reattach smoke pass.

Integrated checks: 210 Rust tests passed, five ignored subprocess/integration entries; the explicit independent multiplexer test also passed. Workspace Clippy with warnings denied, debug/release builds, 8 OMP tests and Zsh syntax passed. Partial integration-directory setup and uninstalled PTY-child errors now clean owned resources; grapheme working text is capped at 4096 UTF-8 bytes with documented native scalar segmentation beyond the bound.

The first full run had one queued-execution acceptance timeout. An instrumented acceptance run, the final complete workspace run and 24 accepted-command cancellation probes passed; no cause is asserted and no test was weakened, disabled or given a longer deadline. Failure snapshots/screens are now retained in assertion output. See the verification report for the exact limitation.

Directory sync is source-verified and exercised by successful saves, not power-loss/fault-injection certified. Native cell-extension bytes remain variable-sized; terminal admission is a record-count policy, not a total memory quota. Preserve e01's exhausted review history: no new independent PASS, review-cap reset, release waiver, autonomous orchestration, live-daemon replacement or landing is implied.
