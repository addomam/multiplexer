# ACP retained-pipe cleanup — S04-OPS-01

Status: S04-OPS-01 and S04-OPS-02 repaired locally; current full gates and fresh round3 still required. All S04 tasks/whole story remain failing. Feature branch feat/acp-agent-interaction; historical reviewed source/evidence checkpoint322652c. No live/provider/auth/install/global/publication authority.

## Reproduce

Fresh S04 specialist round1 reports a high-confidence operational must-fix, not an executed reproduction: an adapter descendant can leave the original session and ancestry while retaining stdin/stdout/stderr. Known-tree termination then cannot produce EOF/write completion, yet ACP cleanup unconditionally joins blocking workers. Raw report: specs/security/epics/e04/e04s04-review-round1.json. A/B100/100 do not override that failed AND gate.

Actual fixture/regression now implements this sequence. Before repair, artifact1449/bg214 executes exact1 and fails after12.76s: close is canonically admitted, captured peer absent, other-session reparented retainer alive, cleanup incomplete. Releasing only those endpoints completes the original cleanup before the test deliberately fails. Failure cleanup and daemon teardown then finish; no unqualified PID/group signal. This is production RED, unlike the earlier S04 fixture failures.

## Isolate

Confirmed boundary: the original known tree is gone while the close waits; only closing the independently retained endpoints unblocks it. That rules out authority/admission and tree termination as the blocker. The unchanged run cleanup joined blocking stdin/stdout/stderr workers with no independent I/O wakeup. ProcessTree's ancestry/session limitation is not changed or falsely represented as comprehensive orphan capture.

## Hypothesize

1. Retained endpoints keep pipe I/O blocked after the known tree is gone. Falsification: the bounded real-child regression must return normally even while the retainer stays alive; if so, this candidate is disproved.
2. Tree termination or authority/admission, rather than pipe I/O, blocks completion. Falsification: exact close admission plus original-peer absence and completion after only fixture endpoint release rule these out.

## Verify and implementation boundary

Focused GREEN: bg215 executes the same exact case1 PASS in3.14s after the minimal owned-pipe repair. The case covers explicit close and direct daemon shutdown while the retainer remains alive; incomplete cleanup stays failed/charged, seven siblings fill the remaining slots and another open is rejected, an independent original RPC computes42, and the daemon exits0 before fixture release/reaping. The unrelated sentinel and owned root/socket boundaries hold.

Repair: nonblocking owned stdio waits on readiness plus one shared sticky cancellation socket. The owner drops the sole wake sender after known-tree cleanup, then joins every worker. Confirmed pipe hangup can drain finite already-buffered bytes to EOF; a still-held endpoint aborts with ConnectionAborted (not write_all-retried Interrupted), records cleanup uncertainty and retains the slot. No thread detaches, unknown PID kills, admitted-work replay, late ownership revalidation, provider runtime or new public protocol. Two cancellation FDs are bounded by the existing eight charged-instance limit.

Current contiguous bg216/artifact1457 passes exact8, workspace392/30suites/10ignored, default281/12suites/7ignored, strict Rust/fmt, OMP8/49/Zsh, final native guards2 and actual showcase1 (218.86s wall). Actual outside-test built-CLI close takes0.224s; shutdown0.268s/0.258s, with daemon0 before fixture release, PID absence and root/socket cleanup without fallback. These are observations, not latency guarantees. Evidence: e04s04-retained-pipe-runtime.json and e04s04-retained-pipe-before-after.json. Require current self-audit and fresh round2 A/B/security before local acceptance; no focused/full command alone is story acceptance. Historical caps/native fence and the separate absent real-adapter task remain unchanged.

## S04-OPS-02 — idle/early-exit endpoint accounting

Reproduce: fresh round2 A and specialist independently identify lost endpoint observability. Actual built-CLI e04s04-idle-pipe-before.json confirms it at6a94038: no application prompt/held write, captured peer absent and escaped stdin-only retainer alive, but cleanup_complete=true/failure=null/lifecycle=ended. Owned daemon0/release/PID/root/socket cleanup succeeds; this is a failed truth counterexample, not acceptance.

Isolate/verify: cleanup uncertainty is set only by a transfer after stopped. The idle writer can return/drop stdin before that call; an early-error/non-live reader can likewise drop stdout. Known-tree cleanup then joins workers and falsely releases the target charge. The blocked-all-three case does not cover these paths. Admission/process kill is not stuck; the actual incorrect terminal metadata is observed after the close reply.

Plan: add real-child idle-stdin close/direct-shutdown and stdout-only early-frame-failure cases, with terminal cleanup witness, retainer liveness, remaining-slot capacity rejection, unrelated sentinel and independent explicit release/reaping. Preserve compiled RED separately. Then keep the original stdio handles in the one shared owner through known-tree termination/final classification, regardless of worker return or partial startup. Use safe shared File I/O without duplicate descriptors or a new allocation per endpoint; poll opposite-end hangup without probe payloads. Cancellation remains sticky/independent and every worker joined. Check cancellation before the next syscall, preserving successful byte counts rather than replacing an already completed transfer. Do not reintroduce continuous-producer hangs, signal unknown retainers or alter ownership admission. Follow with exact GREEN, actual idle/early-output CLI proof, full contiguous gates, self-audit and fresh round3. Rounds1–2 and historical caps remain recorded, never reset.

### Compiled RED and repaired runtime

Test-only8a71a14 preserves two actual compiled failures at bg220/artifact1498 (2 failed,1.01s test/4.07s wall): idle stdin close falsely reports ended/cleanup_complete=true/failure=null; oversized initialization with stdout-only retainer falsely reports failed/frame/cleanup_complete=true. Both independently observe retained process liveness and failure cleanup; neither is a compilation failure or a fabricated defect. The RED commit contains only acceptance.rs and acp_peer.rs. No bundled TDD-isolation script exists; Git log/diff-tree records the test-only separation.

One shared owner now holds the original three stdio Files, safely converted through OwnedFd without duplicate FDs or per-endpoint allocations, before initialization publication. Workers borrow those Files through the shared Arc. After known-tree termination and all joins, the owner independently probes every endpoint needed to establish closure; absent handles, probe errors or a still-held endpoint remain cleanup-failed/charged. Idle or early-return workers cannot erase this evidence. Cancellation is checked before the next syscall; a completed syscall returns its actual count rather than rewriting delivery after a stop race. Continuous producers remain cancellable and finite hung-up buffers can drain. Two wake FDs/one shared allocation remain bounded by the existing eight charged-instance limit; no process identity expansion, unrelated signals, replay or public protocol changes.

Repair attempt bg221 failed compilation (missing pollfd.revents), not a behavioral RED/PASS. After correcting that initializer, bg222 passes both new cases (3.14s), the previous all-three blocked case (3.13s) and cargo build,12.07s total. The new cases cover idle close/direct shutdown, early stdout frame-reader failure, seven remaining charged slots/extra-open rejection, daemon0 before explicit retainer release, sentinel and owned cleanup.

Actual outside-test built CLI/private daemon smoke passes all three paths: idle close, idle direct shutdown and early stdout failure. Closed/failed instances report cleanup_complete=false/failure=cleanup; direct shutdown has no post-exit inspect claim. Every daemon exits0 before retainer release; peer is already absent, retainer exits only after explicit release, unrelated sentinel survives until its own termination, socket/root disappear without fallback. No PTY/drainer or provider/auth process starts. Evidence: specs/verifications/e04s04-endpoint-truth-runtime.json. Full contiguous regression and fresh independent round3 remain required; historical rounds1–2 FAIL and absent/unapproved live Task4 are unchanged.
