---
story_id: e04s04
title: Complete ACP acceptance and truthful fallback
maturity: 3
type: feat
context: specs/epics/archive/e04-acp-agent-interaction/context.yaml
risk: P0
bcps: 8
status: done
---

## 1 Identity
e04s04; separate e04 ACP cycle. Architecture confirmed; Build is conditionally authorized only after two new independent Plan reviews pass. Historical e03 counters/status are unchanged.

## 2 User story
As an amux human or local API controller, I can prove the integrated ACP broker against the actual human surface and both approved real adapters while preserving honest best-effort terminal fallback and all regressions.

## 3 Problem
The native terminal application fence is unresolved; new ACP interaction must be authoritative and same-session without claiming to repair that fallback.

## 4 Goal
Prove the integrated ACP broker against the actual human surface and both approved real adapters while preserving honest best-effort terminal fallback and all regressions.

## 5 Scope
Complete local adversarial/same-session/cleanup/privacy matrix, actual CLI/TUI smoke, live Codex/Grok acceptance using existing OAuth subscriptions and cheap/free supported models, workspace/showcase preservation and accurate limitations/evidence. Subsequent explicit user authorization replaces Claude in the live matrix only; historical Plan/review/installation evidence remains intact.

## 6 Actors
Daemon owns authority and canonical bounded broker state. A genuine original native attachment owns the human editor. External controllers use private handles across sockets; the owned adapter speaks ACP v1 and may use its own tools. No actor can invent application acknowledgement.

## 7 Preconditions
Use feat/acp-agent-interaction worktree, preserve original dirty checkout, passing baseline and separate live/provider/publication gates. Keep every task failing until actual named verification and smoke pass. Refresh LSP references before exported changes; repair isolated-workspace indexing rather than infer no callers from empty results.

## 8 Dependencies
s01–03 safely integrated. Real harness checks require new exact approval for adapter closure/model/auth/private roots/turn and spend bounds. No ACP merge/push/release permission. Normative: PLAN.md, build-integration-contract.md, ADR0004, IMPACT_LATEST, e04-TEST_PLAN_LATEST and THREAT_MODEL.

## 9 Module purpose callers and contracts
PLAN/IMPACT define exact existing callers and preserved contracts. Parent alone changes control/daemon/client/CLI/top-level TUI/manifests/global ledgers. Concrete ACP broker owns protocol/process/identity/retention; Reason for Depth: prevent callers independently reconstructing races and outcomes. ACP editor owns bounded identity-bound drafts; Reason for Depth: typed human actions must not become raw terminal input or optimistic history. Authority admission is one memory-only target→ACP-state transaction, no I/O/wait/join or reverse/topology lock edge.

## 10 Requirements
**MODIFIED — Complete ACP acceptance and truthful fallback.** **Before:** e03 complete-broker/native-fence acceptance remains failing; signed delivery is explicitly limited. No runtime ACP acceptance exists. **After:** ACP acceptance is an independently proved authoritative admission/session path. Terminal-only tools remain explicitly best-effort and e03 native fence/failed acceptance/history stay unchanged; thin integration API is still deferred.

All applicable contracts in PLAN/ADR apply from first exposure: exclusive control; force never implicitly interrupts/kills; ordinary pending cancel revokes old control/grants none/retains work; keep/withdraw preserves human control with external policy; every application-input route gated; no replay; same exact live session and genuine interactions; bounded resource/privacy/owned cleanup. ACP admission is not provider application or completed acceptance.

## 11 Interfaces and data
Use the exact target variants, eight ACP operations, current-control/captured-generation/call identity, canonical projections, worker API and file ownership in build-integration-contract.md. Official schema 1.12.0/default-features=false [OK], stable v1, no SDK runtime/executor or generic plugin interface. Native terminal/agent-* closed variants and OMP agent-settled remain unchanged.

## 12 Main flow
Run integrated local CLI/actual PTY conversation across prompt, genuine questions/allow/deny and running ownership transitions. Observe resource/privacy/cleanup failures under adversarial load. Request exact live approval, run both real adapters against the same surfaces, record safe outcomes/pins/model/spend/cleanup, and retain any failing row until genuine success.

## 13 Alternate and error flows
Wrong target/authority/generation/scope/revision, stale or duplicate actions fail without target effect. Unavailable capability/schema/auth gives explicit error, not automatic fallback/login/approval. EOF/overflow/unconfirmed delivery retain truthful unknown and exact original IDs. Never infer agent application from pipe write or interruption from notification/error/timeout. Late lifecycle events cannot resurrect a closed target.

## 14 Nonfunctional constraints
PLAN resource budgets and pre-accumulation/count+byte bounds are mandatory, including in-flight/reserved work. Single nonblocking FIFO application consumer, dedicated blocking writer, continued stderr drain. Runtime state only, metadata content-free, incremental transcript gaps explicit. Escape/control data is inert in the host terminal. New live runs/global daemon/config/auth changes/main publication are not authorized here.

