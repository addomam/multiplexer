---
type: feat
context: specs/planning-context.yaml
---

# e03 — Same-instance agent interaction and exclusive PTY control

## Authorization and scope

The user confirmed the locked Elaborate design and explicitly authorized Plan. This capsule is a plan, not implemented behavior or permission to enter Build. The source of truth is [planning-context](../../planning-context.yaml), [scope](../../product/SCOPE_LATEST.yaml), [ownership wire contract](../../contracts/pty-control.md), [native UX](../../tech-architecture/pty-control-native-ux.md), and ADRs [0001](../../adr/0001-cooperative-pty-ownership.md), [0002](../../adr/0002-capability-control-interface.md), [0003](../../adr/0003-codex-first-agent-target.md). All 32 interaction scenarios remain required. Prior implementation review history is not reset.

## Delivery order and independently observable value

| Story | BCP estimate | Delivered slice | Dependencies |
|---|---:|---|---|
| e03s01 | 8 | Two real local controllers acquire, yield and accept one PTY; native input uses the same authority; stale input cannot land | None |
| e03s02 | 8 | A user explicitly forces or cancels a stalled handoff and recovers an uncertain one-shot result without cancelling work | e03s01 |
| e03s03 | 5 | Attached humans see lock ownership and use deliberate request/wait/cancel/force controls across all native input routes | e03s02 |
| e03s04 | 8 | An external controller requests idle/background reacquisition; visible keep-control and explicit hand-back preserve user choice | e03s03 |
| e03s05 | 8 | A managed local Codex frontend and structured connection share one live native-first instance and accept explicit prompt intent | e03s02 |
| e03s06 | 8 | A real question is answered and a real permission is allowed/denied structurally without stale-request races | e03s05 |
| e03s07 | 5 | A user explicitly interrupts the exact turn and sees truthful lifecycle/degradation without killing unrelated work | e03s06 |
| e03s08 | 8 | The complete broker safely exchanges native/API control with actual application accounting and authoritative presentation | e03s04, e03s07 |

BCPs are planning estimates, not durations. Total 58. WSJF inputs are estimated business value 8, time criticality 3, risk reduction 13, job size 58: 24/58 = 0.414 rounded. e03 is the only pending epic; e01/e02 are retained historical entries, not competing pending work. Execute stories in listed order under build-epic; dependency edges describe true prerequisites, not permission to edit shared runtime files concurrently.

Each slice implements its supported behavior fully through real consumers. No stub methods, success-shaped unsupported operations, legacy naked-input shims or false safe grants. Adding later closed operations does not make an earlier slice the first complete-feature acceptance. No e03 acceptance or production support claim until e03s08 and every criterion pass.

## Zoom-out and impact

[Impact assessment](../../IMPACT_LATEST.md) identifies actual callers and regressions. Existing responsibilities must stay separate:

- `daemon.rs`: one-shot/persistent API admission, topology/events and execution lifetime. Add connection provenance through `serve`/`ClientSlot`/dispatch, per-PTY ordering and public ownership snapshots; never wait for harness/hook/observer while holding global topology or ownership locks.
- `runtime.rs` and `runtime/compatibility.rs`: native PTY, emulator, bounded writer and process cleanup. Generation-aware queue consumption must reject undispatched revoked work. Writer `write_all` acknowledgement is delivery only. Genuine terminal query replies stay daemon-classified progress, not human input.
- `integration.rs` and `integrations/zsh/adapter.zsh`: stock-shell readiness/FIFO/native acceptance. Retain originating authority across epoch/wake and report replacement; settlement reports may finish old work, but cannot dispatch new revoked input. Preserve busy/partial/readiness and immediate-versus-queued deadlock protection.
- `client.rs`, `cli.rs`: persistent/one-shot framing and typed error codes. Keep raw `amux api`; add a required serialized `--control` argument to existing input/execute commands, rather than silently acquiring another owner's lock. Migrate their docs/examples/test producers. Keep nested view nulls intact.
- `tui.rs`/`tui/view.rs`: one native surface, three-row header and no footer/body geometry change. One native controller per original connection, cached authority, unknown initial view until reported, and recoverable stale-authority errors without replay.
- `projects.rs` and the existing OMP reporter: preserve runtime/disk separation and observation behavior. No controller/broker persistence or new orchestrator policy.

### Only new deep boundaries

1. `src/control.rs`: closed capability/claim/offer state and retained dispatch accounting. **Reason for Depth:** callers must not independently reconstruct generation, reservation, revocation and settlement decisions.
2. `src/codex.rs`: one concrete owned Codex connection/execution and authoritative request registry. **Reason for Depth:** callers should submit validated same-instance actions, not coordinate provider IDs, resource cleanup and acknowledgement-versus-completion themselves. No provider trait/registry or generic method passthrough.
3. `src/tui/control.rs`: native ownership input decisions and presentation state. **Reason for Depth:** key, paste, literal-prefix, application mouse and local-activity paths must not invent different authority rules.

