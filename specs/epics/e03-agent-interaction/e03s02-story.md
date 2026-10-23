---
story_id: e03s02
title: Explicit overrides and recoverable one-shot ownership
maturity: 3
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 8
status: passing
---

## 1 Identity
Second ownership slice, dependent on e03s01. Manifest acceptance: interaction-09, interaction-10, interaction-12, interaction-18, interaction-26, interaction-31, interaction-32. Plan only.

## 2 User story
As a controller confronting a stalled handoff or lost API reply, I can explicitly cancel or force with truthful consequences and recover my own committed state without replaying an uncertain acquisition.

## 3 Problem
A cooperative reservation can outlive its socket. Treating disconnect as cancel or force as task interruption destroys ownership certainty; deleting old call records falsely makes subsequent grants safe.

## 4 Goal
Bounded explicit overrides and serializable private recovery work across independent raw API invocations, retain prior work and never implicitly stop a turn or process.

## 5 Scope
Ordinary claim cancel, claim force, forced-claim creation with compare receipt, resumed acquisition after cancellation, retained outcomes, lost-reply recovery and observer-independent progress. Client waiting remains client-side over primitive read/accept, not new CLI porcelain.

## 6 Actors
External one-shot controller, persistent native controller, disappeared requester, subsequent requester, slow event/hook consumer and daemon. Only the explicit current action changes ownership; notification consumers have no veto.

## 7 Preconditions
s01 has real-daemon capability ownership, target `tests/pty_control.rs`, per-PTY dispatch ordering and retained call accounting. Safe application settlement remains unknown when no authoritative seam exists. No real model calls are needed here.

## 8 Dependencies
e03s01; wire contract sections 2, 3, 5 and 7; PLAN ordinary cutover and evidence limits. s07 later implements exact-turn interruption separately; s08 supplies real harness and complete native application proof.

## 9 Module purpose callers and contracts
`src/control.rs` owns override/compare transitions and accounting; `src/daemon.rs` exposes nonblocking primitives/events; `src/client.rs` retains typed error codes and private-state recovery. `src/cli.rs` raw `api` is the external consumer and `src/tui.rs` the persistent consumer. Writer/integration dispatch must keep rejecting revoked queued work even when old forwarded results arrive. Do not add another retry/wait framework.

## 10 Requirements
- **ADDED — ordinary cancellation.** Before grant, cancel exact current ordinary claim, revoke pre-transfer authority even before yield, remove reservation and grant nobody. Outstanding calls survive. Previous owner may immediately request fresh resumed authority retaining its own work; a different controller still requires settlement or explicit force. After grant, cancel cannot release the new owner.
- **ADDED — explicit force.** Force compares current generation/reservation, revokes prior authority, supersedes only the matching reservation and immediately grants fresh control. Forced creation can recover a stranded reservation without its requester's private handle. Always return forced mode and `prior-calls-may-apply` with actual unresolved records, including an empty list when appropriate.
- **ADDED — retained outcomes.** Yield, cancel, force, resumed acquisition and recovery cannot erase forwarded/unknown calls. Late old-generation results update their own records, never grant or revoke current ownership. Undispatched revoked work stays rejected.
- **ADDED — uncertainty recovery.** Ordinary socket closure, waiter timeout/SIGINT and missing accept reply neither cancel a claim nor release a committed control. Read retained claim/controller state; recover current capabilities, not a replayed acquisition. A historical grant reports `current:false` without usable old control. Raw API exit0 is successful request handling, not proof of a granted lock.
- **ADDED — independence and intent.** Explicit force/cancel do bounded local work independent of slow/failed/absent hooks or observers. Neither overrides nor normal takeover invoke turn interruption or process stop. Preserve the running command/turn and expose blockers after three seconds with no automatic takeover deadline.

## 11 Interfaces and data
Implement ClaimHandle `cancel`/`force {observed}` and `ownership-claim mode:force` exactly as the schema; force omits `when`. Keep public compare receipts nonauthorizing and private controller recovery controller-scoped. `release` is for an already-granted control, never overloaded claim cancel. Preserve typed stale/consumed/completed outcomes, public grant mode, unresolved records and gap/snapshot behavior.

## 12 Main flow
A owns and has tracked forwarded work. B requests; cooperation stalls. B inspects current ownership, explicitly forces and receives fresh control plus unresolved-risk warning while A's running work continues. A cannot dispatch more old input. Later settlement updates only A's old record. If B loses the grant reply, its independent next raw call privately reads the existing grant rather than forcing again.

