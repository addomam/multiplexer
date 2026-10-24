---
story_id: e04s01
title: Shared ACP session and conversation view
maturity: 3
type: feat
context: specs/epics/e04-acp-agent-interaction/context.yaml
risk: P0
bcps: 8
status: passing
---

## 1 Identity
e04s01; separate e04 ACP cycle. Architecture confirmed; Build is conditionally authorized only after two new independent Plan reviews pass. Historical e03 counters/status are unchanged.

## 2 User story
As an amux human or local API controller, I can open one owned ACP session, submit a gated prompt and see its truthful stream in the actual amux view, then close it without leaks.

## 3 Problem
The native terminal application fence is unresolved; new ACP interaction must be authoritative and same-session without claiming to repair that fallback.

## 4 Goal
Open one owned ACP session, submit a gated prompt and see its truthful stream in the actual amux view, then close it without leaks.

## 5 Scope
Explicit ACP target/API/CLI/view path; official schema-only transport, initial authority gate, asynchronous setup/auth visibility, metadata privacy, bounded framing/queues/history and owned lifecycle.

## 6 Actors
Daemon owns authority and canonical bounded broker state. A genuine original native attachment owns the human editor. External controllers use private handles across sockets; the owned adapter speaks ACP v1 and may use its own tools. No actor can invent application acknowledgement.

## 7 Preconditions
Use feat/acp-agent-interaction worktree, preserve original dirty checkout, passing baseline and separate live/provider/publication gates. Keep every task failing until actual named verification and smoke pass. Refresh LSP references before exported changes; repair isolated-workspace indexing rather than infer no callers from empty results.

## 8 Dependencies
No e04 prerequisite; signed main workspace baseline already passed. Form capability remains absent until s02; incoming unsupported requests receive explicit errors rather than hanging. Normative: PLAN.md, build-integration-contract.md, ADR0004, IMPACT_LATEST, e04-TEST_PLAN_LATEST and THREAT_MODEL.

## 9 Module purpose callers and contracts
PLAN/IMPACT define exact existing callers and preserved contracts. Parent alone changes control/daemon/client/CLI/top-level TUI/manifests/global ledgers. Concrete ACP broker owns protocol/process/identity/retention; Reason for Depth: prevent callers independently reconstructing races and outcomes. ACP editor owns bounded identity-bound drafts; Reason for Depth: typed human actions must not become raw terminal input or optimistic history. Authority admission is one memory-only target→ACP-state transaction, no I/O/wait/join or reverse/topology lock edge.

## 10 Requirements
**MODIFIED — Shared ACP session and conversation view.** **Before:** Managed same-instance agent interaction uses the delivered native-Codex terminal path; no separate ACP conversation view or admission boundary exists. **After:** An explicit ACP-only target owns its connection/session and amux view; native terminals and legacy agent-* remain unchanged. ACP actions are fenced at atomic amux admission, not claimed provider application.

All applicable contracts in PLAN/ADR apply from first exposure: exclusive control; force never implicitly interrupts/kills; ordinary pending cancel revokes old control/grants none/retains work; keep/withdraw preserves human control with external policy; every application-input route gated; no replay; same exact live session and genuine interactions; bounded resource/privacy/owned cleanup. ACP admission is not provider application or completed acceptance.

## 11 Interfaces and data
Use the exact target variants, eight ACP operations, current-control/captured-generation/call identity, canonical projections, worker API and file ownership in build-integration-contract.md. Official schema 1.12.0/default-features=false [OK], stable v1, no SDK runtime/executor or generic plugin interface. Native terminal/agent-* closed variants and OMP agent-settled remain unchanged.

## 12 Main flow
Register external A, open by exact project/argv, inspect starting then ready, submit a unique prompt with returned target/control/generation, attach the same view, observe the session-specific stream, deliberately close and observe owned cleanup before fallback.

## 13 Alternate and error flows
Wrong target/authority/generation/scope/revision, stale or duplicate actions fail without target effect. Unavailable capability/schema/auth gives explicit error, not automatic fallback/login/approval. EOF/overflow/unconfirmed delivery retain truthful unknown and exact original IDs. Never infer agent application from pipe write or interruption from notification/error/timeout. Late lifecycle events cannot resurrect a closed target.

## 14 Nonfunctional constraints
PLAN resource budgets and pre-accumulation/count+byte bounds are mandatory, including in-flight/reserved work. Single nonblocking FIFO application consumer, dedicated blocking writer, continued stderr drain. Runtime state only, metadata content-free, incremental transcript gaps explicit. Escape/control data is inert in the host terminal. New live runs/global daemon/config/auth changes/main publication are not authorized here.

