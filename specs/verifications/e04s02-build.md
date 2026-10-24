# e04s02 Build evidence

Status: **FAILING — genuine-form RED observed; backend/editor implementation dispatched in disjoint Herdr worktrees.** Every S02 task/scenario remains failing. No compile, fixture-only result or Plan approval substitutes for actual required scenarios and CLI/controlling-PTY verification.

## Verified predecessor and authorization

S01 completed in local integration4d74d58, completion/owned-retirement receipts8886c2b. Current production-code baseline is workspace343/30suites/10ignored with strict Clippy/check/build/fmt (`artifact://1171`), OMP8/49 and stock Zsh syntax, plus actual private CLI/controlling-PTY proofs. S01 raw final reviews and verified nonblocking P2 repair remain distinct; no independent post-repair PASS or sixth round is claimed. Both prior owned S01 Herdr workspaces/checkouts/shells were retired after integration, without force or discarded work.

The approved S02 story/tasks, Plan, threat model and test matrix are unchanged. Concrete implementation handshake is in build-integration-contract.md; parent owns all shared routing, manifests and ledgers. Product ACP live-provider/auth/model runs, global config/default daemon changes and ACP main merge/push/release remain separately gated. Coding workers are authorized.

## Test-only RED

Commit1c8ba34 changes only tests/acp.rs and tests/fixtures/acp_peer.rs. A real stdio peer emits a session-scoped integer-adjustment form while the original arithmetic prompt is pending. The tracer requires a deliberate typed answer through the exact canonical interaction before the real peer applies adjustment15 and reaches57/count1 in the original session; explicit close/reap and an unrelated sentinel are part of its intended passing outcome.

Command:

```sh
cargo build --example acp_fixture && cargo test --test acp e04s02_api_form_answer_resumes_exact_pending_prompt -- --exact --nocapture
```

Observed in the parent implementation tree: **1 executed,0passed,1failed**, exit101, `pre-S02 broker rejected the genuine form instead of publishing it`. Repeated by parent against the exact committed RED in isolated `/Users/addomam/projects/amux-acp-s02-backend` before its coding session started, with the same1/0/1 result. This isolated check reused the parent target directory; no production S02 code existed in either run. No zero-case filter or successful RED claim.

The installed TDD helper's package-root/HEAD~1 limitation is unchanged from S01; direct isolated committed execution is the proof. The RED remains an ancestor rather than being combined with implementation.

## Exclusive responsibilities and owned sessions

- Parent: src/control.rs, daemon/client/CLI/top-level TUI/control/view routing, manifests, all specs/docs/ledgers, verification and integration. Integrate both tracks before executing the new daemon.
- Backend: agent e04-s02-backend, workspace w1S/tab w1S:t1/pane w1S:p1, branch feat/acp-s02-backend, `/Users/addomam/projects/amux-acp-s02-backend`; only src/acp.rs, tests/acp.rs, tests/fixtures/acp_peer.rs.
- Editor: agent e04-s02-view, workspace w1T/tab w1T:t1/pane w1T:p1, branch feat/acp-s02-view, `/Users/addomam/projects/amux-acp-s02-view`; only src/tui/agent.rs.
- Both start from1c8ba34 and use openai-codex/gpt-6.1-sol with high reasoning for protocol/scope/cancellation/identity and draft races. Bounded documentation remains inline. Neither worker runs build/tests/lint/formatters; parent owns post-integration checks and actual runtime proof. No unmerged work/session is retired.

Initial Herdr creation without explicit `--cwd` selected its ambient logical repository and rejected1c8ba34. Actual Git common-directory facts show the implementation and primary clone share `/Users/addomam/projects/amux-acp/.git`; explicitly passing `--cwd /Users/addomam/projects/amux-acp` created the correct worktrees. Do not treat shell cwd alone as Herdr repository selection.

## Tooling boundary

LSP symbols resolve, but known compiled Broker::prepare/commit and AgentView::event references remain empty at exact declaration positions after scoped rust-analyzer reload. The device cannot send workspace-folder notifications (raw request returns-32601). Reported this limitation; empty results are not proof of absent callers. No exported signature rename is planned; parent retains known shared callers and verifies compiled/runtime routes. Existing concrete interfaces are reused.
