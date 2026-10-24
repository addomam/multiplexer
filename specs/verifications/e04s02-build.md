# e04s02 Build evidence

Status: **PASS — local S02 runtime, all six task gates, self-audit and independent round1 A/B100/100 with zero must-fix/should-fix and zero supported security findings.** Safe feature integration and owned-worker retirement follow. Live compatibility, S03/S04, ACP main publication and historical native acceptance remain separate; no compile/fixture/Plan result substitutes for actual runtime proof.

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

## Actual CLI/controlling-PTY driver baseline

While the workers implement S02, the parent exercised the real S01 binary through a private daemon and actual controlling PTY with an independent continuous terminal drainer. Attempt3 observed API arithmetic42, a local unsent native draft with no additional command, deliberate human Enter reaching57/count2 in the same session, unchanged native terminal IDs, and normal TUI/Zsh-host/daemon exits0. Termios restoration, peer absence before fallback, owned-worker joins and private-root removal were observed; no fallback signal or drainer error. Full synthetic rendered before/after surfaces and facts are in e04s02-runtime-driver-baseline-attempt3.json.

Two driver failures are preserved rather than relabelled: attempt1 used literal q after the prefix instead of Ctrl-Q and opened picker search; its pinned owned host required scoped fallback after daemon shutdown. Attempt2 added cooperative-only `when` to the closed force request and was correctly rejected. Their original records and separate subsequent no-holder/peer-absence/root-removal receipts remain e04s02-runtime-driver-baseline-attempt{1,2}{,-cleanup}.json. Only these driver mistakes were corrected; no product code was changed for them.

This establishes the runtime driver, not genuine-form/permission acceptance: it ran S01 code with forms unavailable. All S02 tasks and scenarios remain failing until integration, their actual intended cases, human interaction proof and fresh review gates succeed. No provider adapter, authentication or model was launched.

Editor native completion receipt artifact://1209 supplies commitc2a12d660e6a07e20295fcc432093f10675e752a, only src/tui/agent.rs. Its full result is preserved in e04s02-editor-delivery.json. It deliberately ran no checks; the backend/helper is not yet integrated. Worker completion is not acceptance, and its worktree/session remains preserved until safe integration.

The separate private native ordering gate is also exercised with the real original TUI connection, not an API impersonating an attachment: it paused the captured native prompt before daemon admission with42/count1 unchanged, then transmitted that original current action and observed57/count2 in the same session. Its acceptor/forwarders and socket were cleaned, termios restored, peer/owned workers gone before fallback, unrelated sentinel survived until deliberate fixture cleanup, and host/daemon exited0 without fallback. See e04s02-native-gate-baseline-attempt2.json. Its first attempt's probe-only104-byte Darwin Unix-socket-path failure and later cleanup are retained separately; the gate now uses the shorter n.sock suffix. No product fix or S02 acceptance is inferred. Captured private authority remains RAM-only and is absent from the evidence.

## Integrated candidate and observed repairs

Backend388da67b06b2b39daccbef14200d483d8b614c0b and editorc2a12d660e6a07e20295fcc432093f10675e752a merged without conflicts using a pending no-commit feature-branch merge. Full native delivery reports are e04s02-{backend,editor}-delivery.json. No worker verification is claimed; parent owns the integration delta and all checks. Both sessions/worktrees remain preserved until a safe integration commit.

First integrated command executed19 S02 tests:18passed/1failed, artifact://1235. The authentication test inspected its obsolete bootstrap tuple after the first session could already establish. Actual private CLI falsification confirmed only session_id changed null→first session, original auth work completed with the exact form scope/RPC2, old inspect correctly returned stale-target, and a new explicit prompt reached57/count1 in that same session; owned cleanup passed without fallback (e04s02-auth-ordering-rca.json). The test now resynchronizes its read tuple before inspecting retained work and asserts the original scoped RPC. No production identity checks were relaxed.

