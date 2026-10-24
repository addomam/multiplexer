# e04s01 Build evidence

Status: **PASS — e04s01 local acceptance; feature integration checkpoint pending.** Round5 meets the approved96.875/100 AND threshold with zero must-fix. Its sole nonblocking P2 is repaired and verified; raw A incorrect report remains unchanged and no independent post-repair PASS is claimed. Later stories, new live ACP harness/model/auth checks and ACP publication remain separately gated.

## Baseline and Plan gate

- Branch `feat/acp-agent-interaction`, isolated `/Users/addomam/projects/amux-acp-build`; original dirty checkout preserved.
- Published base `a46eaddeb79a9a42628d9cb39cd60945ece27fbb`; existing workspace preflight artifact `artifact://731`: 308 Rust tests, 8 OMP tests, strict workspace Clippy/check/build/fmt and Zsh syntax, actual CLI help.
- e04 Plan round3 independently 100/100, zero findings; complete reports in `e04-plan-review-round3.json`. Prior failed rounds and historical e03 acceptance/caps retained.
- User selected existing explicit ACP close and daemon shutdown only; shell close/remove must preserve sibling ACP views. Broader lifecycle UX deferred.

## RED tracer observed

Test-only commit `c8808f3` adds `tests/acp.rs`, standalone non-production `tests/fixtures/acp_peer.rs` and Cargo example declaration. It requires a real private daemon to admit an arithmetic prompt into one non-PTY ACP session, observe a stateful external file effect and matching canonical stream, then reap the owned peer before fixture fallback. It is not an echo/wiring-only assertion or real-model compatibility claim.

Command:

```sh
cargo build --example acp_fixture && cargo test --test acp e04s01_prompt_effect_and_stream_share_one_live_instance -- --exact --nocapture
```

Observed in implementation worktree: example/test compiled; **1 executed, 0 passed, 1 failed**, exit101, `unknown-operation: acp-open`. Repeated against exact committed RED in a separate detached worktree with the same result. The proof worktree was safely removed after the run; its commit remains the feature branch ancestor. No production implementation existed at either RED run.

Installed `verify-tdd-red-commit.sh` hard-codes its own package root and selects HEAD~1, so it was not run against the wrong repository/commit. Direct isolated-commit execution above is the actual RED proof; no helper PASS is claimed.

## Integration responsibility

Parent exclusively owns authority/daemon/client/CLI/top-level TUI/manifests/global ledgers. Backend worker edited `src/acp.rs`, `tests/acp.rs`, `tests/fixtures/acp_peer.rs` at `72e2aa0229ecc10eb4dd8dc4c89534a189a9c73f`; view worker edited only `src/tui/agent.rs` at `286694494abf5127680f89425389df21016f7158`. Both used separate Herdr sessions/worktrees and ran no mid-flight checks. Both commits are merged into the parent implementation tree; the integration merge is pending a verified commit. Keep both worktrees and sessions until safe integration.

## Integrated verification in progress

