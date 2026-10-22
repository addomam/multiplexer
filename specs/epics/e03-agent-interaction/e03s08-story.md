---
story_id: e03s08
title: Complete-broker native and API handoff acceptance
maturity: 3
status: failing
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 8
---

## 1 Identity

Slice e03s08; manifest delta MODIFIED; dependencies e03s04 and e03s07. Direct manifest acceptance is interaction-10; final_acceptance is all-32-interaction-scenarios. This is the first complete-feature acceptance gate, not permission for Build or a Plan-time runtime PASS.

## 2 User story

As a human sharing one native agent with an external controller, I can safely exchange input authority without old native/API input unexpectedly applying afterward, see the authoritative answer and lifecycle, and explicitly choose risky non-cancelling force when settlement is unknown.

## 3 Problem

API-only accounting previously allowed native Enter buffered in a stopped TUI to apply after a false safe grant. A PTY write proves delivery, not application; unrelated question resolution does not settle that native queue. A separate race also displayed the losing native answer as if accepted. Partial broker tests do not close either acceptance gap.

## 4 Goal

Complete actual native/API application accounting and authoritative presentation under the integrated broker, then prove every one of the 32 retained interaction scenarios on actual amux. If supported authoritative settlement or truthful presentation cannot be established, expose unknown/settlement-unavailable and block first acceptance, not silently weaken safe transfer.

## 5 Scope

Integrate src/control.rs, src/codex.rs, src/daemon.rs, src/runtime.rs, src/runtime/compatibility.rs, src/integration.rs, integrations/zsh/adapter.zsh, src/tui.rs and src/tui/control.rs at the actual delivery/application boundaries. Expand tests/pty_control.rs from e03s01 and tests/agent_interaction.rs plus tests/codex_native_smoke.rs from e03s05. Preserve CLI/client and independent terminal-showcase authority cutover. Update existing normative/evidence documentation only when implementation establishes the facts. No early new spike, alternate harness, upstream patch or force-only final acceptance.

## 6 Actors

The daemon arbitrates per-PTY ownership and retains actual dispatch accounting. Codex and stock-shell integration provide authoritative outcomes for their own input paths. Native humans and external controllers use the same capability lifecycle. Multiple attachments observe one native frontend. External policy alone decides reacquisition, retries, prompts, answers, approvals and task acceptance.

## 7 Preconditions

e03s01–e03s07 supported slices exist, including real consumers and migrated ordinary input/execute producers. Tests/pty_control.rs, tests/agent_interaction.rs and tests/codex_native_smoke.rs are actual creator prerequisites, not existing Plan-time targets. Enter Build only after explicit phase approval, feature-branch isolation/baseline and parent quality gates; inherited review history is not reset. Real final smoke requires separate explicit live-run approval, pinned Codex version/auth/native-PTY prerequisites and explicit model opt-in.

## 8 Dependencies

Consume the locked ownership contract, native UX, ADR0003 and PLAN, preserving all prior slice invariants. Reuse [OK] tungstenite 0.30.0 handshake-only; introduce no additional dependency, generic provider/RPC framework, clock abstraction or orchestrator policy. Production timing remains monotonic; deterministic tests supply explicit instants. Refresh LSP references before changing exported symbols, then migrate every affected caller without compatibility shims.

## 9 Module purpose callers and contracts

src/control.rs owns generation/reservation/revocation and retained dispatch/settlement; its daemon, runtime, shell continuation and Codex callers must not reconstruct these decisions independently. src/codex.rs owns authoritative same-instance protocol requests and outcomes. Runtime owns bounded native writer/emulator/process cleanup; integration owns source acceptance, epochs/wakes and shell readiness, not task acceptance. TUI control owns shared key/paste/mouse/literal-prefix/local-activity decisions and existing-header presentation. Public snapshots/events expose IDs, actual blockers and compare receipts, never private handles. Each attachment's activity/view facts remain bound to its original native connection; no source label or copied controller credential creates human provenance.

