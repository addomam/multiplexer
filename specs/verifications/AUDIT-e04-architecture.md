# e04 architecture self-audit

Type: architecture-only self-audit. Context: ADR 0004 and `epics/e04-acp-agent-interaction/context.yaml`. Independent architecture quality passed round2 (96.875 / 100), evidence `e04-architecture-review-round2.json`, and user confirmed architecture with conditional Plan-to-Build authorization. Advisory clarifications are mandatory Plan review input. This is not READY FOR BUILD or runtime proof. Round1 failure remains recorded.

## Checks

| Area | Result | Evidence / boundary |
|---|---|---|
| User decisions | PASS | Amux-owned ACP view and ACP-only amux admission boundary recorded with explicit alternatives; future non-ACP API deferred. |
| Scope/deltas | PASS | Latest scope has before/after MODIFIED requirements, mapped proposed story IDs, exclusions and observable success criteria. |
| Native regressions | PASS at design level | ADR explicitly preserves PTY settlement rules, stock Zsh/OMP and legacy Codex operations; e03s08 remains failing. |
| Ownership race | PASS at design level | One bounded memory transaction under the authority lock; no check/enqueue interval or I/O under that lock. Already-admitted work is canonical, not reassigned after handoff. |
| Noncancelling handoff | PASS at revised design level | Original request/outcome mappings retained; no protocol cancel, discard, kill or session replacement on force/takeover cancellation. Conditional keep/withdraw remains human-preserving. |
| Protocol truthfulness | PASS at revised design level | Stable wire v1 distinct from schema/runtime versions; prompt completion/provider application distinct from admission; exact identities, FIFO consumer and selected/submitted/unknown labels specified. |
| Genuine interactions | PASS at design level | Real form elicitation and offered option IDs; canonical local decision, no losing optimistic history, explicit decline/cancel; request-scoped forms not assumed to be prompt-scoped. |
| Human input | PASS at design level | Separate typed view; drafts/actions bind target/session/generation/revision; no intercepted-input replay or raw-agent stdin. |
| Privacy/security | PASS at design level | No raw prompts/answers/auth/output in metadata/evidence, no secrets in new artifacts, no auto URL launch or unimplemented delegation capability; explicit argv without shell interpolation. |
| Dependencies | PASS for revised selection | Official schema 1.12.0 [OK], Rust 1.88.0/edition2024 verified from published manifest; default features off. Higher SDK runtime not selected because its transport/task queues are unbounded; process helper also conflicts with cleanup. No extra executor dependency. Locked transitive/toolchain/feature compatibility remains Plan verification. |
| Resource cleanup | PASS at revised design level | Owned birth-qualified processes; frame cap before accumulation/deserialization and count/byte bounds on every amux queue; no hidden SDK runtime queue. Explicit overflow/unknown outcomes, no outstanding-work eviction or locks across join. Real ACP proof remains pending. |
| Performance | PASS at design level | Bounded atomic commit; topology snapshots remain content-free, transcript incremental; slow clients do not join PTY processing. Exact bounds are Plan work. |
| Maintainability | PASS at revised design level | Concrete broker/view, bounded stdio worker and atomic admission each have Reason for Depth. Official wire models/envelopes reused; no generic JSON-RPC/provider/plugin framework. Shared-file integration owner exclusive. |
| Provenance / regression gates | PASS | Scope references resolve; seven YAML artifacts parse. Branch, old five-round exhausted review and failing e03s08/ACP-story invariants checked through actual parsed artifacts. |
| Baseline / publication | PASS | Signed main a46eaddeb79a9a42628d9cb39cd60945ece27fbb remotely verified. `artifact://707` default preflight; `artifact://731` 308 workspace tests/29 suites/10 ignored, strict Clippy/check/build/fmt, actual CLI help, eight OMP tests and Zsh syntax. |
| Phase discipline | PASS | New architecture/Plan reviews distinct from e03. No new runtime code, dependency install, live model call, global daemon/config/auth mutation or ACP publication. |

## Known limits, not waived

- Exact request/snapshot/form schema, numeric resource caps, launch ergonomics, locked package compatibility and runnable task ledgers belong to the next Plan. Oversized-frame/flood/stalled-consumer proof is mandatory; transcript truncation alone is not a memory-bound proof.
- Isolated-clone LSP returned no definition/references despite a source-observed caller; issue reported. Repair/recheck workspace registration before exported runtime-symbol changes. No such symbol changed here.
- No new ACP runtime has been exercised. Existing baseline proves regressions before code, not ACP correctness or provider compatibility.
- Historical native application fence and exhausted e03 review remain unresolved/unchanged. ACP is not a retroactive PASS.

No Fowler abstraction smell is deliberately accepted: broker/view own distinct cohesive boundaries; the admission transaction exists for one race. Concrete implementation still needs code review for data clumps, primitive obsession and lock/callback lifetime mistakes.

Rationalization rejected: treating a successful initialize, deterministic peer, pipe write or Plan approval as full real-agent interaction acceptance. Architecture and Plan require their own independent dual review and explicit phase authorization before Build.

## Round1 response

- Both reviewers failed the original bounded-pipes-plus-SDK runtime selection (96.875 and 93.75); the SDK's unbounded line/channel/task internals made the resource claim false. Corrected by selecting the official schema-only crate with a concrete bounded local stdio worker, not weakening resource requirements or forking upstream.
- Added explicit protocol-required Cancelled permission responses as part of authorized cancel admission, reserved response capacity, late cancelling-session permission handling and truthful form cancel. This is not automatic allow/deny.
- Applied all advisory clarifications: independent nonblocking application consumer, exact disposition labels, ordered closure/late-error handling, no raw transport tracing in evidence, and release of existing daemon topology span before ACP projections.
- Historical e03 counters/failures and new e04 story failing statuses remain unchanged. No production code, installs, models or runtime behavior added. Independent round2 subsequently passed with one advisory clarification; that report, not this self-audit, supplies the quality verdict.
