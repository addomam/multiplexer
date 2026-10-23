---
story_id: e03s06
title: Structured questions and permission decisions
maturity: 3
status: passing
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 8
---

## 1 Identity

Slice e03s06; manifest delta ADDED; dependency e03s05. This plan supplies executable implementation obligations, not a claim that protocol fixtures or real requests have passed.

## 2 User story

As the current human or external controller, I can inspect an actual pending question or permission request on the same native conversation, answer it structurally or explicitly allow/deny it, and know which authoritative response won.

## 3 Problem

Lifecycle observation and text prompts cannot answer a provider server request. Reused numeric IDs, optimistic native presentation, late messages and native/API response races can misattribute a losing answer to a different request unless complete identities and authoritative outcomes are retained.

## 4 Goal

Deliver genuine structured answers and per-request permission allow/deny through agent-answer and agent-permission, with authoritative pending registry and tombstones. A reply acknowledgement or unrelated resolved request is not native-input settlement, whole-turn completion or task acceptance.

## 5 Scope

Extend src/codex.rs, src/daemon.rs, src/client.rs and src/cli.rs with the two closed operations and inspectable exact pending identities/outcomes. Expand tests/agent_interaction.rs and tests/codex_native_smoke.rs created by e03s05. Consume ownership and retained-dispatch contracts without adding a provider registry or generic method passthrough.

## 6 Actors

The provider emits authoritative question/approval requests and resolution events. The daemon owns the registry and serializes responses with ownership admission. Native humans and external controllers may respond through their currently authorized surface; observers inspect without gaining input authority. The external caller chooses content/decision, never amux.

## 7 Preconditions

e03s05 has established an owned same-instance native/API target and created both test targets. Installed 0.160.0 generated schema must define the handled request and response shapes. A selected real smoke requires fresh explicit live-run approval, version/auth/native-PTY prerequisites, AMUX_RUN_REAL_CODEX=1 and explicit nonempty AMUX_CODEX_MODEL. Earlier spike authorization does not authorize this run.

## 8 Dependencies

Reuse [OK] tungstenite 0.30.0 handshake-only dependency from e03s05; no new package. Use existing sequenced events/snapshot recovery and src/control.rs per-PTY ordering/retained-call registry. This slice must not depend on a future interrupt operation to fake request cancellation or clean up a failed live probe; owned process stop remains available separately.

## 9 Module purpose callers and contracts

src/codex.rs maintains authoritative pending and resolved/tombstoned identities and maps validated closed actions to installed protocol responses. Its daemon callers submit exact PendingTarget plus current control; inspect/event consumers receive public IDs and lifecycle, not private capabilities. PendingTarget binds terminal lifetime, instance, session, turn, item and exact provider request ID; question IDs are separate answer keys. Preserve integer versus string provider IDs. Tombstones remain associated with the old identity for the live instance, never retargeted by label or numeric coincidence. Existing owning-native connection provenance and cached control remain unchanged.

## 10 Requirements

- ADDED: agent-answer takes target, current control, exact pending turn/item/provider request identity and answers keyed by question IDs. Validate current pending semantics and encode the actual installed response shape. Preserve supported choice/free-text/multiple-selection semantics; reject invalid or unrecognized combinations rather than guessing strings or simulating terminal keys.
- ADDED: agent-permission takes the same complete pending identity/control and decision=allow|deny for that request only. Bind actual supported approval kinds to their installed response shapes; do not infer approval, grant a session-wide permission or automatically allow an unknown request.
- ADDED: Registry admission and queued dispatch validate authority, exact identity and current request state at the same ordering boundary as revocation. Native and API responses share the provider's authoritative request; only its first valid response may apply. Retain forwarded contenders and authoritative losing/rejected/unknown outcomes; do not report local optimistic acceptance as the winning answer.
- ADDED: Resolved, cancelled, timed-out, duplicate and previous-session/instance request identities are tombstoned or explicitly invalid; reject stale responses before forwarding. Native resolution racing with queued API input must invalidate that queued pending identity. Late events update only the retained matching request/call and cannot reopen a tombstone or settle unrelated input.
- ADDED: Inspect exposes exact pending questions/permissions and unknown/unsupported encountered shapes honestly. Missing mandatory structured question or permission allow/deny interfaces block first acceptance; do not relabel them optional, replace them with native-only actions or use fake fixtures as proof of real support.

## 11 Interfaces and data

PLAN records the real question response `{"id":0,"result":{"answers":{"color":{"answers":["Blue"]}}}}` and notes that `serverRequest/resolved` carries threadId/requestId. ADR0003 binds questions to item/tool/requestUserInput and command approvals to accept/decline. These facts are limited recorded probes, not a complete fresh-start transcript or proof of all question/permission permutations. Check installed 0.160.0 schema using `codex app-server generate-json-schema --out PATH` in private temporary storage and remove it after binding; current web docs alone are insufficient. Keep response envelope ID type exact; do not confuse a provider request ID with item, turn or question IDs. Return retained call receipts with queued/forwarded/applied/rejected/unknown boundaries, not task-success booleans.

## 12 Main flow

Inspect the discovered live target and current authoritative pending request. Acquire or retain current control through the ordinary capability protocol. Submit a keyed answer or explicit per-request permission decision. Register forwarding atomically, observe provider resolution of that exact identity, expose which response applied and continue the same conversation natively. Inspect after uncertain socket loss instead of resending the response.

## 13 Alternate and error flows