## 10 Requirements

### MODIFIED: Complete native/application settlement

**Before:** Earlier ownership slices reject revoked queued work and retain forwarded work, but safe transfer is available only where authoritative settlement is actually known; raw PTY writes do not prove native application and the deferred buffered-native seam is not accepted.

**After:** Complete native input/application accounting using supported authoritative application/rejection evidence, retaining stream/batch watermarks where valid instead of per-key heap records. Safe grant requires explicit yield, reserved-requester settlement and accept with no unaccounted prior-owner mutation still able to land. A PTY write/write_all, silence, fixed delay, whole-turn finish or unrelated question resolution is not the native fence. Unknown required settlement visibly blocks safe grant and first acceptance. Force remains explicit, risky and non-cancelling, never renamed safe.

### MODIFIED: Authoritative visible outcomes

**Before:** The recorded uncontrolled native/API race could show a losing native answer as accepted; earlier slices retain exact provider outcomes but have not established complete-broker presentation acceptance.

**After:** Actual amux/native views and structured inspection truthfully reflect the authoritative winning request/answer and unresolved states under complete ownership. Retain the ordinary native UI and geometry; do not forge a result, screen-scrape identity or hide contradictory accepted-answer presentation behind a successful API receipt. If supported interfaces cannot establish truthful presentation, disclose the exact missing seam and block acceptance.

### MODIFIED: Complete cutover and compatibility

**Before:** e03s01–07 introduce ordinary authority and closed agent operations incrementally; final same-instance/native coexistence and all32 integration evidence are not established.

**After:** Every public input/execute/agent mutation, separate paste, runtime queue consumption, shell epoch/wake continuation, automatic command/recipe/OMP startup and independent showcase producer uses bound authority and retained dispatch accounting. Reject undispatched revoked input while permitting old settlement/lifecycle reports and genuine daemon-classified terminal replies. No naked-input alias, automatic acquisition shim or client protocol-label bypass remains. Preserve original busy/partial/readiness/latency and cursor-reply regressions.

### ADDED: Final broker invariants and evidence

The integrated broker must retain: request keeps old control until yield; settlement plus reservation plus explicit accept for safe grant; ordinary cancel revokes old authority and grants nobody; conditional keep/withdraw retains the same human capability and never rearms; force never interrupts; external policy alone requests idle/background reacquisition; original-connection native provenance; no input replay; private handles stay private; no per-key registration. Raw API exit0 is request success, not ownership. Timeout/socket loss never implicitly cancels or releases. All32, real questions and both permission decisions, exact-turn interrupt, native/API bidirectional safe transfer, multiple attachments and owned cleanup are mandatory final acceptance.

## 11 Interfaces and data

Use existing PublicOwnership and retained call identities, not a new event service. Keep native delivered versus authoritatively applied/rejected/unknown watermarks distinct; late outcomes update only their original record, including after force/cancel/fresh acquisition. Shell source acceptance and command completion are distinct. PLAN records `serverRequest/resolved` with threadId/requestId and the example answer `{"id":0,"result":{"answers":{"color":{"answers":["Blue"]}}}}`; these settle that specific question, not other native bytes. Unix provider transport remains WebSocket HTTP Upgrade, initialized exactly once before other requests, while public amux remains v1 newline JSON. Installed 0.160.0 schema generated privately via `codex app-server generate-json-schema --out PATH` governs exact fields and must be removed afterward. No new guessed native fence operation is prescribed: implementation must establish the supported authoritative seam under this complete broker or block acceptance.

## 12 Main flow

Run one owned native-first Codex instance and inspect its discovered target. Use the external controller to start/steer, answer a genuine question and explicitly allow/deny permissions. Request native control while preserving old authority, yield, account for actual forwarded API/native input, retain the requester reservation and explicitly accept a safe offer. Deliberately enter native input, then hand back under the same safe protocol to API control. Observe ongoing turns independently; perform exact-turn interruption only by explicit operation. Exercise idle/background/keep/hand-back through external requests and multiple native views, then detach without stopping and explicitly close the execution to prove owned cleanup.

