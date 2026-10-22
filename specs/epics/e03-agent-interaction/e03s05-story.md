---
story_id: e03s05
title: Managed same-instance Codex launch and prompts
maturity: 3
status: passing
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 8
---

## 1 Identity

Slice e03s05; manifest delta ADDED; dependency e03s02. Implement only after phase confirmation and feature-branch/baseline gates. This is a Plan artifact, not authorization or runtime evidence.

## 2 User story

As a local human or external controller, I can open one managed native Codex execution, discover its authoritative live conversation and deliberately start or steer it through amux while observing that same conversation natively.

## 3 Problem

Existing terminal launch and OMP observation do not provide an owned Codex structured connection. Prior experiments proved selected resumed-thread interactions, not fresh native-first identity discovery. API-first creation followed by resume required rollout history; hiding a setup prompt would violate the selected startup contract.

## 4 Goal

Deliver owned native/frontend/backend lifetime, authoritative fresh identity discovery, read-only inspection and explicit prompt intent through the closed raw API. Preserve stock shell, ordinary open and --omp defaults. A successful call receipt is not turn completion or task acceptance.

## 5 Scope

Create src/codex.rs as the concrete owned connection/registry boundary and wire agent-open, agent-inspect and agent-prompt into src/daemon.rs, src/client.rs and src/cli.rs using existing API framing. Add only the approved WebSocket dependency in Cargo.toml/Cargo.lock. Create tests/agent_interaction.rs and tests/codex_native_smoke.rs; extend existing lifecycle/PTY regressions where launch behavior changes. Implement fresh and explicit persisted resume_thread paths without a shadow instance.

## 6 Actors

The daemon owns execution resources; the local app-server supplies protocol facts; one ordinary native Codex frontend occupies the execution PTY. Multiple amux attachments view that frontend. An external controller supplies explicit prompt intent and current authority, not scheduling policy.

## 7 Preconditions

The e03s01/e03s02 ownership admission, retained calls and recovery exist, including tests/pty_control.rs. Installed Codex must be checked as 0.160.0 and its generated schema reviewed during implementation. Real tests additionally need later explicit live-run approval, valid authentication and explicit model selection; Plan performs no probe or dependency installation.

## 8 Dependencies

Use [OK] tungstenite 0.30.0 with default-features=false and features=["handshake"] on local UnixStream only. Review transitive MSRV/license/size on the actual dependency change; no TLS, new async runtime, SDK, fixture crate or provider abstraction. Existing event sequence/gap/snapshot behavior and the ownership wire contract are prerequisites. e03s06/e03s07 extend this driver, not alternative backends.

## 9 Module purpose callers and contracts

src/codex.rs owns one concrete Codex connection/execution and authoritative identities, requests and receipts. Reason for Depth: callers submit validated same-instance actions instead of coordinating provider IDs, cleanup and acknowledgement/completion themselves. src/daemon.rs calls it for closed agent operations and execution stop; src/runtime.rs supplies native PTY/process lifetime; src/control.rs serializes authority and dispatch. CLI/raw API and native attachments consume snapshots/events. Keep PTY lifetime, instance, session, turn, ownership generation and persisted resume identity distinct. Detaching a viewer is not stopping an execution.

## 10 Requirements

- ADDED: agent-open accepts a controller credential, existing source/project/name/cwd selection conventions, explicit model and optional persisted resume_thread. Reject unknown/incompatible fields; return terminal/instance IDs, initial control and starting state. Bind caller-directed startup to that returned control and retain startup dispatch accounting. Do not use the unrelated automatic-launch controller as a later input bypass.
- ADDED: Start a private app-server/socket and ordinary native frontend sharing that owned server. For a fresh session, let the native frontend create the conversation and discover it through authoritative protocol events. Until discovery, session identity is explicitly unavailable and mutations fail without forwarding. No screen-derived identity, fabricated bootstrap prompt/turn, separate headless conversation or upstream patch.
- ADDED: agent-inspect takes terminal plus expected instance_id and needs no input control. Return current target, capability/lifecycle dimensions, exact pending identities when known, recent action outcomes and existing event cursor. Gaps/degradation/unknown states require snapshot recovery, not replay.
- ADDED: agent-prompt takes target={terminal,instance_id,session_id}, current control, intent=start|steer and text; steer additionally requires expected_turn_id. Bind start to turn/start and steer to turn/steer with expectedTurnId. Reject active-turn conflicts, stale steer and stale target; never silently queue, start another turn or retarget.
- ADDED: Register actual forwarding at the same per-PTY ordering point as revocation and recheck queued authority before provider I/O. Retain queued/forwarded/applied/rejected/unknown outcomes; a socket write is not application. Force/cancel/recovery never erase these records. No state lock is held across provider I/O.
- ADDED: Expose frontend/backend exit separately, mark uncertain calls unknown and clean up only owned resources on explicit stop/PTY close. Native attach/detach and external socket closure do not create another instance, implicitly surrender control or cancel turns. Runtime state and credentials never become persisted runtime restoration.

## 11 Interfaces and data

PLAN quotes the primary app-server transport: Unix transport uses "WebSocket HTTP Upgrade, not amux newline JSON"; "initialize once and send initialized before other requests." The amux client-facing transport remains bounded v1 newline JSON. At implementation use `codex app-server generate-json-schema --out PATH` in a private temporary directory, bind exact installed 0.160.0 shapes, then remove generated data. tungstenite exposes client(request, stream) over the caller-provided stream. These verified details establish the transport choice, not proof of fresh startup. Public snapshots/events contain IDs and outcomes, never controller/control/claim/offer references, credentials or provider connection secrets. Private return values retain capabilities through ordinary one-shot socket closure only, not daemon restart.

