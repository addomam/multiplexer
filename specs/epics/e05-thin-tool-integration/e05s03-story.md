---
story_id: e05s03
title: Ownership handoff and truthful recovery
maturity: 3
type: feat
context: specs/epics/e05-thin-tool-integration/context.yaml
risk: P0
bcps: 8
status: failing
---

## 1 Identity
e05s03; e05-only initiative. BCPS8, risk P0. Proposed contract/Plan, no implementation or runtime PASS.

## 2 User story
As a tool author or human/API controller, I can tool targets join the same authority/admission boundary without changing native settlement or ACP public contracts.

## 3 Problem
Existing authority/canonical interaction guarantees apply to ACP and native paths, not yet the new closed Tool target.

## 4 Goal
Tool targets join the same authority/admission boundary without changing native settlement or ACP public contracts.

## 5 Scope
ToolV1 wire contract, exact live identity, existing authoritative admission and canonical human/API view; relevant scenarios below.

## 6 Actors
Integration child owns one real tool session; daemon owns authority/canonical work; original native attachment and external controller act through current private handles.

## 7 Preconditions
Explicit Build approval after fresh dual Plan pass; isolated feat/thin-integration-api worktree and actual baseline/LSP navigation. Live/provider/global/main gates remain separate. Keep all tasks failing until their real verification and smoke/cleanup succeed.

## 8 Dependencies
e05s01, e05s02. Normative: PLAN.md, ADR0005, ToolV1 contract, IMPACT-e05, e05 test plan and threat model. Parked by explicit user decision; no implementation.

## 9 Module purpose callers and contracts
Use exact file/caller/interface responsibilities in PLAN/IMPACT. Parent alone owns shared broker/control/daemon/client/CLI/top-level TUI/manifests/ledgers. Two concrete codecs, one canonical/process boundary; no second arbiter, extra bridge process or generic provider framework.

## 10 Requirements
**MODIFIED. Before:** Existing authority/canonical interaction guarantees apply to ACP and native paths, not yet the new closed Tool target. **After:** Tool targets join the same authority/admission boundary without changing native settlement or ACP public contracts.

All preserved invariants apply from first exposure: force never interrupts/kills; ordinary pending cancel grants none/retains work; conditional keep/withdraw preserves human control; every mutation gated; no replay; genuine original responders; truthful unknown; bounded private runtime and owned cleanup. Existing ACP public contracts and native fence/caps unchanged.

## 11 Interfaces and data
Exact ToolV1 envelopes/methods/capabilities/scopes/IDs/limits/winner rules and closed tool-* operations are in ../../contracts/tool-integration-v1.md. No ACP implementation or internal broker imports required from consumer. Tool target kind cannot be a PTY or ACP alias.

## 12 Main flow
Tool targets join the same authority/admission boundary without changing native settlement or ACP public contracts. Observe actual arithmetic/file effects and unchanged instance/session/original responder, not a mock echo or transport write.

## 13 Alternate and error flows
Wrong target/kind/control/generation/call/scope/revision, unsupported capability/schema, overflow/EOF and ambiguous delivery fail explicitly with retained actual state. No retry/replacement/automatic answer or fabricated application success. Definite non-admission permits only a new deliberate correction.

## 14 Nonfunctional constraints
Wire/count/byte/in-flight/retention limits and reserved cancellation/close capacity apply before accumulation/publication. Memory-only authority→broker transaction, no I/O/join/topology inversion. Content inert/private, raw auth/content not public; cleanup of original endpoints/workers/birth-qualified owned processes observed before fixture fallback.

## 15 Implementation steps
See matching task ledger for RED boundary regressions, implementation, actual CLI/PTY smoke and compatibility/evidence. Each step has risk, scenarios, actual future verify command and failing status. Tasks are not passes merely because a filter executes zero tests.

## 16 Verification script
After creating named real cases: `cargo test --test tool_integration e05s03_ -- --nocapture --test-threads=1`. First list cases and require every mapped name with nonzero execution. Run actual built CLI/API/controlling-PTY changed path outside test-only evidence, then current full default/workspace and inherited regression commands. Private roots only; preserve failures and source fingerprints. Live case is ignored without separate explicit approval and does not become a pass by skipping.

## 17 Acceptance criteria
1. **SC-e05s03-P0-01** — Under blocked transport, revoke-before-commit cannot reach tool and commit-before-revoke remains retained/deliverable without revalidating a later owner. Case: `e05s03_admit_revoke_orders`.
2. **SC-e05s03-P0-02** — Cooperative yield/explicit accept can finish while original question remains outstanding; force sends no interrupt/close; ordinary pending cancel revokes old control, grants nobody and retains original work. Case: `e05s03_handoff_force_cancel`.
3. **SC-e05s03-P0-03** — Only original live attachment activity/keep/withdraw preserves human conditional consent; borrowed/forged provenance and late acceptance cannot steal control. Case: `e05s03_conditional_provenance`.
4. **SC-e05s03-P0-04** — Physical keys/repeat/paste/mouse/focus/double-prefix and old form drafts cannot mutate after revoke or replay on reacquisition; fresh deliberate current input still works. Case: `e05s03_native_stale_input`.
5. **SC-e05s03-P0-05** — Only explicit authorized exact foreground prompt interrupt sends a tool interrupt; requested/terminal cancelled/error/EOF distinctions cannot cancel newer work or fabricate success. Case: `e05s03_interrupt_identity`.
6. **SC-e05s03-P1-06** — Lost caller reply, stream gap and hostile text recover through canonical exact identities without replay/false application acknowledgement or host terminal effects. Case: `e05s03_disconnect_gap_render`.

## 18 Out of scope
No opaque native-buffer fix/early spike, generic plugins/remote protocol, persistence/restoration, scheduling/automatic reacquisition, auth/global config changes, arbitrary language SDK distribution or unauthorized provider/main publication.

## 19 Risks
P0 identity/admission/revoke/first-winner/resource lifetime; P1 schema/view/privacy/provenance. Strong reasoning for protocol/race integration, cheap tools only for bounded mechanical/docs work. Independent workers never overwrite shared files or run checks mid-flight.

## 20 Evidence and handoff
Create specs/verifications/e05s03-build.md only with actual results: exact source, commands, case counts, observed same-instance effects, private cleanup and preserved unknowns. Every task/scenario starts failing. Next story consumes integrated contracts, never unmerged assumptions.