## 13 Alternate and error flows

At final complete-broker validation revisit the stopped/buffered-native case: hold native input in the frontend application boundary, request transfer, and prove no safe grant while prior input can still land; release the hold and verify actual authoritative settlement before any safe acceptance. If no supported fence can establish this, keep unknown/settlement-unavailable and block acceptance. Exercise force as an explicitly warned alternative while retaining unresolved records, not as a passing replacement for safe transfer. Ordinary cancel leaves unowned with old calls retained; a different controller still requires settlement or force. Old source reports may settle old work but never launch revoked continuation input. Competing accept/cancel/force and late old-turn/request outcomes cannot revoke a newer owner. Gaps/lost replies use snapshots/private recovery without replay. Missing mandatory question/approval/native-presentation interfaces remain blockers, not optional support labels.

## 14 Nonfunctional constraints

Use the per-PTY ordering point for authority, dispatch and relevant activity/view changes; never hold global/ownership/writer locks while waiting for provider/hook/observer. Borrow cached control on hot input; use bounded queues and native batch/watermark accounting where valid. Preserve stock Zsh, independent cursor replies, native body geometry and responsiveness. Real smoke enforces <=1800s overall watchdog, daemon/backend/model-response deadlines <=15/30/180s and <=6 meaningful model turns. Fit all real final scenarios into that explicit budget; share genuine turns where appropriate, never fabricate a bootstrap or approval. Budget exhaustion reports blockers rather than extra automatic runs or acceptance. Use private mode0700 HOME/config/runtime/CODEX_HOME/workspace/socket roots and necessary private auth copy only. On success/failure/SIGINT/deadline restore terminal modes, terminate/reap only owned daemon/frontend/backend groups, verify sockets/roots removed and delete credentials/schemas/rollouts/markers. Retain redacted evidence only; never modify global Codex daemon/session/config or install/upgrade its binary.

## 15 Implementation steps

1. Complete actual native/API/shell dispatch and application accounting across src/control.rs, src/codex.rs, src/runtime.rs, src/runtime/compatibility.rs, src/integration.rs and integrations/zsh/adapter.zsh; expand existing real-daemon/protocol targets for revoked queues, continuation authority and retained late outcomes -> verify: `cargo test --test pty_control && cargo test --test agent_interaction`.
2. Complete src/tui.rs and src/tui/control.rs authoritative visible state and consumer cutover with src/daemon.rs/client.rs/cli.rs and terminal-showcase/tests/multiplexer.rs; add actual controlling-PTY multi-attachment/routing/conditional/recovery regressions and losing-answer checks -> verify: `cargo test --test pty_control && cargo test --test agent_interaction`.
3. Expand tests/codex_native_smoke.rs with ignored exact root real_complete_broker_handoff and all32 evidence assertions, including the final user-deferred buffered-native case, real winning-answer presentation, bidirectional safe transfer, questions/allow/deny, interrupt and cleanup; update specs/contracts/pty-control.md, specs/tech-architecture/pty-control-native-ux.md and specs/adr/0003-codex-first-agent-target.md with actual supported behavior and explicit remaining blockers -> verify: `cargo test --test codex_native_smoke --no-run && cargo test --test pty_control && cargo test --test agent_interaction`.
4. After separate explicit live-run approval, run exact final smoke and full compatibility checks, requiring all32 evidence and native captures before first acceptance -> verify: `test "${AMUX_RUN_REAL_CODEX:-}" = 1 && test -n "${AMUX_CODEX_MODEL:-}" && cargo test --test codex_native_smoke real_complete_broker_handoff -- --ignored --exact --nocapture --test-threads=1 && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo check --workspace --all-targets && cargo build --workspace && cargo test -p terminal-showcase --test multiplexer independent_showcase_runs_inside_amux_and_checks_real_cursor_replies -- --ignored --exact --nocapture`.