Keep shared contract types with those modules, not a generic RPC/object framework. Production time uses a monotonic clock; deterministic transition tests supply explicit instants rather than a new clock abstraction. Borrow cached control on hot input and use batch/watermark accounting where valid, not per-key capability registration or needless copies.

## Ordinary authority cutover

Implement the exact existing wire schema and its role checks, including ControllerHandle registration/recovery, ControlHandle-only input, non-authorizing Claim/Offer handles, generation plus reservation compare receipts, and public events without private capabilities. Claim read is never accept. Native attachment disconnect marks view departure, not release/cancel; external socket loss has no ownership effect.

Public input/execute require control with no legacy bypass. Migrate CLI/TUI (including separate paste), root acceptance and shell-race helpers, and `terminal-showcase/tests/multiplexer.rs` one-shot execute producers together. Preserve the independent showcase's no-amux-import oracle. Direct low-level PTY/integration tests remain transport/shell tests, not fabricated native controller proof; migrate signatures if changed. Before changing exported symbols, refresh LSP references.

Automatic command/recipe/OMP startup also needs bound authority and retained accounting. For an existing launch without a caller controller, use one private, nonpersisted external-role launch controller bound to the new PTY; it does not authorize arbitrary later reports. Retire that launch authority through normal release once authoritative source acceptance/dispatch settles, never from a socket write or fixed sleep. Caller-directed managed Codex creation binds startup to its returned initial control instead. Neither path introduces reacquisition/retry policy. Unknown prior work blocks safe transfer and remains visible; it is never erased by cancellation, release or force.

The first slice proves cooperative transfer on a genuinely settled path, including an empty forwarded ledger. It does not pretend a raw PTY write certifies native application. e03s08 completes the native/harness application seam under the complete broker. Safe transfer remains unavailable wherever required settlement is genuinely unknown; force is explicit and visibly risky. This is an intermediate safety invariant, not permission to accept a force-only final feature.

## Planned structured-agent API binding

These are new closed v1 raw API operations to implement in e03s05–07, not existing commands. Preserve ordinary shell `open`/`--omp` defaults. Do not expose arbitrary provider methods.

- `agent-open`: controller credential, existing `source`/`project`/`name`/`cwd` selection conventions, explicit `model`, optional persisted `resume_thread`. Create one managed PTY/frontend plus private backend; return live terminal/instance IDs, initial control and starting state. Reject incompatible/unknown fields. No hidden prompt or bootstrap turn. Thread/session identity remains explicitly unavailable until authoritative native-first discovery; later mutations require the discovered live target.
- `agent-inspect`: live terminal plus expected `instance_id`; return current target, capability/lifecycle dimensions, exact pending requests, recent action outcomes and event cursor. No control required; unknown/degraded/gap states are explicit. Reuse existing sequenced events/snapshot recovery rather than another event service.
- `agent-prompt`: `target` = terminal/instance_id/session_id, current `control`, `intent: start|steer`, text; steer requires `expected_turn_id`. Start binds `turn/start`, steer binds `turn/steer` with `expectedTurnId`; never stale-steer fallback, silent queue or implicit new turn.
- `agent-answer`: target, control, exact pending turn/item/provider request identity and answers keyed by question IDs. Preserve provider request IDs as string or integer. Validate current question semantics and produce the installed response shape.
- `agent-permission`: same pending identity and control, explicit `decision: allow|deny` for this request only. No session-wide/inferred approval.
- `agent-interrupt`: target, control and exact `turn_id`; bind `turn/interrupt`. Acknowledgement, interrupted completion, later pending-request cleanup and process stop remain distinct.

Each mutation returns an inspectable call receipt, not task success: distinguish queued, forwarded, applied, rejected and unknown. Register actual dispatch under the same ordering point as revocation; recheck queued authority before forwarding. Do not hold state locks across provider I/O. Preserve late outcomes and expose unresolved calls through ownership state. A resolved question alone cannot settle unrelated native input. No blind retry after socket loss; inspect authoritative retained outcomes.

### Verified external details and dependency slopcheck

