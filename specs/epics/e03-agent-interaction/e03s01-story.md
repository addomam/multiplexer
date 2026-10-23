---
story_id: e03s01
title: Exclusive ordinary PTY control and cooperative handoff
maturity: 3
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 8
status: passing
---

## 1 Identity
First e03 slice; implements ordinary capability ownership end to end. Manifest acceptance: interaction-07, interaction-08, interaction-25, interaction-27, interaction-28, interaction-29, interaction-30. Plan only; implementation is not authorized by this document.

## 2 User story
As a native human or external local controller, I can acquire one PTY, finish a coordination step when asked, and explicitly transfer it to its reserved requester without concurrent or stale input.

## 3 Problem
Existing input/execute producers do not carry the selected capability. Guarding only a new API would leave native paste, shell wake continuations and automatic startup able to bypass ownership.

## 4 Goal
Two real local controllers exchange a settled PTY through request, yield and accept; every current input/execute producer uses the same daemon authority. Unknown application settlement blocks safe transfer rather than producing a false grant.

## 5 Scope
Controller registration/private recovery; ordinary now claims; controls, claims and offers; release/yield; public snapshots/events; dispatch accounting; all naked-input/execute caller cutover; initial native connection identity and cached control. Force/cancel are s02, complete UX s03, conditional consent s04, agent operations s05 onward.

## 6 Actors
Daemon owns in-memory authority and accounting. External controllers reuse serialized registration across sockets. Each original persistent native connection has its own attachment controller. Shell integration reports acceptance/progress but is not an independent controller.

## 7 Preconditions
Build requires phase confirmation and isolated feature branch. Refresh LSP references before changing exported symbols. Use existing framing, IDs, typed errors, shell readiness and integration contracts; no legacy input compatibility path remains.

## 8 Dependencies
No story prerequisite. Normative inputs: PLAN.md ordinary cutover, contracts/pty-control.md and schema/examples, ADR0001/0002, CONVENTIONS.md. No package addition. Later s08 must close the application-fence seam before first acceptance.

## 9 Module purpose callers and contracts
`src/control.rs` centralizes closed capability/claim/offer decisions and retained dispatch accounting. Reason for Depth: callers must not reconstruct generation, reservation, revocation and settlement independently. `daemon.rs` admits requests and preserves topology/events; `runtime.rs` consumes the bounded writer queue; `integration.rs` and `integrations/zsh/adapter.zsh` retain originating authority through epoch/wake. `client.rs`/`cli.rs`, `tui.rs`, acceptance/shell-race helpers and independent showcase execute producers consume the changed contract. Preserve stock Zsh, native terminal query replies, busy/partial/readiness errors and process lifetime.

## 10 Requirements
- **MODIFIED — input/execute admission. Before:** existing public producers submit input/execute without a ControlHandle. **After:** both operations require current correctly bound control; CLI input/execute require serialized `--control`; TUI keys and separate paste use cached authority. Missing/wrong/stale handles reject before forwarding. Migrate examples and test producers, not a silent auto-acquire shim.
- **ADDED — role and lifetime.** ControllerHandle registers/recovers, ClaimHandle records intent and OfferHandle proposes transfer; none authorizes input. Only current ControlHandle binds controller, live PTY and generation. References never revive across restart/exit/replacement; public events contain no usable private handles.
- **ADDED — cooperative transfer.** A request preserves old authority. Yield/release revokes and rejects undispatched work, preserves requester reservation and forwarded accounting, then offers only after authoritative settlement. Read does not accept; exact offer acceptance commits fresh authority atomically. Competing acquisition cannot steal the gap.
- **MODIFIED — queued and automatic work. Before:** source/epoch/wake and automatic launch paths need no originating ownership capability. **After:** retain authority through queue consumption and native acceptance. A private nonpersisted external-role launch controller binds automatic command/recipe/OMP startup; retire it by normal release after authoritative acceptance/dispatch settlement, never fixed sleep/socket write. Caller-directed future managed Codex startup instead uses its returned initial control. Reports can settle old work but cannot dispatch revoked input.
- **ADDED — settlement truth.** A writer acknowledgement is delivery only. Empty forwarded ledger/genuinely settled source acceptance can support this slice's safe path; genuinely unknown native application remains `settlement-unavailable`. No whole-turn finish, question resolution or timeout substitutes for the fence. No hot-path registration or per-key heap record is required; use borrowed cached controls and bounded batch/watermark accounting.

## 11 Interfaces and data
Implement closed v1 `controller-create`, `ownership-claim` cooperative `when:{kind:now}`, and applicable `control-handle` methods from the schema. Compare `observed:{generation,reservation_id}`; it is not authority. Store private capabilities separately from PublicOwnership, sequenced events and unresolved call records. Original native connection provenance survives dispatch; initial view is unknown, departure is not release. Preserve nested meaningful nulls in raw CLI requests.