## 16 Verification script

Targets already exist from e03s01/e03s05 and are expanded, not replaced. Deterministic real-daemon and installed-schema protocol tests cover dispatch-versus-revocation, buffered writer consumption, old shell epoch/wake continuations, delayed provider outcomes after force/cancel/recovery, all conditional activity/view/consent boundaries, stale transfers, wrong-kind handles and uncertain one-shot results. These tests cannot alone certify actual Codex/native application or truthful native presentation.

The final ignored root real_complete_broker_handoff is created in step 3. Select only that exact test after explicit live approval with AMUX_RUN_REAL_CODEX=1 and nonempty AMUX_CODEX_MODEL. The executable checks those guards and pinned version/auth/native prerequisites; missing prerequisites fail visibly without fixture substitution. Its internal watchdog, phase deadlines, turn counter and owned-resource cleanup enforce §14. It launches actual amux attachments on controlling PTYs, exercises keys/paste/literal-prefix/application mouse/modal controls and inspects target effects plus rendered ownership/request state. Keep visible captures separate from event assertions. Include two native attachments and observer-origin provenance attacks, warning keep activity before/after consent, background owner/all/unknown views, delayed accept/cancel/force and socket loss/private recovery. Cover real questions and both real permission decisions, exact-turn interrupt, unchanged running work during force, and API-to-native plus native-to-API genuinely safe transfer. Revisit buffered native Enter and prior losing-answer presentation only here under the complete broker; do not reopen an isolated spike or substitute a force path.

Map each actual assertion/capture/outcome to all32 below. Run the workspace and explicit independent showcase commands in step 4 after all changes land, preserving the showcase's independent no-amux-import oracle. Any missing native fence, contradictory authoritative presentation, missing real mandatory interface, budget overrun or absent scenario evidence blocks first acceptance. Ordinary cargo test never silently runs ignored model probes; no blanket --ignored run. No command here ran during Plan.

## 17. Acceptance criteria