Primary [Codex app-server documentation](https://developers.openai.com/codex/app-server) states Unix transport uses WebSocket HTTP Upgrade, not amux newline JSON; initialize once and send `initialized` before other requests. Version-specific schema generation is `codex app-server generate-json-schema --out PATH`. At implementation pin/check installed 0.160.0 and generate into a private temporary directory to bind exact request/approval/response shapes; remove generated temporary data afterward. Current docs can differ from the installed version and are not a substitute for that check.

Recorded question response: `{"id":0,"result":{"answers":{"color":{"answers":["Blue"]}}}}`; `serverRequest/resolved` carries `threadId`/`requestId`. These are real spike facts, not a complete startup transcript. Fresh native-first discovery and additional choice/permission permutations remain implementation checks. No upstream patch, shadow thread, screen-derived identity or fake setup model prompt.

**[OK] adopt tungstenite 0.30.0**, with `default-features = false`, `features = ["handshake"]`, for the local UnixStream client only. `cargo info tungstenite@0.30.0` actually reported MIT/Apache-2.0, Rust 1.85 (compatible with this package's 1.86), and the handshake feature. The [primary API](https://docs.rs/tungstenite/0.30.0/tungstenite/) exposes `client(request, stream)` over a caller-provided stream; cached crate source also directs custom streams to `client`. No new TLS, async runtime, fixture crate, SDK, generic RPC framework or provider registry. `opensrc` was unavailable; Cargo's fetched source and primary docs supplied the source check. Review transitive MSRV/license/size during the actual dependency change; no dependency was added in Plan. No SUS/SLOP package is proposed.

## Future verification entrypoints and their creators

| Entry point | Created by | Purpose |
|---|---|---|
| `tests/pty_control.rs` | e03s01 | Private real-daemon ownership, queue/continuation, recovery and native-attachment regression cases; expanded through e03s04/e03s08 |
| `tests/agent_interaction.rs` | e03s05 | Deterministic installed-schema protocol fixtures and identity/lifecycle/error races, expanded through e03s08 |
| `tests/codex_native_smoke.rs` | e03s05 | Explicitly selected, opt-in real Codex/native smoke; never silently run with the normal suite |

Future targets above do not exist yet and were NOT run during Plan. Task commands become executable as the responsible implementation task creates the target; a missing target is failure, not a fabricated passing check. No count-only/source-text/mock-echo assertions. Tests cover plausible user-visible stale input, competing transitions, lost replies, timer/view boundaries, process cleanup and errors. Preserve all existing regressions with valid authority so they still reach their original busy/partial/latency boundary.

Real-smoke exact root test names: `real_native_first_start` (e03s05), `real_questions_and_permissions` (e03s06), `real_explicit_interrupt` (e03s07), `real_complete_broker_handoff` (e03s08). Each is ignored by default and requires BOTH explicit exact test selection and `AMUX_RUN_REAL_CODEX=1`, plus explicit nonempty `AMUX_CODEX_MODEL`. Missing version/auth/model/prerequisites must fail the selected test visibly, never skip/substitute a fixture. New model calls need explicit live-run approval; the earlier spike authorizations do not apply to Plan.

Each opt-in run: overall watchdog <=1800 seconds, per-phase deadlines (daemon15s/backend30s/model response180s), at most six meaningful model turns. Bounds stop the experiment and report blockers; they never automatically grant a lock or weaken settlement. Account for every actual turn; fresh startup cannot smuggle a bootstrap turn. Use private HOME/config/runtime/CODEX_HOME/workspace/socket, mode0700 roots and private auth copy only if needed. Restore native terminal modes; on success/failure/interrupt/deadline terminate and reap only owned daemon/frontend/backend trees, verify process groups and socket/root removal, remove copied credentials/schemas/rollouts/markers, retain only redacted evidence. Never touch the global Codex daemon/session/config or install/upgrade its binary.

## Verification and first-acceptance gate

- Existing regressions: `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build --workspace`; compile/typecheck via `cargo check --workspace --all-targets`.
- Explicit independent compatibility: `cargo build --workspace && cargo test -p terminal-showcase --test multiplexer independent_showcase_runs_inside_amux_and_checks_real_cursor_replies -- --ignored --exact --nocapture`.
- Native surface proof must launch actual amux attachments on controlling PTYs, exercise key/paste/mouse/modal actions, inspect rendered ownership state and target effects, and record visible captures separately. Unit event injection alone is not UI proof.
- Final complete-broker smoke must exercise all 32 scenarios, real questions and both permission decisions, same-thread prompt/interrupt, API-to-native and native-to-API safe transfer, retained calls across force/cancel/recovery, multi-attachment ownership, idle/background keep and actual detach/exit cleanup. Cover the prior losing-answer presentation and buffered-native-input case here, not with an early isolated-broker detour.
- If a supported native application fence or authoritative presentation cannot be established, keep explicit unknown/settlement-unavailable and BLOCK first acceptance. Report the exact missing seam; do not rename force-only behavior safe, weaken a criterion, patch upstream or silently pick another harness.

Tasks stay `failing` until their own verify command actually exits0 in Build/Verify and evidence is recorded. Whole-target deterministic commands avoid empty named filters. Each selected real/showcase command must report exactly one executed test; zero selected tests blocks the task even if Cargo exits0. Current Plan validation checks artifact consistency and command syntax only; it cannot pre-mark runtime success. Before Build: Plan dual quality >=94/no must-fix, explicit phase confirmation, then feature-branch isolation and baseline verification. No commit/merge/push/release action is included.
