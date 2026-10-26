---
story_id: e04s03
title: Admission-fenced ownership and retained work
maturity: 3
type: feat
context: specs/epics/archive/e04-acp-agent-interaction/context.yaml
risk: P0
bcps: 8
status: done
---

## 1 Identity
e04s03; separate e04 ACP cycle. Architecture confirmed; Build is conditionally authorized only after two new independent Plan reviews pass. Historical e03 counters/status are unchanged.

## 2 User story
As an amux human or local API controller, I can hand off ACP control during a running turn/question without interruption or stale replay, preserving human conditional consent and recoverable work.

## 3 Problem
The native terminal application fence is unresolved; new ACP interaction must be authoritative and same-session without claiming to repair that fallback.

## 4 Goal
Hand off ACP control during a running turn/question without interruption or stale replay, preserving human conditional consent and recoverable work.

## 5 Scope
Cooperative/force/cancel/conditional ownership integration, captured draft/action identities, all application-input routes, interruption outcomes and disconnect/gap recovery.

## 6 Actors
Daemon owns authority and canonical bounded broker state. A genuine original native attachment owns the human editor. External controllers use private handles across sockets; the owned adapter speaks ACP v1 and may use its own tools. No actor can invent application acknowledgement.

## 7 Preconditions
Use feat/acp-agent-interaction worktree, preserve original dirty checkout, passing baseline and separate live/provider/publication gates. Keep every task failing until actual named verification and smoke pass. Refresh LSP references before exported changes; repair isolated-workspace indexing rather than infer no callers from empty results.

## 8 Dependencies
s01–02 integrated same-session backend/view and canonical interaction contracts. Reuse the existing single ownership registry; no second state machine or whole-turn settlement requirement. Normative: PLAN.md, build-integration-contract.md, ADR0004, IMPACT_LATEST, e04-TEST_PLAN_LATEST and THREAT_MODEL.

## 9 Module purpose callers and contracts
PLAN/IMPACT define exact existing callers and preserved contracts. Parent alone changes control/daemon/client/CLI/top-level TUI/manifests/global ledgers. Concrete ACP broker owns protocol/process/identity/retention; Reason for Depth: prevent callers independently reconstructing races and outcomes. ACP editor owns bounded identity-bound drafts; Reason for Depth: typed human actions must not become raw terminal input or optimistic history. Authority admission is one memory-only target→ACP-state transaction, no I/O/wait/join or reverse/topology lock edge.

## 10 Requirements
**MODIFIED — Admission-fenced ownership and retained work.** **Before:** Native handoff depends on its unresolved application fence and original terminal attachment consent; terminal input cannot prove the new ACP admission cut. **After:** Only ACP targets use proven atomic amux admission with accepted work retained across transfer; genuine ACP-view attachment provenance supplies conditional consent. Native fence/claims remain unchanged and failing acceptance is preserved.

All applicable contracts in PLAN/ADR apply from first exposure: exclusive control; force never implicitly interrupts/kills; ordinary pending cancel revokes old control/grants none/retains work; keep/withdraw preserves human control with external policy; every application-input route gated; no replay; same exact live session and genuine interactions; bounded resource/privacy/owned cleanup. ACP admission is not provider application or completed acceptance.

## 11 Interfaces and data
Use the exact target variants, eight ACP operations, current-control/captured-generation/call identity, canonical projections, worker API and file ownership in build-integration-contract.md. Official schema 1.12.0/default-features=false [OK], stable v1, no SDK runtime/executor or generic plugin interface. Native terminal/agent-* closed variants and OMP agent-settled remain unchanged.

## 12 Main flow
Human A owns a live ACP view while its prompt asks a real question. B requests cooperative control; A yields and B explicitly accepts the reserved offer without waiting for turn completion, answers the question and observes original prompt finish. Repeat force/pending cancel and conditional keep/withdraw with stale drafts and private recovery.

## 13 Alternate and error flows
Wrong target/authority/generation/scope/revision, stale or duplicate actions fail without target effect. Unavailable capability/schema/auth gives explicit error, not automatic fallback/login/approval. EOF/overflow/unconfirmed delivery retain truthful unknown and exact original IDs. Never infer agent application from pipe write or interruption from notification/error/timeout. Late lifecycle events cannot resurrect a closed target.

## 14 Nonfunctional constraints
PLAN resource budgets and pre-accumulation/count+byte bounds are mandatory, including in-flight/reserved work. Single nonblocking FIFO application consumer, dedicated blocking writer, continued stderr drain. Runtime state only, metadata content-free, incremental transcript gaps explicit. Escape/control data is inert in the host terminal. New live runs/global daemon/config/auth changes/main publication are not authorized here.

