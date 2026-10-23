---
story_id: e04s02
title: Genuine forms and permission decisions
maturity: 3
type: feat
context: specs/epics/e04-acp-agent-interaction/context.yaml
risk: P0
bcps: 8
status: failing
---

## 1 Identity
e04s02; separate e04 ACP cycle. Architecture confirmed; Build is conditionally authorized only after two new independent Plan reviews pass. Historical e03 counters/status are unchanged.

## 2 User story
As an amux human or local API controller, I can answer a genuine question or offered permission through API or human view on the same session with exactly one truthful canonical disposition.

## 3 Problem
The native terminal application fence is unresolved; new ACP interaction must be authoritative and same-session without claiming to repair that fallback.

## 4 Goal
Answer a genuine question or offered permission through API or human view on the same session with exactly one truthful canonical disposition.

## 5 Scope
Request/session-scoped forms, all known primitive/enum/multiselect fields, defaults/required/optional semantics, permission options, canonical competing decisions and complete explicit cancellation obligations.

## 6 Actors
Daemon owns authority and canonical bounded broker state. A genuine original native attachment owns the human editor. External controllers use private handles across sockets; the owned adapter speaks ACP v1 and may use its own tools. No actor can invent application acknowledgement.

## 7 Preconditions
Use feat/acp-agent-interaction worktree, preserve original dirty checkout, passing baseline and separate live/provider/publication gates. Keep every task failing until actual named verification and smoke pass. Refresh LSP references before exported changes; repair isolated-workspace indexing rather than infer no callers from empty results.

## 8 Dependencies
s01 stable reviewed target/admission/transport/projection interfaces. No new provider model/auth calls. This story enables form capability only after its supported controls/response paths pass. Normative: PLAN.md, build-integration-contract.md, ADR0004, IMPACT_LATEST, e04-TEST_PLAN_LATEST and THREAT_MODEL.

## 9 Module purpose callers and contracts
PLAN/IMPACT define exact existing callers and preserved contracts. Parent alone changes control/daemon/client/CLI/top-level TUI/manifests/global ledgers. Concrete ACP broker owns protocol/process/identity/retention; Reason for Depth: prevent callers independently reconstructing races and outcomes. ACP editor owns bounded identity-bound drafts; Reason for Depth: typed human actions must not become raw terminal input or optimistic history. Authority admission is one memory-only target→ACP-state transaction, no I/O/wait/join or reverse/topology lock edge.

## 10 Requirements
**ADDED — Genuine forms and permission decisions.** Real ACP requests, not terminal text parsing or manufactured questions, drive both surfaces. Exact response identity and canonical disposition survive ownership changes and transport ambiguity.

All applicable contracts in PLAN/ADR apply from first exposure: exclusive control; force never implicitly interrupts/kills; ordinary pending cancel revokes old control/grants none/retains work; keep/withdraw preserves human control with external policy; every application-input route gated; no replay; same exact live session and genuine interactions; bounded resource/privacy/owned cleanup. ACP admission is not provider application or completed acceptance.

## 11 Interfaces and data
Use the exact target variants, eight ACP operations, current-control/captured-generation/call identity, canonical projections, worker API and file ownership in build-integration-contract.md. Official schema 1.12.0/default-features=false [OK], stable v1, no SDK runtime/executor or generic plugin interface. Native terminal/agent-* closed variants and OMP agent-settled remain unchanged.

## 12 Main flow
Hold a prompt; peer emits a real form and permission. Review/edit in the human view, answer via API and reverse surfaces; exact response resumes original prompt. Race responses and admitted interrupt, inspect the sole canonical disposition and real transport outcome.

## 13 Alternate and error flows
Wrong target/authority/generation/scope/revision, stale or duplicate actions fail without target effect. Unavailable capability/schema/auth gives explicit error, not automatic fallback/login/approval. EOF/overflow/unconfirmed delivery retain truthful unknown and exact original IDs. Never infer agent application from pipe write or interruption from notification/error/timeout. Late lifecycle events cannot resurrect a closed target.

## 14 Nonfunctional constraints
PLAN resource budgets and pre-accumulation/count+byte bounds are mandatory, including in-flight/reserved work. Single nonblocking FIFO application consumer, dedicated blocking writer, continued stderr drain. Runtime state only, metadata content-free, incremental transcript gaps explicit. Escape/control data is inert in the host terminal. New live runs/global daemon/config/auth changes/main publication are not authorized here.

