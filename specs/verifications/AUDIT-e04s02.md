---
type: feat
context: specs/epics/e04-acp-agent-interaction/context.yaml
story_id: e04s02
scope: e04s02-genuine-forms-and-permissions
status: pass
---

# e04s02 source self-audit

S02 local runtime, self-audit and independent round1 gates PASS: both code reviewers100/100, zero must-fix/should-fix, security zero supported findings. Both Consider-only comments have explicit no-change dispositions in e04s02-build-review-round1.json. All six tasks now pass their actual command/smoke/security gate. Safe local integration and owned-worker retirement follow; live/S03/S04/publication and historical caps remain separate.

## Review priority and exercised candidate

The bundled churn helper is absent. Scoped integration diff from verified predecessor8886c2b prioritizes src/acp.rs (+1500/-58), src/tui/agent.rs (+1174/-4), tests/acp.rs (+1006), then tests/fixtures/acp_peer.rs (+395/-2). Review focused on immutable admission, responder reuse, scope lifetime, schema/default validation, cancellation reservations and bounded native projections, not an unrelated module split.

Current corrected production source passes artifact://1266: workspace372/30suites/10ignored, strict all-target workspace Clippy/check/build/fmt, Bun8/49 and stock Zsh syntax. Exact approved story command artifact://1269 passes22 ACP library cases and19 S02 integration cases. The source manifest is e04s02-round1-source.json.

Actual private CLI/controlling-PTY evidence is indexed in e04s02-build.md: all known typed fields; native/API permission allow/reject once/always; four genuine original-native/API response competitions; bootstrap null-session form; deliberate decline/form-cancel; explicit interrupt with pending/late mandatory replies;65th question after completed history. Earlier surface proofs precede the final view-accounting correction; its changed paths have separate current-source before/after proof for supported large defaults/omitted names and near-4-MiB canonical projection. Do not label all captures the same source snapshot.

## Checklist

- PASS correctness/admission: target/instance/connection/session, captured control/generation, call ID and interaction/revision are checked before immutable response selection under the existing authority→ACP ordering point. First valid selection wins; no later owner check discards accepted work, no losing answer enters accepted history.
- PASS protocol identity/lifetime: exact typed inbound IDs remain distinct from outbound RPC scope. A write completion stamp permits legal ID reuse before delayed bookkeeping without releasing the new responder. Original initialize/newSession/auth/prompt scopes are retained through parent closure rather than rebound to the latest prompt.
- PASS schema semantics: raw supported schema checked before official default-on-error/untagged decoding; shared pure backend validation owns answer constraints. Required/optional/unset/default, false/zero/empty, enum intersection, Unicode length, finite numeric/i64 bounds and distinct multiselect counts have behavioral coverage. Pattern/format certification is explicitly not claimed.
- PASS permission/cancellation: only offered exact IDs are selectable. Native navigation/focus/detach never chooses. Form Decline, form Cancel and session interrupt stay distinct. Interrupt reserves notification/all mandatory replies before publication; selected replies are not duplicated, applicable late requests are nonselectable, malformed/foreign scopes still receive protocol errors. Only the original prompt stop reason confirms interruption.
- PASS resources/performance: incoming frame/schema/count limits and reserved/in-flight writer charges remain bounded; dedicated writer/FIFO do not block authority/topology. Live responders are not evicted. Native ceiling matches64 live plus1024 completed records and bounded server metadata separately from canonical4-MiB content. Initial declared defaults remain editable under incoming bounds;64-KiB answer admission is unchanged. Immutable interaction sizes/prepared display are cached rather than reserialized every frame. Reconciliation is bounded but not claimed asymptotically optimal; no numeric latency/throughput benchmark is fabricated.
- PASS locks/cleanup: no new authority/ACP reverse edge, process I/O, spawn, wait or join under those locks. Successful runtime probes observe peer absence/worker joins before fallback, unrelated sentinel alive, normal exits, termios restored and no-holder private-root removal. Failed driver records retain their own distinct cleanup and fallback outcomes. Worker worktrees/sessions remain preserved until safe integration commit.
- PASS security/privacy: snapshots/default inspection expose safe metadata, content only through explicit inspect; rejected schema errors use safe categories, not raw backend text. Host rendering neutralizes terminal controls. Scoped evidence scan found no control-ref/key/Bearer patterns. No provider auth/model run, global config/default daemon operation or main publication occurred. Independent HIGH-confidence security review is still required; this checklist is not its verdict.
- PASS supply chain: no S02 dependency/manifests changes; reuse approved schema-only ACP1.12.0, default features disabled, with no executor/runtime/provider abstraction/plugin layer.
- PASS provenance/scope: approved Plan/ADR/handshake and isolated test-only RED1c8ba34 remain retained. Worker file responsibilities stayed disjoint; parent owns integration/routing/docs/evidence. Original dirty user checkout is not part of this candidate. Native fallback fence and e03s08 remain failing.
- PASS maintainability/types: one canonical broker and one identity-bound editor; no compatibility alias, duplicate answer validator or raw terminal fallback. Existing concrete authority/client routes reused. Cohesive deep-module size is intentional per approved Plan, not the generic300-line/20-line heuristic, which is absent from project CONVENTIONS.md.
- PASS docs: README/changelog now document actual enabled forms/permissions, deliberate keys, exact identities, limits and unproved live compatibility. Build evidence retains all failures rather than repinning wording/default/source tests.

