---
name: verify-reopen-settlement
description: "Check that amux refuses to reopen a saved conversation while a previous reopen attempt is still unsettled, so a crash cannot start a second live process. Use when changing reopen_conversation, mark_reopen, refresh_observed_peer, acp_open, or agent_open."
---

# Verify reopen settlement

A saved conversation must not gain a second live process because a daemon crashed after it recorded a reopen attempt and before it recorded the new process. The harm is two agents running one conversation. Tests cover particular orders. This check covers every order of the steps below for one record.

## What is checked

The property: while `launch_pending` is set, a later daemon start does not spawn another process for that record. A crash, or an unsettled failure, leaves `launch_pending` set. A failure that is known not to have started a process clears it and may be retried. After the new process id is recorded, `launch_pending` is clear and the recorded peer is alive, so the next start also refuses.

The model is `.cursor/skills/verify-reopen-settlement/ReopenSettlement.tla`. Its steps are the code steps:

| Model step | Code |
|---|---|
| `RefusePending` | `reopen_conversation` rejects `record.launch_pending` |
| `RefuseAlivePeer` | `reopen_conversation` rejects a recorded peer that is still alive |
| `MarkPending` | `mark_reopen(record, true)` after `same_reopen_intent` |
| `Spawn` | `open_inner`, `acp.start`, or the Codex open that follows the mark |
| `Rollback` | `mark_reopen(record, false)` when that launch is known not to have started a process |
| `Settle` | `refresh_observed_peer` after `checkpoint_recovery_inner` has a `peer_pid` |
| `Crash` | the daemon stops, or Codex leaves the mark in place because the failure is not settled |

`CheckPending` is true in `ReopenSettlement.cfg`. `ReopenSettlementBroken.cfg` sets it false, which is the code with the `launch_pending` refusal removed. That run must report `Invariant NoDuplicate` violated: mark, spawn, crash before settle, then mark and spawn again.

Tool: TLC from TLA+ tools 1.7.4. This is an ordering question over a small model of the reopen steps, which is what TLC decides. It needs a JDK 17 or newer (`brew install openjdk`). The checker downloads `tla2tools.jar` on first run.

## Run

From the repo root:

```sh
.cursor/skills/verify-reopen-settlement/bin/check
```

Exit 0 means the covered source still contains those steps in that order, the source matches the sync commit, TLC found no error with the refusal present, and TLC found a second live process with the refusal removed. When TLC prints a violation, read that result with `/statechart` before deciding it is a product bug or a modeling error.

## Coverage

Confirmed at commit `2033f4a588d835d4011ce70d2790b6eaa261aa5d`.

- `src/daemon/recovery.rs`: `reopen_conversation`, `mark_reopen`, `refresh_observed_peer`, `checkpoint_recovery_inner`
- `src/daemon.rs`: `acp_open`, `agent_open`

If `git diff 2033f4a588d835d4011ce70d2790b6eaa261aa5d -- src/daemon/recovery.rs src/daemon.rs` is not empty, the check exits 2. Read the diff. Move the sync commit in `bin/check` and in this section only after you confirm the table still describes the code at the new commit. Do not move it to make a failing run pass.

A missing `!record.launch_pending` refusal, a spawn before `mark_reopen`, or a settle before `peer_pid` is a failed check, not a prompt to weaken the model.

## Scope

One saved record that is not `RecoveryBlock`ed, whose recorded peer is already dead at the first attempt. `blocked`, journal IO, project metadata edits, and the distinction among ACP, OMP, and Codex except the mark-before-start order and the conditional Codex rollback are outside the model. A pass is every interleaving of these steps, not the whole daemon. Two daemons cannot run the same runtime; the lock in `daemon::run` is not part of this model. The duplicate the model catches is a later start of one daemon after a crash, not two daemons at once.

## Product finding

At the sync commit the check passes. There is no counterexample in the current code under this scope.