- First integrated compile succeeded, but the test harness aborted on an assertion inside fallible socket I/O and a second panic during cleanup (`artifact://974`). Return `UnexpectedEof` rather than panic; preserve the race's acknowledgement, independent owned-process reap and unrelated-sentinel assertions. The exact close/shutdown race then passed.
- The next run passed 19 cases and failed the flood case (`artifact://981`). Instrumenting its mode isolated `flood`; 512 successfully processed requests did not necessarily exhaust a bounded queue. Increase the finite fixture burst to 4096 so the unread reply pipe and writer queue actually fill; retain fail-closed, responsiveness and cleanup assertions.
- Integrated ACP suite then **20 passed** and library suite **5 passed** (`artifact://984`). Strict Clippy identified an avoidable boxed-job vector; replace it with three direct thread spawns and fixed handle storage.
- Initial full workspace verification **333 passed, 30 suites, 10 ignored**, strict Clippy/build/formatting passed (`artifact://986`), before the cleanup/status fixes. Current full verification after those fixes is **335 passed, 30 suites, 10 ignored**, 7 ACP library cases, strict workspace Clippy/build/formatting (`artifact://995`).
- Actual CLI/controlling-PTY smoke now **passed**, in `e04s01-cli-pty-smoke.json`: API total42 and human +15/total57 on one peer/session, local-only paste until Enter, revoked draft/status invalidation, observer paste/Enter creates no work, original stock-Zsh terminal identities preserved, resize/picker, restored terminal modes, TUI/host/daemon exit0 and explicit ACP-close peer `ESRCH` before fallback. Early failed probes incorrectly expected a noncumulative total, no initial Zsh terminal, and post-session-leader PTY validity on Darwin; all failed records are retained. The final probe uses a persistent private stock-Zsh controlling host.
- Initial failed smoke cleanup and its exact observed frame are in `e04s01-smoke-probe-correction.json`; subsequent private smoke cleanup acknowledged shutdown, reaped TUI/daemon with exit0 and independently observed peer `ESRCH`. No default daemon/config/auth changes or broad process signals.
- Self-audit found that failed process-identity capture could be labelled cleanup-complete and lose its resource reservation merely because its owner thread joined. A new invariant test failed on the false `true`; keep uncertain capture cleanup incomplete, retained and charged. Test the actual uncaptured pinned-group reap separately, including descendants and an unrelated live sentinel.
- The actual observer frame exposed a stale “Controlled here” status after takeover despite a correctly locked header/body. Invalidate that status when current local authority becomes unavailable; never replay input or reacquire.
- Exact story command (`cargo build --example acp_fixture && cargo test --lib acp && cargo test --test acp e04s01_ -- --nocapture && cargo test --test pty_control --test agent_interaction`) passed **7 library +20 s01 +57 legacy cases**, 1 legacy ignored (`artifact://1002`). OMP `bun test`: **8 passed, 49 assertions**. Zsh adapter syntax passed.
- Source self-audit initially passed and fresh e04s01 round1 independent general reviewers A/B plus a security specialist were dispatched against the frozen current working tree. Static security found **no blocker at confidence ≥8**, with explicit limits, retained in `e04s01-security-review-round1.json`; it is not story acceptance.
- Post-dispatch contract check found a public wire-name mismatch: normative ACP ownership uses `settlement_boundary`, but the current helper and three native consumers use `boundary`. Preserve source freeze while both code reports finish; migrate implementation/callers to the approved field, verify again and retain the failed audit finding before acceptance. Do not change the settled contract to fit code.
- All four owned private CLI/PTY proof roots were removed after saved evidence, daemon/host/client reap and peer disappearance observations, and a no-holder check. Worker Herdr sessions/worktrees remain retained until verified integration is committed; no unmerged work is discarded.

No task/story status has been promoted, no new independent Build/security PASS is claimed, and no ACP live-provider/model/auth check or ACP publication has run. Historical e03 acceptance failures and review caps remain unchanged.

## Post-suspend resumption