A native response may win before API dispatch: reject the stale queued response without forwarding it. If two responses were already forwarded, use provider-authoritative outcome; unknown remains unknown and does not imply both applied. Reject wrong question IDs/choices, wrong ID type, missing fields, stale authority, old session, timed-out/cancelled/resolved requests and unsupported approval shapes explicitly. A gap or disconnect must not clear pending requests as if resolved; resynchronize or expose unavailable. Missing mandatory interfaces block acceptance even if a fixture or native-only path succeeds. An unrelated question resolution cannot certify buffered native input.

## 14 Nonfunctional constraints

No provider I/O under state locks or slowdown of PTY processing from observers. No automatic answer selection, approvals, retries or new packages. Real-run bounds: <=1800s watchdog, <=15s daemon/30s backend/180s model-response deadlines and <=6 meaningful turns including every actual model turn. Use private mode0700 HOME/config/runtime/CODEX_HOME/workspace/socket roots and private auth copy only as needed. Restore terminal modes and terminate/reap only owned daemon/frontend/backend process groups on every exit; verify sockets/roots removed and delete copied credentials, schemas, rollouts and markers. Evidence is redacted; global Codex sessions/config/processes/binary are untouched.

## 15 Implementation steps

1. Implement authoritative pending request registry, exact string/integer identities, installed-schema question semantics and tombstones in src/codex.rs; expand tests/agent_interaction.rs with resolution/reuse/session-gap regressions -> verify: `cargo test --test agent_interaction`.
2. Wire agent-answer and agent-permission through daemon/client/CLI with current-control/queued-dispatch validation, real response shapes and explicit request-only decisions; test native/API first-valid races, revoked queues and retained unknown outcomes -> verify: `cargo test --test agent_interaction && cargo test --test pty_control`.
3. Add ignored exact root real_questions_and_permissions to tests/codex_native_smoke.rs with inherited opt-in/private-resource/deadline/turn-budget guards and actual native captures; update specs/adr/0003-codex-first-agent-target.md with supported semantics and evidence limits -> verify: `cargo test --test codex_native_smoke --no-run && cargo test --test agent_interaction`.
4. With later explicit live-run approval, answer a real question and exercise both real permission decisions on the same native/API instance, verify authoritative winning response and continuation plus owned cleanup -> verify: `test "${AMUX_RUN_REAL_CODEX:-}" = 1 && test -n "${AMUX_CODEX_MODEL:-}" && cargo test --test codex_native_smoke real_questions_and_permissions -- --ignored --exact --nocapture --test-threads=1`.

## 16 Verification script

The deterministic target was created by e03s05; extend it with actual installed-schema request/response fixtures and adversarial ordering, not source-text assertions. Cover choice/free-text semantics the installed interface exposes, string/integer ID distinctions, multiple questions, duplicate request delivery, wrong answer keys, native resolution before dispatch, late API outcomes, old-instance/session numeric ID reuse, tombstone non-reopening and disconnect/gap unknowns. Fixtures prove amux behavior under those protocol messages, not that a model emitted a genuine question/approval. The real smoke name is created in step 3 and selected only by the guarded step 4 command after explicit approval; absence of version/auth/model/native prerequisites must fail visibly, never skip/substitute fixture success. Drive actual amux attachments on controlling PTYs and retain rendered request/answer and ownership captures separately. Assert the provider's accepted answer/decision and same-request continuation, not a local send return. Exercise both allow and deny using genuine requests within the turn budget. If required semantics cannot be provoked or supported, report a blocker, not acceptance. Revisit losing-answer authoritative presentation under the complete broker in e03s08.

## 17. Acceptance criteria

1. interaction-03: A genuine pending harness question is inspectable with exact identity and answerable through agent-answer; supported choice/free-text semantics are preserved and validated rather than reduced to simulated keys.
2. interaction-04: Real permission requests are inspectable and agent-permission explicitly allows and denies actual requests through their installed structured response shapes, with per-request scope.
3. interaction-05: Native and API responses refer to the same authoritative request, and only the first valid response applies; the losing response is never reported as the accepted winner.
4. interaction-06: Late, duplicate, cancelled, resolved, timed-out and previous-session responses cannot answer a later request, even when request labels/numeric values repeat.
5. interaction-15: Missing mandatory structured interfaces visibly block first acceptance; native-only behavior, guessed responses or fixtures cannot waive real question and allow/deny proof.

## 18 Out of scope

No interrupt implementation until e03s07, generic permission service, autonomous responses, session-wide grants or provider extension framework. Final native-buffer/application accounting and authoritative optimistic-answer presentation are completed and revalidated under e03s08, not an early separate spike. This story alone is not first acceptance.

## 19 Risks

Native optimistic answer display previously contradicted the authoritative winner; retain both authoritative outcome and uncertainty, and require final visible proof. Installed question and approval variants may exceed prior probes: unsupported encountered variants remain visible and never auto-approved. Cancellation/cleanup may arrive after turn completion, so do not expire identity protection on a guessed turn boundary. Live runs may fail to elicit a required genuine request; bounded failure is evidence of a blocker, not permission for fabricated proof.

## 20 Evidence and handoff

All tasks remain failing; no commands or live calls ran in Plan. Evidence must distinguish deterministic protocol-race tests from opt-in genuine provider/native observations. Pass exact request identity, tombstone and receipt contracts to e03s07; pass losing-answer and unresolved-native accounting obligations to e03s08. Preserve all32 and the implementation phase gate.