## 12 Main flow

Register an external controller and call agent-open with explicit model. Allocate the private backend/frontend execution once, return starting identity and bound control, attach actual amux native viewers to its PTY, and discover the native-created session authoritatively. Inspect the live target; send start, observe accepted activity and continuation/completion, then deliberately steer an exact active turn when supported. Recover uncertain receipts by inspection rather than repeating prompts.

## 13 Alternate and error flows

Reject malformed intent, missing control, wrong-kind/stale capabilities, ended PTY, replaced instance/session, undiscovered session and stale expected turn before forwarding. Fail unsupported/missing protocol prerequisites visibly. Backend/frontend disconnect is degradation, not successful completion or automatic restart. A lost reply can follow actual dispatch: retain its identity/outcome and report unknown until authoritative evidence arrives. Event gaps resynchronize existing snapshots. Late old-instance events cannot rewrite a replacement target. Explicit resume may use a real persisted thread but cannot stand in for the fresh-path test.

## 14 Nonfunctional constraints

Do not hold topology/ownership/event locks across handshake, reads or model I/O, or stall PTY output behind observers. Reuse the existing event service and cached control; no generic RPC framework or per-key registration. Opt-in smoke must enforce an overall watchdog <=1800 seconds, daemon/backend/model-response deadlines <=15/30/180 seconds and <=6 meaningful model turns, counting every actual turn. Use mode0700 private HOME/config/runtime/CODEX_HOME/workspace/socket roots; privately copy auth only if needed. Restore native modes; on success/failure/SIGINT/deadline terminate and reap only owned process trees, verify groups/socket/root removal, remove copied credentials/schemas/rollouts/markers, retain redacted evidence. Never change global Codex state or install/upgrade its binary.

## 15 Implementation steps

1. Add the approved dependency and src/codex.rs owned WebSocket/native launch/discovery path; create tests/agent_interaction.rs covering handshake, fresh-not-yet-discovered state, authoritative discovery and owned cleanup -> verify: `cargo test --test agent_interaction`.
2. Bind agent-open/agent-inspect/agent-prompt through daemon/client/CLI, initial authority and exact start/steer identity; expand behavioral protocol tests for dispatch/revocation and uncertain receipt recovery -> verify: `cargo test --test agent_interaction && cargo test --test pty_control`.
3. Create tests/codex_native_smoke.rs with ignored exact root real_native_first_start and its opt-in/budget/private-resource guards; update specs/adr/0003-codex-first-agent-target.md and specs/contracts/pty-control.md implementation-facing operation/evidence notes, without declaring acceptance -> verify: `cargo test --test codex_native_smoke --no-run && cargo test --test agent_interaction`.
4. After separate explicit live-run approval, prove native-first discovery with zero hidden setup turns, same-instance start/steer observations, real attachments and detach/stop cleanup; record redacted evidence -> verify: `test "${AMUX_RUN_REAL_CODEX:-}" = 1 && test -n "${AMUX_CODEX_MODEL:-}" && cargo test --test codex_native_smoke real_native_first_start -- --ignored --exact --nocapture --test-threads=1`.

## 16 Verification script

The implementation creates both future targets in steps 1/3; they do not exist at Plan time. Deterministic fixtures assert received/sent installed-schema messages and observable daemon outcomes, including stale identity/turn conflicts, delayed acknowledgements, control revocation before queue consumption, event gaps, process failure and receipt recovery; they do not prove actual Codex startup or native application. Compile smoke without running it in ordinary development. Only after explicit approval, set AMUX_RUN_REAL_CODEX=1 and a nonempty AMUX_CODEX_MODEL, then select real_native_first_start exactly. The selected test must fail, not skip or replace itself with fixtures, if version/auth/model/native-PTY prerequisites are missing. Exercise real controlling-PTY amux attachment input and rendered state; retain visible captures separately. Assert frontend/API session equality, no fake bootstrap, start versus exact-turn steer and process cleanup under all exit paths. Commands above are future implementation checks, not executed Plan evidence.

## 17. Acceptance criteria

1. interaction-01: A fresh native-first launch and structured inspection/prompt operate on one authoritative instance/session; adding a viewer never launches another agent. Evidence includes actual native captures and protocol identities, not fixture-only claims.
2. interaction-02: Explicit start and expected-turn steer reach that live instance with authoritative activity/continuation/completion visible; conflicting/stale intent is rejected without silent queuing or a new turn.
3. interaction-13: Unknown startup, dropped control/report connection, event gaps and either process exit produce honest unavailable/unknown/resynchronizing states and retained receipts, with no replay or fabricated success.

## 18 Out of scope

Structured question/permission responses and interrupt belong to subsequent slices. No additional provider, relay, remote service, autonomous retry/reacquisition policy, restoration or CLI porcelain. The final native-buffer/application fence remains deferred to e03s08; this slice cannot claim complete safe-native handoff or first acceptance.

## 19 Risks

Fresh native-first discovery remains unproved: missing supported authoritative discovery blocks the actual path, not a reason to add a setup model turn. Installed schema can differ from current docs; bind the pinned generated schema before implementation. Provider acknowledgement may precede application; unknown outcomes must block safe grant. Live-model nondeterminism is bounded and reported, never papered over with fixture evidence.

## 20 Evidence and handoff

All ledger entries remain failing. No source, dependency, test, live model or process changes were performed during Plan. e03s05 creates the agent_interaction and codex_native_smoke targets for later stories; e03s06 inherits exact identities and retained outcomes. Future evidence must separate deterministic fixtures, real Codex smoke, native surface captures and unproved seams. First acceptance is reserved for e03s08/all32, and requires explicit phase approval before Build.