- Both independent round1 general reviews failed with `getaddrinfo ENOTFOUND api.x.ai`, without final reports/verdicts. Retained as an incomplete attempt in `e04s01-build-review-round1.json`; round/cap history is not reset. Endpoint DNS resolves again after resume.
- Migrated the approved `settlement_boundary` ownership field, force warning and all three native consumers; no compatibility alias and no contract rewrite.
- `artifact://1014`: 7 ACP library and 20 s01 cases passed. The native ownership target had 32 passing and 2 failing cases: `removed_terminal_consumes_original_shell_settlement_without_retargeting` and `native::native_cancel_grants_nobody_and_changed_force_facts_require_new_confirmation`. Chained Clippy/build did not execute after this failure.
- An isolated actual Unix-socket probe showed Darwin's nonblocking listener produces a nonblocking accepted stream: read returned `WouldBlock` in 2µs before the callback arrived. Explicit blocking mode then delivered byte120. `ShellReportGate` now clears that inherited mode before framed callback I/O; original deadlines and assertions stay unchanged. Targeted verification is pending.
- Actual resumed CLI/TUI reached API42 then human57 on one peer/session through the migrated field, but the TUI subsequently exited1 with `setting control response timeout: Invalid argument (os error 22)` before the next draft. Full synthetic failure frame retained in `e04s01-resume-smoke-failure.json`; not a passing full smoke. Parent explicitly shut down/reaped the private daemon0 and persistent host1 and closed the PTY FDs. Root retained temporarily for diagnosis.
- Independent bounded investigations cover native force warning/confirmation ordering and the socket-timeout error. No “flaky” dismissal, timeout increase, assertion weakening or speculative error suppression.
- The repaired callback gate passed both original affected cases individually. Native-force ANSI analysis established an unchanged repaint demoted previously completed eligibility; preserve it only across unchanged still-fully-visible repaints, and invalidate changed facts/resize/scroll/clipping/new panels/stale observation. Original controlling-PTY force case then passed1/1.
- Current full `artifact://1036`: workspace335/30suites/10ignored; strict workspace Clippy, all-target check, workspace build and formatting passed. Bun8/49assertions and Zsh syntax passed after repairs.
- Actual OS socketpair and private-daemon probes establish valid35s timeout EINVAL after peer closure/documented idle expiry. The exact original blocked interval remains unrecorded; no client source, timeout, retry or error-suppression workaround. `e04s01-daemon-idle-expiry-probe.json` records6.004s idle expiry and continued daemon responsiveness.
- Corrected continuously drained actual CLI/PTY smoke passed in `e04s01-resume-cli-pty-smoke.json`: same-session API42/human57,6.2s deliberate driver idle survives, local paste, revoked draft discarded, observer no work, approved-field canonical work panel, human deliberate force, native IDs preserved, termios restored, peer gone before fallback, TUI/host/daemon0 and drainer joined/no error.
- Both post-resume private roots were removed only after saved failure/success evidence, reaping/peer disappearance and exact no-holder checks. Herdr workers remain preserved pending integration commit.
- Source self-audit PASS now permits fresh blind A/B/security round2, dispatched as E04S01ReviewA2/E04S01ReviewB2/E04S01Security2. No story/task promotion while verdicts are pending.

## Round2 findings and repair verification

- Both independent code reports FAIL: A96.875 (1must/0should), B90.625 (1must/2should). Security has zero confidence-8 blockers. Original reports and B's omitted-consider clarification are retained without rescoring.
- Permanent persistent-socket regression failed before repair on the unknown `persistent` field. The late capture-failure regression failed before repair on null failure after an earlier close. Actual controlling-PTY probe independently reproduced busy-queue draft loss (`e04s01-queue-draft-before.json`).
- Repairs allow the existing transport boolean, latch critical cleanup uncertainty after close, return the original owned unqueued envelope without copying, and restore only the still-matching captured prompt draft. No automatic retry or fresh-authority substitution.
- All unscored comments are dispositioned: canonical outcome-object rendering fixed; observed auth response survives a failed follow-up setup enqueue; empty cleanup-complete failed opens roll back through the existing removal event, while concurrent admitted-close history/unknown cleanup remain retained. Ended eviction intentionally emits removal rather than guaranteeing an extra final projection; bounded retention and no-live-mapping eviction remain unchanged.
- `artifact://1069`: 9 ACP library/21 integration passed, then native33/34 failed on restoration resize; strict downstream checks did not run. `artifact://1074`: 11 ACP library and the instrumented native case passed1/1; strict Clippy found2 mechanical nonminimal-bool conditions, since corrected. No failing run is relabelled.
- Current complete `artifact://1080`: **340 workspace tests,30 suites,10 ignored**, strict workspace Clippy, all-target check, workspace build and formatting passed. OMP8/49 and Zsh syntax passed.
- `e04s01-queue-draft-after.json`: actual private CLI/continuously drained controlling PTY retained the rejected draft; positive original-connection focus-loss/gain FIFO barriers establish no automatic replay; deliberate Enter produces total57/count2 in the original session after total42/count1. Termios restored; TUI/host/daemon0; peer absent before fallback; no fallback signals; proxy/drainer joined. A shutdown-side proxy BrokenPipe is retained, not hidden.
- Both queue-proof roots were removed only after saved evidence, independent reaping and exact no-holder checks. No live adapter/auth/model, global config/default daemon or main publication operation occurred.

