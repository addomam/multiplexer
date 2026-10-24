# ACP retained-pipe cleanup — S04-OPS-01

Status: confirmed production defect; focused repair PASS, full runtime/regression/audit/review pending. All S04 tasks remain failing. Feature branch feat/acp-agent-interaction; checkpoint3f0780b, test-only RED9c46d0b. No live/provider/auth/install/global/publication authority.

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

Full ACP/PTY/workspace/default/OMP/Zsh/showcase gates, actual outside-test CLI lifecycle, self-audit and fresh round2 A/B/security remain required. No focused pass is complete acceptance. Historical caps/native fence and the separate absent real-adapter task remain unchanged.