## 15 Implementation steps
1. Open one separate ACP target end to end through real CLI/daemon with returned initial authority, asynchronous initialize/newSession, basic human view and exact-session prompt stream; use standalone non-production acp_fixture stdio peer. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s01_ -- --nocapture`
2. Implement initial exact-identity atomic authority→ACP admission, duplicate call rejection, foreground busy semantics and retained RPC mapping through caller loss; no initially unguarded route. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s01_ -- --nocapture`
3. Implement bounded owned stdio/FIFO using official schema payloads; prove newline-free, malformed, ingress/egress/reservation/in-flight/stall failures and responsive concurrent daemon operations. → verify: `cargo build --example acp_fixture && cargo test --lib acp && cargo test --test acp e04s01_ -- --nocapture`
4. Implement exact explicit agent-type authenticate progression, error/closure foreground release, metadata-only snapshots and incremental content/history; continue draining capped stderr. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s01_ -- --nocapture`
5. Implement owned child/descendant startup/publication/explicit ACP close/daemon shutdown cleanup outside locks; test failed capture, late publication and unrelated sentinels before fallback. Native shell close/remove must preserve ACP views; no project-close API. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s01_ -- --nocapture`
6. Integrate actual native attachment/CLI surface and all initial input-kind rejection routes; exercise private runtime smoke, preserve existing ownership/native broker regressions and write redacted evidence. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s01_ -- --nocapture && cargo test --test pty_control --test agent_interaction`

## 16 Verification script
Run each command from the ACP worktree after implementing its actual cases; zero-case filters are failure. Use private daemon/socket/project/HOME roots, deterministic real stdio peer and explicit barriers. Launch built CLI and actual controlling-PTY attachment outside test-only proof; observe expected same-session content/dispositions and absence of stale marker effects. Observe owned child/descendant/worker/socket/root cleanup before fallback and unrelated sentinels alive. Record case names/counts and redacted results. No live adapter/auth/model launch in this slice.

## 17. Acceptance criteria
1. **SC-e04s01-P0-01** — Given a private project and registered controller; when CLI opens an ACP adapter and API prompts its exact target; then one child/connection/session streams the marker into that same actual human view, without a fake terminal.
2. **SC-e04s01-P0-02** — Given an active target with current A authority; when invalid/stale/wrong-target inputs, duplicate call IDs or a second prompt are submitted; then they produce no target marker or second foreground turn; every initially exposed mutation is gated.
3. **SC-e04s01-P0-03** — Given initialize/newSession/prompt or explicit auth is pending; when updates/requests/responses or caller disconnect arrive; then the FIFO keeps processing exact identities and retains outcomes; no automatic auth/cancel/steer.
4. **SC-e04s01-P0-04** — Given a bounded live transport; when oversized/newline-free/malformed/flooded/stalled traffic reaches each budget; then the exact instance fails/degrades with unknown affected work while ownership/snapshot/close stay responsive.
5. **SC-e04s01-P0-05** — Given owned child/descendants and an unrelated sentinel; when failed/late startup, explicit ACP close and daemon shutdown race; then owned resources exit once without resurrection or unrelated signal, observed before fallback. Native shell close/remove preserves the separate live ACP view.
6. **SC-e04s01-P1-06** — Given synthetic secret markers and capped output; when global snapshot/events/errors and explicit content inspect are read; then metadata stays content-free, content gaps are explicit, live records retained and stderr keeps draining.

## 18 Out of scope
No generic non-ACP integration API, provider trait, retry policy, restoration/persistence, remote protocol, unstable ACP v2, replacement stock Zsh/OMP behavior or retroactive e03 fence/review pass. No unrelated refactoring, global daemon/config/auth changes, ACP merge/push/release. Later stories extend the same canonical session, not a second writer/backend.

## 19 Risks
P0 authority/revoke/response identities and process cleanup; P1 untrusted rendering, privacy, capacity and schema semantics. Tests assert actual consumer effects and state transitions, not source/wiring/default/wording echoes. Fresh code/security audit must show no new unresolved findings in affected paths before completion. Review/compile/transport success is not runtime acceptance.

## 20 Evidence and handoff
Evidence destination: specs/verifications/e04s01-build.md (created only with actual results). All tasks/scenarios start failing. Record exact commands, named cases/counts, runtime surface observations, pins/cap boundaries and owned cleanup; no real content/auth/env/raw tracing. Next story consumes the integrated contract, never unmerged worker assumptions. Thin non-ACP API remains deferred; e03s08 stays failing.