## 15 Implementation steps
1. Prove both admission/revocation orders under stalled transport and running question; preserve accepted actions without later owner revalidation and permit safe ACP offer before whole-turn completion. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s03_ -- --nocapture`
2. Integrate cooperative/force/pending-cancel and private recovery with the single ownership registry; preserve same process/session/RPC, reserved gap and noncancelling force. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s03_ -- --nocapture`
3. Integrate genuine original-attachment ACP focus/activity/conditional warnings, keep/withdraw and final revalidation without PTY-native provenance fabrication or automatic policy. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s03_ -- --nocapture`
4. Fence all top-level raw key/paste/mouse/focus/prefix/resize routes and captured drafts against revocation/identity change; no fresh control substitution or later replay. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s03_ -- --nocapture && cargo test --test pty_control native`
5. Verify exact-prompt explicit interrupt, truthful stopReason/error/EOF transitions and authoritative gap/disconnect resync with inert untrusted terminal rendering. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s03_ -- --nocapture`
6. Run actual two-surface handoff/conditional/stale-input runtime smoke, cleanup observations, old ownership/native regression boundaries and redacted evidence. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s03_ -- --nocapture && cargo test --test pty_control`

## 16 Verification script
Run each command from the ACP worktree after implementing its actual cases; zero-case filters are failure. Use private daemon/socket/project/HOME roots, deterministic real stdio peer and explicit barriers. Launch built CLI and actual controlling-PTY attachment outside test-only proof; observe expected same-session content/dispositions and absence of stale marker effects. Observe owned child/descendant/worker/socket/root cleanup before fallback and unrelated sentinels alive. Record case names/counts and redacted results. No live adapter/auth/model launch in this slice.

## 17. Acceptance criteria
1. **SC-e04s03-P0-01** — Given old prepared input and a revocation barrier; when commit/revoke execute in either order; then revoked-before-commit cannot affect target while committed-before-revoke remains retained and can transmit after handoff.
2. **SC-e04s03-P0-02** — Given A running a turn/question and B/C competing; when cooperative or force takeover happens; then only exact reserved acceptance grants; force changes authority only, same process/session/request survives and new owner can answer.
3. **SC-e04s03-P0-03** — Given a pending claim and accepted work; when cancel and late yield/accept/force or caller loss race; then old control invalidates, nobody is granted by ordinary cancellation, newer reservations survive and private recovery reports retained actual state.
4. **SC-e04s03-P0-04** — Given a genuine original attachment focused on an ACP view; when conditional warning/keep/withdraw/accept or forged view occurs; then human control is preserved on keep/withdraw, final facts revalidate and external policy remains external without fake native batches.
5. **SC-e04s03-P0-05** — Given draft/control/interaction identities captured before revoke; when keys/paste/mouse/focus/double-prefix and later reacquisition occur; then no intercepted old input reaches adapter or replays; only deliberate fresh current input works.
6. **SC-e04s03-P0-06** — Given a current foreground prompt; when explicit current/stale interrupt, noncancelled result/error/EOF or close occurs; then only authorized exact prompt sends session/cancel; cancelled stop reason alone confirms interruption and newer work is not cancelled.
7. **SC-e04s03-P1-07** — Given event gap/disconnect and untrusted content; when view resyncs or renders mixed updates/escape bytes; then unknowns/identities stay truthful, no invisible replacement/turn attribution or host-terminal injection.

## 18 Out of scope
No generic non-ACP integration API, provider trait, retry policy, restoration/persistence, remote protocol, unstable ACP v2, replacement stock Zsh/OMP behavior or retroactive e03 fence/review pass. No unrelated refactoring, global daemon/config/auth changes, ACP merge/push/release. Later stories extend the same canonical session, not a second writer/backend.

## 19 Risks
P0 authority/revoke/response identities and process cleanup; P1 untrusted rendering, privacy, capacity and schema semantics. Tests assert actual consumer effects and state transitions, not source/wiring/default/wording echoes. Fresh code/security audit must show no new unresolved findings in affected paths before completion. Review/compile/transport success is not runtime acceptance.

## 20 Evidence and handoff
Evidence destination: specs/verifications/e04s03-build.md (created only with actual results). All tasks/scenarios start failing. Record exact commands, named cases/counts, runtime surface observations, pins/cap boundaries and owned cleanup; no real content/auth/env/raw tracing. Next story consumes the integrated contract, never unmerged worker assumptions. Thin non-ACP API remains deferred; e03s08 stays failing.
