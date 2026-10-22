---
story_id: e03s07
title: Exact-turn interruption and truthful lifecycle
maturity: 3
status: passing
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 5
---

## 1 Identity

Slice e03s07; manifest delta ADDED; dependency e03s06. Primary acceptance is interaction-14; lifecycle/degradation tests preserve interaction-13 from e03s05 without changing manifest ownership.

## 2 User story

As the current controller, I can explicitly interrupt the exact running turn, observe whether interruption actually completes and distinguish that from a request acknowledgement, later request cleanup or stopping the agent process.

## 3 Problem

An empty provider interrupt acknowledgement can precede interrupted completion and pending-request cleanup. Treating it as completed cancellation, or interrupting whichever turn is current after a race, can misreport work or cancel unrelated work. Process exit is not successful task completion.

## 4 Goal

Deliver agent-interrupt bound to exact live turn and current control, with retained outcomes and honest lifecycle dimensions. Keep takeover, pending takeover cancellation, conditional keep/withdrawal, turn interruption and process stop semantically separate.

## 5 Scope

Extend src/codex.rs and daemon/client/CLI closed operation routing with agent-interrupt. Integrate execution teardown/degradation in src/daemon.rs and src/runtime.rs without changing detach semantics. Expand existing tests/agent_interaction.rs, tests/pty_control.rs and tests/codex_native_smoke.rs; add no new package or verification target.

## 6 Actors

The external controller or owning human chooses explicit interruption. The daemon validates ownership/identity and owns cleanup. Codex authoritatively reports turn outcome and pending-request resolution. Native viewers remain observers of the same frontend and can detach without stopping it.

## 7 Preconditions

e03s06 provides same-instance live target, exact pending registry/tombstones, answer/permission behavior and future test targets created by e03s05. e03s02 supplies force/cancel and uncertain one-shot recovery. Real verification additionally requires later explicit live-run approval, installed 0.160.0 version/auth/native-PTY prerequisites and explicit env/model opt-in.

## 8 Dependencies

Reuse [OK] tungstenite 0.30.0 handshake-only from e03s05 and the existing event/snapshot service. src/control.rs registers actual dispatch at the same ordering point as revocation and retains unresolved calls across generation changes. No retry/reconnection service, process supervisor framework or provider abstraction is introduced.

## 9 Module purpose callers and contracts

src/codex.rs validates exact AgentTarget plus turn_id, maps interruption to provider protocol and records acknowledgement, authoritative interrupted completion and pending cleanup separately. Daemon raw API dispatch is its mutation caller; snapshots/events and native rendering consume lifecycle facts. src/runtime.rs/daemon execution lifetime owns stop/PTY close cleanup, never treating detach or a successful interrupt as resource destruction. Keep process lifetime, activity, pending input, turn outcome, capability availability and external task acceptance as distinct fields/states. No provider result changes current ownership generation.

## 10 Requirements

- ADDED: agent-interrupt requires target={terminal,instance_id,session_id}, current control and exact turn_id, binding to turn/interrupt. Validate target, current authority and exact live turn before registration/forwarding, and recheck queued authority. A stale turn never selects or interrupts the replacement turn.
- ADDED: Return an inspectable call receipt distinguishing queued, forwarded, applied/rejected and unknown outcomes. Interruption acknowledgement is not interrupted completion. Observe authoritative turn outcome and any later pending-request cleanup independently; do not erase unresolved questions because a turn completion arrived first.
- ADDED: Duplicate, old-session, old-instance, already-ended-turn and revoked-authority interruption attempts fail explicitly without a new dispatch. An uncertain socket reply is recovered by inspect rather than repeated blindly. Late old-turn outcomes can update only retained matching records and cannot complete a newer turn or alter ownership.
- ADDED: Safe or forced takeover, ordinary takeover cancellation and conditional keep/withdrawal do not issue turn/interrupt or kill processes. Only the explicit interrupt operation requests turn cancellation. A process stop/PTY close is separate and cleans up the execution's owned resources rather than claiming successful cancellation or task acceptance.
- ADDED: Frontend/backend/control/report loss and event gaps expose degraded/unavailable/unknown/resynchronizing dimensions. Preserve retained call and pending identity history, reject unsafe new mutations, and never recreate/reconnect/replay automatically. Detaching a viewer keeps PTY/frontend/backend and ongoing turn intact; explicit execution stop cleans all and only owned resources.

## 11 Interfaces and data

PLAN binds agent-interrupt to turn/interrupt and states "Acknowledgement, interrupted completion, later pending-request cleanup and process stop remain distinct." ADR0003 records that its empty acknowledgement is not completed cancellation and that request cleanup may follow completion. Bind installed Codex 0.160.0 generated schema rather than guessing response/event names; generation via `codex app-server generate-json-schema --out PATH` uses private temporary storage that is removed afterward. Preserve existing amux v1 closed-field validation and stable IDs. agent-inspect and existing sequenced events expose retained call ID, exact turn identity, authoritative outcome and remaining pending requests, never private capability references.

## 12 Main flow