## Round3 source freeze and late surface observation

Fresh code A/B and security review3 were dispatched against the frozen source in `e04s01-round3-source.json`. Security3 reports zero confidence-8 findings; full report/limitations are retained. Both code reviewers received the same actual late-frame observation: the completed second prompt still shows the earlier busy/not-queued chrome and capture-time not-yet-admitted notice. Source remains frozen until both code reports finish; repair and new actual-surface verification are required before acceptance. All21 s01 effects/bounds and340 workspace tests/strict gates remain observed facts, not a substitute for that UI correction.

## Round3 findings and current repair verification

- Full reports `e04s01-build-review-round3.json`: A96.875 (1must/0should/1consider), B90.625 (1must/2should/2consider); FAIL. Security3 has zero confidence-8 findings. Source unfroze only after both code reports completed.
- Shared status finding: successful deliberate local enqueue now clears the superseded local rejection; stable capture notice refers to canonical work without asserting admission. Old late replies cannot erase a newer error.
- Inspect finding: `e04s01-inspect-before.json` reproduces actual unsent-draft/transcript loss through a real daemon stale-target rejection. Retain the last confirmed projection on read error, render that failure separately, and rewind the cursor on actual identity change. Current authority changes still discard stale drafts.
- `e04s01-inspect-after.json`: actual continuously drained controlling PTY retains both draft and total42/count1 transcript through rejection/recovery. Positive original-connection focus FIFO barriers show no automatic replay; deliberate Enter produces57/count2 in the original session. Read-error chrome clears on recovery.
- `e04s01-queue-status-after.json`: actual delayed first admission reply reproduces local busy rejection; draft retained, positive FIFO barrier/no automatic replay, deliberate retry completes57/count2, and observed header/footer no longer contradict canonical completion.
- Each PTY proof restores terminal modes, observes peer absence before fallback, joins proxy/drainer, and reaps TUI/host/daemon0 with no fallback signals. Teardown BrokenPipe remains recorded. All three private roots were removed after evidence and exact no-holder checks.
- Late-startup test now truthfully proves responsive close while the peer reply is held; no meaningless post-reap release. A separate memory-only regression drives the production consumer with a complete response already dequeued under the held authority ordering point, ends the lifetime before application and proves no session publication plus released in-flight ingress charge. It does not claim a killed peer emitted a response.
- Initial new-unit run `artifact://1111` failed11/12 because it expected process-worker completion in a memory-only race. Correct expectation is nonlive `closing` with no session; actual process cleanup/ended/reap assertions in integration remain unchanged. No downstream commands ran after that failure.
- Removed the writer-budget test's manual decrement/success tautology; retained its real in-flight charge rejection boundary.
- `e04s01-label-boundary.json`: actual CLI rejects each residual bidi/line-format name with invalid-request and publishes no extra target; normal peer closes/reaps and daemon exits0. Bounded picker/header and broker metadata use consistent sanitation. This record is CLI-only, not visual proof.
- Specific repaired run `artifact://1113`: **12 ACP library +21 s01 integration passed**, actual binary/fixture built. Complete `artifact://1121`: **341 workspace tests,30 suites,10 ignored**, strict workspace Clippy, all-target check/build/fmt passed. Bun8/49 and stock Zsh syntax passed at the discovered integration paths; the initial nonexistent-cwd invocation ran neither check.
- Key-pattern scan of affected ACP source and new runtime records found no matches. Source self-audit is ready for fresh blind A/B plus security round4; cap5 and historical e03 counters remain unchanged. S02 is still gated.