## 13 Alternate and error flows
Cancel wins before accept: no owner, old control revoked, no B input forwarded; accept wins first: late cancel fails and does not release B. Force/accept compete at one ordering point. A stale receipt cannot force a newer owner/reservation; refresh facts and require deliberate new action. Lost create/accept replies require controller inspection, not blind repeat. Restart/target exit invalidates old credentials. Native departure retains server ownership; another controller can explicitly force using fresh public facts without stolen private handles.

## 14 Nonfunctional constraints
No global/ownership lock across notifications, client waits or harness I/O. Use deterministic barriers for race winners, bounded real-daemon deadlines for latency/cleanup and exact non-forwarded effect assertions. Keep same-user ownership separate from security sandbox claims. No auto-release, auto-retry or implicit task policy.

## 15 Implementation steps
1. Add cancel/unowned/resumed transitions and retained accounting in `src/control.rs`/`src/daemon.rs`, extending `tests/pty_control.rs` with both cancel/accept orderings -> verify: `cargo test --test pty_control`
2. Implement claim force and public-receipt forced creation, generation comparisons and unresolved warnings; test queue revocation and stranded reservations -> verify: `cargo test --test pty_control`
3. Exercise private recovery across separate raw CLI/socket calls, timeout and lost committed-accept reply; preserve typed client errors and document raw exit semantics in existing API docs -> verify: `cargo test --test pty_control`
4. Emit real blocker progress without waiting for consumers, and prove force/cancel plus PTY output continue with blocked/absent event readers -> verify: `cargo test --test pty_control`

## 16 Verification script
Future tests extend s01's target. Use private daemon/socket/root and controlled running shell work with an observable post-transition marker and unchanged execution identity; reap only owned processes. Hold forwarded work unresolved and queue a second mutation, then cancel/force: old queued bytes never land while retained records remain inspectable. Inject late results and assert current ownership is unchanged. Separate one-shot client processes retain serialized credentials, close while waiting, and deliberately drop an accept response after commit; inspect the exact same claim/controller to recover. Exercise historical grant inspection after ownership changes and snapshot recovery after event gaps. Bound latency checks with actual stalled subscribers rather than mocked success. Real agent noninterruption and native-buffer application settlement remain s08 proof obligations; a shell marker alone is not that evidence.

## 17. Acceptance criteria
1. **interaction-09:** Ordinary/forced transfer and cancel preserve running execution identity/output; none dispatches a turn interruption or process-stop action. Real agent proof remains required in s08.
2. **interaction-10:** Cancel/force reject revoked undispatched queue entries, preserve forwarded/unknown accounting and never call a PTY write safe application settlement. Late outcomes cannot alter the winner; s08 completes native application proof.
3. **interaction-12:** Blocked/failed/absent notification consumers do not delay explicit force/cancel or PTY output; ordinary cooperation may still wait for its owner.
4. **interaction-18:** Fresh force grants immediately without settling old work, includes the actual unresolved warning and rejects subsequent old automation without interrupting running work.
5. **interaction-26:** Pre-grant cancel grants neither party and removes reservation; prior owner is not restored, can freshly resume its own retained work, and a third party cannot misread unowned as settled.
6. **interaction-31:** A timeout/closed waiter cannot cancel claim or release a lost-reply grant; controller/claim inspection resolves current versus historical authority without replay.
7. **interaction-32:** Separate raw API processes retain one serialized claim through read/offer/accept; persistent native clients use the identical lifecycle, with cached authority rather than registration per input.

## 18 Out of scope
No exact-turn interrupt implementation, process-stop redesign, sync CLI command naming, reconnect engine, fairness queue or real Codex run. No early isolated native-fence investigation and no waiver of the final guarantee.

## 19 Risks
P0: cancellation accidentally restores old token; test fresh resumed generation and invalid old control. P0: stale force revokes an unrelated winner; barrier-controlled receipt races. P0: lost accept reply triggers double mutation; inspect retained authoritative state. P0: accounting is cleared on override; assert unresolved records and late settlement across each transition.

## 20 Evidence and handoff
Ledger remains failing; all commands are future Build checks and none ran here. Record redacted serialized-role recovery, observed effect markers, race outcomes and owned-resource cleanup later. Hand off deliberate native controls to s03; agent noninterruption and all native settlement/presentation obligations remain final s08 acceptance, with all 32 criteria intact.