## F.I.R.S.T

| File | Fast | Independent | Repeatable | Self-validating | Timely |
|---|---|---|---|---|---|
| src/acp.rs unit boundaries | PASS: bounded in-memory transitions/owned fixtures | PASS: isolated broker/trees | PASS: explicit state/order, no provider | PASS: capacity, exact IDs, first-winner and failed-delivery invariants | PASS: story tracer precedes implementation; boundary regressions accompany this slice |
| tests/acp.rs + real stdio peer | PASS:19 current cases4.94s | PASS: private daemon/HOME/socket/projects and owned cleanup | PASS: barriers/observed effects, no timing-as-success | PASS: real arithmetic/tool effects, canonical dispositions, original RPC/session and sentinel/reap oracles | PASS: committed isolated tracer; parent auth-ordering error diagnosed before test correction |
| Native view changes | Actual controlling-PTY smoke rather than layout/wording tests | Private roots and continuous independent drainer | Explicit observed interaction/worker/file barriers | Exact causal effects and original IDs; no optimistic response echo | Real history/schema/projection failures preserved before parent repairs |

No per-function padding, mock-echo, source-text or incidental-default tests were added. Parent UI corrections use actual before/after surface proof per developer policy. The initial auth test was resynchronized to the actual session boundary without relaxing identity checks. No claim that every worker-added case had its own isolated historical RED commit.

## Red flags, omissions and explicit limitations

- Three native accounting defects were found by actual runtime, not inferred from green tests: total record count, declared-default/unset sizing, and metadata-vs-content ceiling. All have retained failing-before/passing-after evidence. Failed driver assertions/socket paths are separately identified and do not justify product workarounds.
- Generic bundled churn/TDD/trace/wiki scripts are not supplied here; absent helpers are not successful checks. Actual project commands and isolated committed tracer execution are the evidence.
- LSP symbols work; isolated-worktree references remain unavailable after reported reload/device attempts. Empty references are not interpreted as absent callers. Existing exported method signatures were not renamed.
- Fowler smells considered: protocol identity data clumps encapsulated in Target/Binding, boundary JSON primitive strings reflect pinned wire vocabulary, larger match blocks keep transactions visible. No actionable Mysterious Name, Feature Envy, Message Chains or Middle Man found in the reviewed delta; fresh reviewers may disagree.
- No claim of real adapter/model compatibility, quantitative performance threshold, Linux runtime, complete-broker acceptance or repaired terminal application fence. No review cap reset or inferred authorization. No checklist omission is being used to promote a failing task.

## Independent gate and disposition

Both complete A/B reports and A's same-source32-check detail supplement are retained separately; the supplement is not a new round. No production source changed after review. Cached request-parent state is not rendered or used for admission, so the first Consider item is not a current consumer defect. The second proposes a checkbox policy change: deliberate first Space selects true, second false, F4 unset; these already preserve all distinct typed states. Keep the standard toggle rather than add a no-op first press or speculative policy. No open must-fix/should-fix/security finding remains.

Strict ledger validation also preserved the previously duplicated earlier trace/story mappings under explicit historical keys rather than discard them; active last-record semantics and all review caps are unchanged.