Inspect the same live native/API target and active turn. Submit agent-interrupt with current control and that exact turn_id. Record actual forwarding under the per-PTY ordering point and report acknowledgement without pretending it is completion. Observe interrupted turn completion, then later pending-request cleanup if applicable. Confirm the frontend/backend remain alive and accept a later deliberately started turn only when appropriate. Explicit stop is separately chosen and reaps owned resources.

## 13 Alternate and error flows

If the turn ends or another starts before dispatch, reject the stale exact target without interrupting the new one. If force/revocation wins before forwarding, reject the queued request; if forwarding wins first, retain the old call through later ownership changes. Timeout or socket loss leaves an uncertain receipt and never causes implicit cancellation/retry. A completion arriving before pending cleanup does not falsely clear the request registry. Provider or frontend exit is degradation, not task success. Event gaps refresh snapshots; old-session events never replace current state. Explicit stop during unknown interruption cleans resources without retroactively labelling the interruption applied.

## 14 Nonfunctional constraints

Never wait for provider events or cleanup under global/ownership locks, and never block PTY processing on slow consumers. Use retained accounting with no per-key registration or new dependency. Each real smoke has <=1800s watchdog, daemon/backend/model-response deadlines <=15/30/180s and <=6 meaningful model turns; interruption is not a loophole to omit turns from the budget. Isolate mode0700 HOME/config/runtime/CODEX_HOME/workspace/socket roots and necessary private auth. On all exit paths restore native modes, terminate and reap only owned process groups, verify socket/root removal, delete credentials/schemas/rollouts/markers and retain redacted evidence. Global Codex resources and binary remain untouched.

## 15 Implementation steps

1. Implement agent-interrupt exact-target/control routing in src/codex.rs, src/daemon.rs, src/client.rs and src/cli.rs; expand tests/agent_interaction.rs for stale-turn/non-retargeting and acknowledgement-versus-completion -> verify: `cargo test --test agent_interaction`.
2. Integrate independent pending cleanup, event-gap/exit degradation and owned execution teardown in src/codex.rs, src/daemon.rs and src/runtime.rs; test old-call outcomes, revoked queue races, force/cancel non-interruption and detach versus stop -> verify: `cargo test --test agent_interaction && cargo test --test pty_control`.
3. Add ignored exact root real_explicit_interrupt to tests/codex_native_smoke.rs with inherited guards/bounds/private cleanup; update specs/adr/0003-codex-first-agent-target.md with actual lifecycle/evidence boundaries -> verify: `cargo test --test codex_native_smoke --no-run && cargo test --test agent_interaction`.
4. After separate explicit live-run approval, interrupt a real exact turn on the same native/API instance, inspect authoritative completion/cleanup separately, and prove takeover/detach do not stop the execution -> verify: `test "${AMUX_RUN_REAL_CODEX:-}" = 1 && test -n "${AMUX_CODEX_MODEL:-}" && cargo test --test codex_native_smoke real_explicit_interrupt -- --ignored --exact --nocapture --test-threads=1`.

## 16 Verification script

Use e03s05-created agent_interaction fixtures to deterministically reorder empty interrupt acknowledgement, turn completion, delayed request resolution, provider disconnect and queued revocation. Assert stale-turn rejection at the actual dispatch boundary and inspect retained outcomes after lost replies; do not merely test serializer defaults or source wiring. Extend e03s01-created pty_control for force/cancel preserving turn/process and old results preserving newer authority. Fixture proof is not evidence of real interruption. Compile the expanded smoke without running it, then only after explicit approval set AMUX_RUN_REAL_CODEX=1 and nonempty AMUX_CODEX_MODEL and select real_explicit_interrupt exactly. Missing installed version/auth/model/PTY prerequisites fail visibly. Use actual amux controlling-PTY attachments, retain separate visible captures, and observe the provider's interrupted completion and remaining pending request identities. Never assert that pending cleanup must precede completion. Verify process survival through interrupt/detach, later explicit stop, and all owned-resource cleanup on failure/deadline as well as success. These are future commands, not executed Plan checks.

## 17. Acceptance criteria

1. interaction-14: An explicit exact-turn cancellation has a separately observable acknowledgement, authoritative interrupted completion and possibly later pending cleanup. A stale turn cannot cancel another; takeover/cancel-claim/keep do not interrupt, and process stop remains a different operation. Lifecycle evidence does not equate cycle completion with external task acceptance.

## 18 Out of scope

No task acceptance policy, automatic retries/reconnect/restart, force-implies-interrupt behavior or process-kill fallback for turn cancellation. No separate native-fence spike: complete-broker application accounting and final all32 acceptance remain e03s08. This slice's genuine interrupt evidence does not certify native-input settlement.

## 19 Risks

Provider acknowledgement may be empty while a turn keeps running; require authoritative completion before claiming interruption. Pending cleanup can arrive later and must not be hidden by a completion transition. Exit/degradation can make an outcome permanently unknown; retain uncertainty rather than fabricating success. Live timing varies, so bounded phases report blockers, never silently switch turn targets or kill unrelated processes.

## 20 Evidence and handoff

All tasks remain failing and no checks ran during Plan. Deliver exact-turn/lifecycle/degradation contracts and test additions to e03s08. Future evidence separates deterministic message ordering, real provider interruption, visible native captures and owned process cleanup. First acceptance still requires the complete broker, all32 and the unresolved native application seam; no phase advancement is authorized here.