After that test repair: targeted1, all S02 integration19, ACP library22 and workspace372/30suites/10ignored passed, with strict workspace Clippy/check/build/fmt (artifact://1238). This is the pre-history-repair checkpoint, not final source acceptance.

Actual native smoke then exposed a real boundary bug not caught by those tests:64 completed forms plus one valid pending form exceeded the editor's64-total record limit, hiding its editor. Before evidence e04s02-history-ui-before.json retains the failed rendered surface. The view now uses the broker's shared bound of64 live/responding plus1024 completed records, still subject to4MiB content. After evidence e04s02-history-ui-after.json shows the65th genuine form edited/reviewed/accepted through native keys, no default/first-review submission, exact original RPC66 and same-session57/count65. Terminal modes, peer/owned workers, unrelated sentinel and private-root cleanup passed before fallback; no fallback was used.

Actual e04s02-rich-native.json exercises every known control with native keys/paste, complete scrollable review and explicit second F5: integer15, number1.5, false replacing true, Unicode text, enum intersection, titled enum, both multiselect forms and explicit unset of optional default99. Exact canonical typed values causally produce68/count1 in the original prompt/session. Defaults and first review do not submit; local pattern/format limitation is visible. Normal TUI/host/daemon exits0, termios restoration, peer/worker cleanup and sentinel survival are recorded.

## Genuine human/API interaction surfaces

- e04s02-permissions-native-api-attempt3.json: one persistent peer/session, native allow-once →42/count1, API reject-once →42/count2, API allow-always →57/count3, native reject-always →57/count4. Exact opaque offered IDs are retained; denied turns do not run the tool. Focus loss/highlight is not selection. Two attachments retire normally. Attempts1/2 retain driver-only canonical-selection-shape and border-aware row-selector mistakes, with separate cleanup evidence; neither is relabelled passing.
- e04s02-{form,permission}-{api,native}-first.json: four actual CLI/original-native-connection competitions. A private transparent proxy holds the genuine native response before daemon admission; API uses that captured authority only in memory. Either surface wins with one canonical response, actual stale-interaction loser and no losing accepted history. Form winners produce73 or57; permission winners deny at0 or allow at42. This is pre-daemon ordering, not an internal production admission test hook; deterministic module/public-daemon tests cover the ordering transaction.
- e04s02-bootstrap-native-attempt2.json: genuine initialize-scoped form at null session/RPC0, honest unknown agent identity, native edit/review/answer, original bootstrap completes and deliberate native prompt reaches57/count1 in the first session. Failed attempt1 incorrectly cycled an already-selected history panel; preserved without a product workaround.
- e04s02-native-form-{decline,cancel}.json: actual native F6/F7 return distinct Decline/Cancel, original prompt end_turn at32/22 respectively. Both leave the same peer alive until explicit close; neither sends session/cancel.
- e04s02-native-interrupt.json: actual Ctrl-C with two pending and two late requests. Peer validates exactly one session/cancel and every exact mandatory form-Cancel/permission-Cancelled response. No late question is selectable; application remains unknown. Original prompt stays incomplete/cancelling while its response is held, then only its cancelled stop reason confirms interruption. Peer survives until explicit lifecycle close.

All successful records above observe termios restoration, normal TUI/host/daemon exits0, owned peer absence and worker/proxy/drainer joins before fallback, unrelated sentinel survival and private-root removal. No live provider, auth or model contact occurred. Failed driver records retain their own actual fallback/cleanup results; they are not combined into successful terminal verdicts.

## Native schema/content boundary audit and repair

Two additional actual surfaces failed before repair:

- e04s02-native-schema-defaults-before.json: supported schema with a70-KiB editable optional default and70-KiB omitted optional field name incorrectly displayed unsupported controls. The local initial-schema budget was incorrectly the answer budget.
- e04s02-native-projection-budget-before.json: five retained interactions have4,194,176 canonical detail/selection bytes (128 below4MiB) but4,195,282 projected bytes including server metadata. The editor incorrectly cleared all response controls by charging those different quantities to the same ceiling.

The editor now bounds declared defaults/names under incoming content rather than submitted-answer size, excludes unset fields from draft accounting and charges a newly set field before insertion. Submitted values still pass the existing64-KiB budget and shared backend validator. Canonical broker retention stays4MiB; the view reserves512 additional bytes per bounded server-generated record for its fixed metadata, not extra agent content.

e04s02-native-schema-defaults-after-attempt2.json proves deliberate unset of the large optional default, omission of the large unset name, native review without submission, exact typed answer `{bonus:15}`, original RPC2 and same-session57/count1. e04s02-native-projection-budget-after-attempt2.json proves the same path at the five-record near-capacity boundary, original RPC6 and same-session285/count5. Both retire normally with no fallback and remove their private roots.

The first post-fix driver waited for a review-status footer outside the long scrollable viewport even though its captured surface already showed `REVIEW: F5 accepts`. Those failed after.json records remain; only the driver assertion changed to the actual visible header before attempt2. A separate constructor-only long Darwin socket path failed before any peer launched; e04s02-projection-probe-constructor-failure.json records the startup error, no open root holders and root removal. No product change was made for these driver errors.

## Current candidate gates

Final corrected source: artifact://1266 is one contiguous successful command covering workspace372/30suites/10ignored, strict workspace/all-target Clippy and check, workspace build, fmt, Bun8/49 and stock Zsh syntax. The exact approved story command separately passes22 ACP library cases and19 S02 integration cases, artifact://1269. Historical failures and pre-repair results remain above.

README/changelog describe actual form/permission keys, typed/unset/default behavior, exact authority and truthful delivery/cancellation boundaries. Both complete independent code reports pass100/100, zero must-fix/should-fix; static security has no supported findings. The six tasks were promoted only after their actual command/smoke/security gates. Live compatibility, S03/S04, ACP publication and the deferred non-ACP API remain separate.

While preserving verification metadata, a strict YAML check exposed existing duplicate `gate_trace` and `stories` keys in execution-status.yaml. Their earlier records are now explicitly named `previous_gate_trace` and `historical_stories`; the later active mappings keep exactly their prior last-key semantics. No historical values, caps or verdicts were discarded or reset. Strict unique-key validation then passed for both current ledgers and the S02 task ledger; all six tasks still fail pending review.

Independent security round1 reports zero supported findings and no separate proven P0 contract violation. Full report is e04s02-security-review-round1.json. It is read-only static review of current source/cross-file paths with parent runtime evidence, not independent execution, hash/diff recomputation, real-provider compatibility or a sandbox claim.

## Independent round1 completion and checkpoint

e04s02-build-review-round1.json records the exact AND rule and both100/100 scores. Original A/B reports, A's same-source32-check detail supplement and security report are retained separately. The supplement completes the requested rubric detail, not a new iteration; maximum5 and all S01/e03 counters remain unchanged. A had two Consider-only comments, B none; both are explicitly dispositioned without code changes: an unused cached parent-state field is not rendered/used for admission, and first-Space→true/second→false/F4→unset is the existing standard deliberate checkbox behavior, not a collapsed optional value.

No production source changed after independent review. README only drops its now-obsolete under-review sentence; ledger/evidence completion is non-runtime. Source/code comparison and scoped local feature commit precede retiring the two owned Herdr sessions/worktrees. No main merge, push, release or live approval is inferred.