## Round4 pass and final corrected candidate

- Complete blind reports `e04s01-build-review-round4.json`: A100/B100, zero must/should; A has one consider. Security4 has zero confidence-8 findings, with all static-only/process/dependency/live limits retained.
- A's consideration is real: on first read after selecting an ACP view the cache is empty, so an error cannot truthfully claim a retained projection. `e04s01-first-inspect-before.json` reproduces that actual CLI/PTY surface through real daemon rejection.
- Fixed only the banner's cache-availability context. Keep the old-identity reset: retaining another agent's projection/binding on selection would be unsafe. Successful cursor0 inspection remains the only source of new canonical identity/history.
- `e04s01-first-inspect-after.json`: actual first failed read shows no confirmed projection, then recovers the original canonical42/count1 session without replay. `e04s01-inspect-final.json`: same-view rejection still retains draft/history, recovery clears the banner, deliberate Enter produces57/count2 in that same session.
- Both after-fix PTY proofs and the before-fix proof restore termios, reap TUI/host/daemon0, independently observe peer absence before fallback, join proxy/drainer and use no fallback signals. Teardown BrokenPipe is retained; all three roots removed after exact no-holder checks. Temporary probe source has been removed.
- Final corrected `artifact://1142`: **341 workspace tests/30suites/10ignored**, strict workspace Clippy, all-target check/build/fmt; actual CLI/fixture built first. Bun8/49 and stock Zsh syntax pass. Scoped final source/capture key-pattern scan has no matches.
- Round4 remains a genuine PASS of its exact manifest, not a review of this later change. Source self-audit passes for the fifth/final independent A/B/security round. No cap reset, s01 promotion or S02 start.

## Round5 threshold and verified responder repair

- Complete raw reports in `e04s01-build-review-round5.json`: A96.875,0must/1should; B100,0must/0should. Security5 has zero confidence-8 findings on the recorded frozen source. The approved numerical AND rule passes. All five attempts and prior round4 PASS remain retained; no sixth review or counter reset.
- A's P2 is genuine: during explicit cancellation, malformed or foreign-session permission params closed the instance and left original work unknown. Both distinct regression cases fail before repair (`artifact://1165`,0/2), not merely a static inference.
- Repair preserves the known inbound request ID and reserves an invalid_params response instead of unwinding the consumer. Correct late permissions still receive Cancelled; the original prompt response alone confirms cancellation. Existing egress capacity failure semantics are unchanged.
- Both new regressions pass after repair, plus the existing exact explicit-interrupt case. Full `artifact://1171`:343 workspace tests/30suites/10ignored,12 ACP library/23 S01/57 legacy; strict workspace Clippy/all-target check/build/fmt. Bun8/49 and stock Zsh syntax pass.
- Actual built CLI proof `e04s01-cancel-invalid-permission-cli.json`: both bad-params/scope paths receive invalid_params; original cancellation confirms, follow-up on the same live session reaches57/count2, native IDs stay unchanged. Owned peer absent before fallback, workers joined, unrelated sentinel alive, daemon0, no fallback signals; private root removed only after no-holder check. This is synthetic local peer compatibility, not live-provider proof.
- The initial precautionary human-decision hold was cleared after reading the exact approved AND rule: no zero-should-fix or mandatory post-P2 round is added. The verified later source is recorded separately in `e04s01-post-review-source.json`; no independent review of that repair is claimed.
- All six S01 tasks now pass their named exercised scenarios and local runtime gate. Story completion and Herdr retirement follow the safe feature checkpoint. No S02 implementation, live run, main merge/push or release has happened.