1. interaction-01: Native frontend and structured API demonstrably share one live instance/session, with no shadow agent or UI replacement.
2. interaction-02: Explicit API prompt start/steer is accepted on that instance and authoritative activity/continuation/completion is observable.
3. interaction-03: A genuine question is inspectable and answerable structurally with supported choice/free-text semantics.
4. interaction-04: Genuine permission requests can be inspected and explicitly allowed and denied through the structured API.
5. interaction-05: Native/API responses share the same authoritative pending identity; only the first valid response applies and visible presentation does not advertise a losing answer as accepted.
6. interaction-06: Late, duplicate, cancelled, resolved, timed-out and previous-session responses cannot target another request.
7. interaction-07: Only the owning controller/attachment injects input; observation, genuine native protocol progress and explicit handoff remain available.
8. interaction-08: Safe transfer requires owner yield, authoritative settlement and reserved requester acceptance; notification/offer alone grants no control.
9. interaction-09: Takeover leaves the already-running turn and process alive unless a separate explicit operation cancels/stops them.
10. interaction-10: Relinquish/force/cancel reject undispatched revoked work and retain forwarded native/API work. Safe grant certifies actual application settlement, including the stopped/buffered-native counterexample; neither PTY write nor unrelated question resolution suffices. Unknown blocks safe grant/acceptance, while force explicitly discloses retained risk.
11. interaction-11: Human control persists indefinitely without an eligible external reacquisition request or explicit hand-back; amux never invents relocking policy.
12. interaction-12: Slow/absent/failed hooks cannot delay force/cancel or PTY processing; cooperation may intentionally await its owner.
13. interaction-13: Report/control loss, process exit and event gaps expose unknown/unavailable/resynchronization rather than success or replay.
14. interaction-14: Exact-turn interruption acknowledgement, interrupted completion, pending cleanup, cycle completion and process stop remain distinct from task acceptance.
15. interaction-15: Missing mandatory harness/native settlement/presentation interfaces block first acceptance rather than being relabelled unsupported or replaced by fixtures.
16. interaction-16: Actual native chrome shows lock icon plus distinct border; request/intercepted-input flows consume opening and confirming input without forwarding or replay.
17. interaction-17: After three seconds pending handoff shows real owner/request blockers; wait/cancel/force stay usable and no timer grants control.
18. interaction-18: Explicit force immediately grants input with unresolved-call warnings, preserves the turn/process and rejects revoked-token automation.
19. interaction-19: Explicit external idle request displays its configured warning countdown after human inactivity; agent output does not reset it.
20. interaction-20: Warning keep/input aborts that attempt, retains the same human capability and never rearms/retries autonomously.
21. interaction-21: Backgrounding transfers only for an eligible explicit request with known scoped views; absent/unknown request facts preserve human control.
22. interaction-22: Reacquisition atomically rechecks current ownership/activity/views/consent/settlement and returns fresh authority; stale facts cannot take a newer generation.
23. interaction-23: Explicit hand-back needs no idle delay and no external request leaves human ownership intact indefinitely.
24. interaction-24: A second actual native attachment requests the same shared lock; the owner sees its request and observers cannot inject keys, paste or application mouse input.
25. interaction-25: Request alone preserves old authority; yield/force/ordinary cancel revoke it and reject later undispatched work.
26. interaction-26: Ordinary pre-grant cancellation grants nobody, revokes old control, retains outstanding calls and permits fresh acquisition without restoring the old orchestrator.
27. interaction-27: Obsolete yield/accept/cancel/force messages cannot revive a transfer or invalidate newer ownership.
28. interaction-28: Reserved requester cannot lose the yield/accept gap to opportunistic acquisition; cancelled reservation no longer blocks fresh acquisition.
29. interaction-29: Claim/offer/controller references never authorize input; only correctly bound current ControlHandle does, without public private-handle leakage.
30. interaction-30: Claim reads/offers do not acquire control; exact explicit acceptance atomically returns fresh control or a non-granted/stale result.
31. interaction-31: Wait timeout/socket closure never cancels a claim or releases a committed grant; authoritative private inspection recovers state without acquisition replay.
32. interaction-32: Serialized claims survive separate raw API calls for inspect/accept on the same PTY, and persistent native clients use that lifecycle with cached authority, not per-key registration. Raw API exit0 alone is not grant evidence.

## 18 Out of scope

No separate new fencing spike, upstream patch, provider replacement, fake bootstrap/fixtures-as-real-proof, force-only acceptance, autonomous retry/reacquisition, runtime persistence/restoration, new remote service or native UI replacement. No task acceptance policy, merge/push/release or phase advancement is implied by writing these files.

## 19 Risks

The supported native application seam and truthful accepted-answer presentation remain unproved; final complete-broker validation must establish them or name the exact blocker. Never declare the story complete because explicit force remains usable. Real-model question/approval elicitation and all32 integration must stay within six meaningful turns/1800s; inability to finish is a reported blocker, not permission to expand budget silently or weaken criteria. Shell continuation and private launch authority can otherwise bypass revocation, so prove both actual dispatch provenance and retirement only after authoritative acceptance/settlement. Multi-view and warning races require actual original connection facts, not forged test source labels.

## 20 Evidence and handoff

All tasks are failing; no runtime, native probe, model call, build, test, lint or validation ran during Plan. At Build/Verify retain redacted all32 scenario-to-assertion/native-capture mapping, exact model/installed version, actual turn/deadline accounting, owned cleanup results and explicitly unresolved seams. Distinguish deterministic fixtures, real-daemon regressions, real Codex smoke and independent showcase evidence. First acceptance is blocked until every criterion passes; parent owns final state/index/quality integration and user phase confirmation.