## 15 Implementation steps
1. Exercise integrated actual CLI/controlling-PTY prompt, genuine questions/permissions, running ownership and stale-input rejection with observed cleanup; write redacted complete local matrix. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s04_ -- --nocapture`
2. Prove complete broker resource/privacy/owned-process boundaries under hostile frames, stalled traffic and concurrent snapshot/ownership/shutdown; no false settlement or unrelated cleanup. → verify: `cargo build --example acp_fixture && cargo test --test acp e04s04_ -- --nocapture`
3. Run full workspace/default preflight, OMP/Zsh/independent showcase and final revisit of native buffer/application fence; document best-effort terminal fallback without altering historical e03 failure/caps. → verify: `cargo build --example acp_fixture && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo check --workspace --all-targets && cargo build --workspace && cargo fmt --check && bun test integrations/omp && zsh -n integrations/zsh/adapter.zsh && cargo test -p terminal-showcase --test multiplexer independent_showcase_runs_inside_amux_and_checks_real_cursor_replies -- --ignored --exact --nocapture`
4. Under the user's subsequent live authorization replacing Claude with Grok, run fail-closed ignored e04s04_live_adapters against real pinned Codex and Grok adapters using existing OAuth subscriptions, genuine questions/allow/deny, same-session handoff, bounded cheap/free supported models, redacted evidence and owned cleanup. Failed/unsupported rows remain failing, never skipped-to-pass; no API-key or premium-model fallback. → verify: `cargo test --test acp e04s04_live_adapters -- --ignored --exact --nocapture`

## 16 Verification script
Run each command from the ACP worktree after implementing its actual cases; zero-case filters are failure. Use private daemon/socket/project/HOME roots, deterministic real stdio peer and explicit barriers. Launch built CLI and actual controlling-PTY attachment outside test-only proof; observe expected same-session content/dispositions and absence of stale marker effects. Observe owned child/descendant/worker/socket/root cleanup before fallback and unrelated sentinels alive. Record case names/counts and redacted results. The ignored live command fails without a run-specific separately approved harness/model/auth/spend record and actual both-adapter outcomes; it is never an automatic skip/pass. Until approved/exercised live task/story remain failing.

## 17. Acceptance criteria
1. **SC-e04s04-P0-01** — Given all three local slices integrated; when actual CLI and rendered PTY complete prompt/question/permission/handoff/conditional/stale-input scenario; then same session/canonical work and visible effects prove behavior beyond unit tests.
2. **SC-e04s04-P0-02** — Given the subsequent explicit Codex/Grok live OAuth authorization and pinned isolated adapter/auth/model roots; when both real adapters each exercise genuine API/human question, allow/deny and takeover with cheap/free supported models; then each original session/turn survives and evidence includes exact dependency closure/model/bounded run/cleanup; fixtures or old provider runs cannot substitute. Claude is excluded from this live matrix by user direction.
3. **SC-e04s04-P0-03** — Given full existing runtime/workspace consumers and original native bug; when all regressions/showcase and final native-fence review execute; then existing boundaries remain passing while unresolved native settlement is explicitly still failing, not relabelled fixed by ACP.
4. **SC-e04s04-P1-04** — Given complete broker under hostile/stalled traffic and owned resources; when concurrent ownership/snapshot/shutdown plus privacy/cleanup checks run; then bounded responsive truthful outcomes, no content leaks/unrelated impact and only safely integrated owned sessions/tabs/worktrees retired.

## 18 Out of scope
No generic non-ACP integration API, provider trait, retry policy, restoration/persistence, remote protocol, unstable ACP v2, replacement stock Zsh/OMP behavior or retroactive e03 fence/review pass. No unrelated refactoring, global daemon/config/auth changes, ACP merge/push/release. Real compatibility is an outstanding separately gated acceptance task, not replaced by the deterministic peer.

## 19 Risks
P0 authority/revoke/response identities and process cleanup; P1 untrusted rendering, privacy, capacity and schema semantics. Tests assert actual consumer effects and state transitions, not source/wiring/default/wording echoes. Fresh code/security audit must show no new unresolved findings in affected paths before completion. Review/compile/transport success is not runtime acceptance.

## 20 Evidence and handoff
Evidence destination: specs/verifications/e04s04-build.md (created only with actual results). All tasks/scenarios start failing. Record exact commands, named cases/counts, runtime surface observations, pins/cap boundaries and owned cleanup; no real content/auth/env/raw tracing. Next story consumes the integrated contract, never unmerged worker assumptions. Thin non-ACP API remains deferred; e03s08 stays failing.

Current gate: authorized supplemental1/global6 on741fe4a clears the prior reply-budget defect but fails on E04S04-OPS-S1-01, definite retained-content non-admission without correlated native capture recovery. Fresh existing native2/Grok5/CLI checks pass; a separate actual CLI confirms this new rejection trigger and same-grant deliberate correction, not the native latch itself. See specs/verifications/e04s04-build-review-supplemental1.json and e04s04-retained-admission-cli.json. All tasks/story remain failing. Original5/5 and supplemental1/1 are consumed without reset; no automatic extra review or publication.

Subsequent direct repair: E04S04-OPS-S1-01 is locally fixed and verified without another reproduction/review cycle. Actual controlling-PTY correction49/F7 cancel32 pass on the original grant/session/RPC with cleanup. Resource-bounded full1803 passes402workspace/291default and strict/runtime gates; initial parallel openpty failure1799 is retained. See specs/verifications/e04s04-rejection-recovery.json. Historical review outcomes remain unchanged; all tasks/story still await changed-source independent/human decision, with no additional live calls or publication.

Current delivery decision: the user explicitly accepts the verified repair and directs local main integration. Actual runtime evidence above remains source-attributed; the historical independent FAIL reports and original5/5 plus supplemental1/1 caps are unchanged. All four tasks now pass their actual runtime gates with this post-cap human delivery decision, not an invented independent PASS. See specs/verifications/e04-main-landing.json. No new live call, push, tag, release or global change is authorized.