## 15 Implementation steps
1. Implement official typed request-/session-scoped genuine forms with retained exact RPC scope, asynchronous bootstrap support and capability enablement only after full supported field paths. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s02_ -- --nocapture`
2. Render/edit all known flat primitive, enum and multiselect controls with required/optional/default and supported validation semantics, explicit review/accept/decline/cancel and truthful pattern/format limits. → verify: `cargo build --example acp_fixture && cargo test --lib acp && cargo test --test acp e04s02_ -- --nocapture`
3. Implement same-session tool permissions with exact offered IDs/kinds/scope and actual human/API allow/deny paths; never synthesize options or implicit dismissal. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s02_ -- --nocapture`
4. Linearize competing API/native form/permission decisions with authority admission, retaining selected/submitted/failed/unknown evidence while excluding rejected losing answers from shared history. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s02_ -- --nocapture`
5. Commit explicit cancellation obligations and pending/late session permission/form dispositions atomically with reserved writer capacity before publication; selected requests never receive duplicate responses. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s02_ -- --nocapture`
6. Exercise genuine question/permission human surfaces and protocol errors with private CLI/controlling-PTY smoke, observe owned cleanup and record redacted evidence. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s02_ -- --nocapture`

## 16 Verification script
Run each command from the ACP worktree after implementing its actual cases; zero-case filters are failure. Use private daemon/socket/project/HOME roots, deterministic real stdio peer and explicit barriers. Launch built CLI and actual controlling-PTY attachment outside test-only proof; observe expected same-session content/dispositions and absence of stale marker effects. Observe owned child/descendant/worker/socket/root cleanup before fallback and unrelated sentinels alive. Record case names/counts and redacted results. No live adapter/auth/model launch in this slice.

## 17. Acceptance criteria
1. **SC-e04s02-P0-01** — Given a pending prompt or retained bootstrap RPC; when the adapter emits a genuine session- or request-scoped form; then both surfaces address the exact same interaction/connection/request and answer resumes that original work.
2. **SC-e04s02-P1-02** — Given each known field type and declared defaults/constraints; when human edits/reviews/submits or API submits typed values; then required/optional/unset/default and enum/multiselect/numeric semantics hold; unsupported schemas fail explicitly and local pattern/format limits are visible.
3. **SC-e04s02-P0-03** — Given a genuine tool request with offered permission IDs; when native/API deliberately select real allow/reject once/always choices; then only the offered opaque ID is sent; no fabricated permission or dismiss-as-deny.
4. **SC-e04s02-P0-04** — Given API and human responses paused at canonical admission; when either wins the same interaction revision; then exactly one selection/response is committed and the loser never appears as accepted in history.
5. **SC-e04s02-P0-05** — Given pending session permissions/forms and current-prompt interrupt; when cancel races selections and late incoming requests or capacity exhaustion; then required Cancelled/Cancel responses reserve atomically; late requests never become selectable and failure stays truthful.
6. **SC-e04s02-P1-06** — Given a real form and unadvertised URL/fs/terminal or unknown request; when decline/cancel/unsupported mode is selected or received; then actions remain distinct and explicit errors replace silent hangs, navigation or unsafe credential workflows.

## 18 Out of scope
No generic non-ACP integration API, provider trait, retry policy, restoration/persistence, remote protocol, unstable ACP v2, replacement stock Zsh/OMP behavior or retroactive e03 fence/review pass. No unrelated refactoring, global daemon/config/auth changes, ACP merge/push/release. Later stories extend the same canonical session, not a second writer/backend.

## 19 Risks
P0 authority/revoke/response identities and process cleanup; P1 untrusted rendering, privacy, capacity and schema semantics. Tests assert actual consumer effects and state transitions, not source/wiring/default/wording echoes. Fresh code/security audit must show no new unresolved findings in affected paths before completion. Review/compile/transport success is not runtime acceptance.

## 20 Evidence and handoff
Evidence destination: specs/verifications/e04s02-build.md (created only with actual results). All tasks/scenarios start failing. Record exact commands, named cases/counts, runtime surface observations, pins/cap boundaries and owned cleanup; no real content/auth/env/raw tracing. Next story consumes the integrated contract, never unmerged worker assumptions. Thin non-ACP API remains deferred; e03s08 stays failing.