## 12 Main flow
Register A/B; A acquires an unowned settled PTY and supplies control with input. B requests now; A can still input. A yields exact claim; queued old work is rejected, forwarded records settle and reservation excludes C. B reads ready offer, explicitly accepts and receives new control; only fresh deliberate B input can reach the PTY.

## 13 Alternate and error flows
Wrong-kind/forged-tag/wrong-controller/old-lifetime handles fail without bytes. Late yield/accept targets cannot select a replacement transfer. Duplicate accept returns consumed/stale and recovery reads current private state. Unknown forwarded outcomes remain visible blockers. Slow consumers cannot hold ownership/topology locks or stall PTY processing. Nonowning attachments remain observers; automatic query replies continue through daemon-classified paths, never a caller-provided protocol label.

## 14 Nonfunctional constraints
All admission, queue-forward registration and revocation linearize per PTY. No locks span harness/hook I/O. Runtime memory only; no durable control ledger. Preserve independent showcase's no-amux-import oracle and existing readiness/deadlock/latency boundaries. No harness/model calls in this slice.

## 15 Implementation steps
1. Add `src/control.rs`, daemon registration/ordinary operations, private recovery and PublicOwnership; create `tests/pty_control.rs` with real-daemon role/lifetime and two-controller tests -> verify: `cargo test --test pty_control`
2. Implement reserved yield/settlement/read/accept ordering and public events without private references; exercise the genuinely settled empty-ledger path and competing requester -> verify: `cargo test --test pty_control`
3. Bind runtime writer and shell source/epoch/wake continuations to originating authority, with retained forwarded records and private startup controller retirement -> verify: `cargo test --test pty_control`
4. Cut over CLI/client/TUI including paste, automatic launches, acceptance/shell-race helpers and showcase producers; update existing input/execute docs/examples and preserve original negative-test boundaries -> verify: `cargo test --test pty_control && cargo test --test acceptance --test shell_races --test integration --test readiness --test runtime`

## 16 Verification script
Future commands above create/use the `pty_control` target, not a current Plan-time target. Real-daemon cases use private roots/sockets and owned process cleanup with deadlines. Assert target effects and exact non-forwarded stale markers, not only response codes. Pause queue consumption before yield, then resume; an old shell wake cannot inject a command, while genuine terminal replies and old settlement reports still progress. Exercise CLI serialization and native persistent connection input/paste under two identities. Existing negative shell tests must carry valid authority so they still test busy/partial/readiness rather than failing earlier at ownership. Parent's later workspace preflight and explicit ignored independent showcase command in PLAN remain integration obligations. This slice supplies no full native UI or complete-broker proof.

## 17. Acceptance criteria
1. **interaction-07:** A alone can input; B still observes and terminal query progress continues. Missing or wrong authority leaves a unique target marker absent.
2. **interaction-08:** B cannot input after notification or offer read; yield, authoritative settlement and explicit current-offer acceptance are each necessary for a fresh safe grant on the genuinely settled path.
3. **interaction-25:** A's token works after request, stops dispatching at yield, and queued old commands cannot be revived by later shell reports.
4. **interaction-27:** Late yield/accept against consumed, ended or replaced transfer identities cannot resurrect a reservation or alter a newer generation; s02 adds cancel/force races.
5. **interaction-28:** C cannot acquire between yield and B's accept; reservation identity, not event sequence, protects the gap. s02 adds cancelled-reservation cleanup.
6. **interaction-29:** Controller/claim/offer handles, altered kind tags and stale controls cannot authorize native or public input/execute; public snapshots expose none of their references.
7. **interaction-30:** Claim reads are side-effect-free; successful accept returns current control, while duplicate/stale accept returns explicit non-granted outcome recoverable by private inspection.

## 18 Out of scope
No generic capability registry, retry policy, provider integration, new launch defaults, force-only safe-transfer claim or early native-fence spike. s08 owns complete-broker native application and authoritative presentation validation; its guarantee is not waived.

## 19 Risks
P0: authority gaps in separate paste/automatic startup/source continuations; detect with paused-consumption markers and consumer tests. P0: falsely equating delivered with applied; retain unknown and block safe grant. P0: leaking capabilities in snapshot/events; assert public observations cannot be used as handles. The initially settled path is deliberately narrower evidence than whole e03 acceptance, not narrower product scope.

## 20 Evidence and handoff
All tasks start failing. No runtime command, probe or test ran in Plan. Record later redacted evidence with exact commands and cleanup; hand off working ordinary ownership to s02, cached original-connection authority to s03, and unresolved native application obligations to s08. No e03 production acceptance until all 32 scenarios pass.
