# e03 integrated Build evidence

## Scope, authorization and isolation

Implementation is uncommitted in `/Users/addomam/projects/amux-e03-build`, branch `feat/e03-agent-interaction`, base `6318845`. Parent owns shared daemon/client/runtime routing and final integration.

Native work ran in the owned herdr `w16:p1` session and `amux-e03-native` feature worktree. Codex work ran independently in `w17:p1` and `amux-e03-codex`. Workers never edited shared routing files.

Initial implementation and bounded fixes were transferred by assigned file ownership. Seeded shared files were checked unchanged before transfer. Worker reports are historical implementation reports, not independent runtime acceptance.

The user subsequently authorized the four exact real Codex checks and relevant installed harnesses with real auth and cheapest compatible models, preferring local/free inference. No sol is permitted for these live checks.

This did not authorize global daemon/config/auth mutation, commit, merge, push or release. Historical review caps remain unchanged. Approved Plan is not runtime acceptance.

## Mechanical and native evidence

| Exercised command or milestone | Observed result |
| --- | --- |
| Initial combined library/ownership/Codex tests (`artifact://68`) | Library 94 passed; deterministic Codex 18 passed, 1 ignored; live 3 ignored; ownership/native 22 passed, 6 failed |
| Post-fix workspace suite (`artifact://90`) | 298 passed, 10 ignored across 29 suites; native ownership target 29 passed; deterministic Codex 21 passed, 1 ignored |
| Post-Clippy corrections (`bg_72`) | Workspace/all-target Clippy with `-D warnings`, all-target check, workspace build and explicit independent showcase exited 0; showcase 1 passed |
| Rendered evidence privacy regression | `complete::rendered_evidence_redacts_wrapped_credentials_and_private_refs` passed; real VT decoding retains geometry/color while redacting wrapped synthetic credentials and private references |
| Private CLI configuration after real PTY forks | Two actual `amux config` calls exited 0 and persisted their distinct prefixes in the owned config only |
| Managed Zsh + ordinary OMP wrapper, real OAuth, `gpt-6-luna` | CLI exited 0; actual reply `AMUX_OMP_ACCOUNT_READY` appeared in the managed terminal; lifecycle reports included session/agent/turn start/end and shutdown |
| Final required target commands (`bg_93`) | Live fixture compiled with `--no-run`; deterministic Codex 23 passed, 1 ignored; ownership/native 29 passed |
| Final workspace suite (`artifact://259`) | 301 passed, 10 ignored across 29 suites |
| Final workspace Clippy/check/build (`artifact://259`) | All exited 0, including all-target Clippy with `-D warnings` |
| Final independent showcase / OMP / Zsh (`artifact://259`) | Exact ignored independent showcase 1 passed; OMP lifecycle 8 passed; stock adapter `zsh -n` exited 0 |

The earlier 298-test count predates the privacy, auxiliary-title and explicit-subscription regressions; the final 301-test result includes them. Ignored model tests were never enabled by a wildcard.

The six initial native failures were diagnosed, not suppressed: stale-view classification; blank queued-consent rendering; missing owner-yield action; obsolete wait label; wrapped command misidentified as a shifted body origin; and competing-view reservation timing.

Later failures exposed inherited `NO_COLOR`, a stale force panel, and a force test confirming before its fresh header was drawn. The real controlling-PTY suite subsequently passed all 29 cases without weakening ownership/consent or replay boundaries.

A full integration run exposed configuration-lock retention after closing a duplicated descriptor. A regression performing a real configuration write failed before explicit `LOCK_UN` in `ConfigurationLock::drop`, then passed after the fix.

The regression proves duplicate-descriptor retention. Attribution to a particular concurrent fork is inference; the actual post-PTY-fork CLI saves also passed.

The final regression run exposed an inherited signing dependency in the existing Git-common-directory acceptance fixture: its temporary commits invoked the user's 1Password signer and failed. Only those fixture invocations now specify `-c commit.gpgsign=false`; no global or repository Git configuration changed. The exact failing test then passed in 0.99 seconds, followed by the full passing workspace gates.

## Native-first Codex diagnosis and correction

All four exact account checks were initially executed serially with explicit `gpt-6-luna` and private copies of the approved auth file. All failed at fresh native-first join; none passed acceptance.

The real native frontend created a loaded, zero-turn thread. `thread/read` returned that live identity, but `thread/resume`, including `excludeTurns:true`, rejected it with `no rollout found for thread id`.

Pinned upstream source explains the difference: running-thread resume reads stored metadata before attaching; metadata-only thread read can use the live snapshot. This was not fixed by a hidden prompt, API-first conversation or upstream patch.

The observer initializes before native launch and binds the native-created live identity through metadata read. Automatic listener setup is best-effort; the later actual checks below established that it did not deliver the scoped events required by this observer.

That change progressed the actual check through authoritative turn identity and same-turn steer acknowledgement. The next observed failure was `native-session-replaced`, not a missing rollout or authentication failure.

Identity-only temporary diagnostics showed an unrelated native title helper: source `vscode`, classification `thread_title`, separate session tree. Pinned native source explicitly creates these title threads as ephemeral.

The broker now distinguishes that concrete ephemeral auxiliary from replacement conversations. A permanent consumer-visible regression failed before this correction: the bound turn lost all capabilities. A non-ephemeral root with the same classification must still invalidate the target.

Temporary identity diagnostics were removed. The next exact three checks (`bg_82`) cleared the actual owned-workspace trust dialog and reached the ordinary native composer without a setup turn. They still failed: same-turn completion at 182.72 seconds; native Plan-mode selection at 32.81 seconds; genuine structured command-request observation at 187.22 seconds. None reported a cleanup/mode-restoration failure.

The redacted first-start attachment shows the real model's concatenated start and steer markers, followed by the native completed-turn surface. Its public broker state still said `running`, with both prompt receipts applied and no authoritative completion. The interrupt attachment shows the genuine command-approval dialog while broker activity said `awaiting-input` but its pending registry stayed empty. These are observer/interaction failures, not missing model authentication.

A finite method-only diagnostic (`bg_83`) confirmed the observer received matching `thread/status/changed` but no `turn/started`, `turn/completed` or structured command request in the selected check. The native continuation and broker receipts therefore cannot substitute for missing authoritative event observation.

A model-free follow-up named the fresh native thread through the supported metadata operation. Naming succeeded, but `thread/resume` still rejected the zero-turn thread with code -32600; no setup model turn was consumed. Metadata naming is not used as fabricated bootstrap persistence.

### Explicit scoped subscription and paginated history

The broker now explicitly resumes the already native-created root once the first requested native/API turn materializes its history. Attachment is bounded; an unmaterialized first turn remains visibly attaching, not falsely ready. No prompt is invented or mutation replayed. Exact identity validation, ended-turn retention and degraded-connection checks prevent a late snapshot from reviving old authority or a finished turn.

The installed protocol requests `excludeTurns:true` plus `initialTurnsPage:{limit:1,sortDirection:"desc",itemsView:"notLoaded"}`. Completion is recovered from `result.initialTurnsPage.data`, not legacy `thread.turns`. A native-root fixture suppressing scoped events completed its first real requested turn before attachment: the regression failed with the legacy parser (`artifact://212`), then passed with the paginated parser. The final whole protocol target passed all 23 cases.

The real native root reports paginated history. Full `thread/read` hydration rejects that mode; fresh zero-turn history also rejects listing until materialized. Neither error is interpreted as a fabricated empty history. The fixture now reads authoritative `thread/turns/list`, full items, exact thread/turn, limit seven with no truncation, and requires all materialized turns to match its own submissions with a ceiling of six. The first real start/steer check accounts for exactly one turn, ruling out a hidden setup turn.

Native mode selection is actual UI input, not a model prompt or provider override: `/plan`, a separately rendered Enter, then enhanced-keyboard Shift-Tab (`CSI 9;2u`) to return to Default. Legacy `CSI Z` did not cycle this frontend. The question is submitted through that genuine native Plan-mode composer, because an independent API `turn/start` does not inherit the frontend's local mode mask.

Completed `plan` items are documented authoritative assistant output alongside `agentMessage`; user-message items remain excluded. A cheap-model run also dropped a repeated hex digit across a split nonce. Marker instructions now keep the nonce intact; the exact expected output remains absent from the echoed prompt and all continuation/effect assertions remain enforced.

### Request-only denial is not native-menu cancellation

The actual command prompt advertised `accept`, another menu action and `cancel`, but not `decline`. amux initially rejected deny as unsupported. Installed response schema and the pinned response handler distinguish request-only `decline` (deny and continue) from `cancel` (deny and interrupt); `availableDecisions` describes native presentation choices, not the complete typed response schema.

The existing permission regression was updated with that actual menu and failed before the admission correction (`artifact://242`). Deny now uses only the supported `decline`; allow remains constrained by the offered request and never broadens into session/policy permission. All 23 deterministic cases then passed (`artifact://244`), followed by real allow/file creation and deny/no-file-effect/continued-turn proof.

### Primary implementation sources

- [Codex 0.160.0 automatic thread listeners](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server/src/lib.rs#L1282-L1307)
- [Live metadata read and persisted resume](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server/src/request_processors/thread_processor.rs#L2860-L2908)
- [Running-thread resume reads stored metadata](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server/src/request_processors/thread_processor.rs#L4269-L4305)
- [Native title generation and model selection](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/tui/src/app/thread_title.rs#L30-L120)
- [Ephemeral structured helper creation](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/tui/src/temporary_structured_request.rs#L141-L159)
- [Pinned command-approval response conversion: decline versus abort](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server/src/bespoke_event_handling.rs#L1979-L2021)
- [Pinned real command-approval decline behavior](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server/tests/suite/v2/turn_start_zsh_fork.rs#L197-L330)

## Exact live checks after corrections

Each command selected one exact ignored root, with both approval guards, private copied account auth, explicit `gpt-6-luna`, serial execution and the original deadlines/turn ceiling.

| Exact root | Result | Meaningful turns | Test duration |
| --- | --- | ---: | ---: |
| `real_native_first_start` (`artifact://216`) | PASS: native/API identity, same-turn start/steer continuation in both attachments, stale-steer rejection and owned stop | 1 | 20.68 s |
| `real_questions_and_permissions` (`artifact://251`) | PASS: genuine native-originated question, exact structured answer/continuation, actual permission allow effect and deny/no effect with continued completion | 3 | 16.30 s |
| `real_explicit_interrupt` (`artifact://218`) | PASS: exact acknowledgement, interrupted completion, independent pending cleanup, detach/process survival and deliberate next turn | 2 | 13.70 s |
| `real_complete_broker_handoff` (`artifact://254`) | BLOCKED/FAILED: first safe offer cannot certify native/application settlement | 0 | 18.25 s |

Redacted native text and cell-color captures remain under the integration worktree's `target/e03-codex-native/`, with per-run directories recorded by each command. The durable [all32 map](e03-complete-broker-acceptance.json) preserves observed versus unobserved scenarios without turning partial observations into acceptance.

## Live fixture cleanup and privacy

Initial real failures also exposed fixture cleanup failures. Opportunistic outer-PTY reads stopped during API waits/teardown; retained writers outlived reap deadlines. Independent continuous bounded readers removed those survivor failures.

Readers retain at most 32 MiB per attachment and continue draining after an explicit overflow error. They remain active through owned process shutdown/reaping, then stop and join before terminal-mode restoration. Cleanup budgets were not inflated.

A real private attach/detach probe isolated Darwin's remaining mode-restoration failure: the exited controlling slave returned errno 25; the retained master successfully restored and read back the original modes.

The fixture now saves a master descriptor. The subsequent exact real check failed its protocol assertion but reported no cleanup or terminal-mode errors. That is cleanup evidence, not agent acceptance.

Private auth/schema/raw logs remain 0600 inside owned 0700 roots during investigation. Retained native evidence is decoded and redacted, not raw PTY logs. Live failure diagnostics expose only finite public degradation codes.

A generated, non-user local key was sent to the ordinary OpenAI endpoint when `OPENAI_BASE_URL` did not redirect this Codex version. OpenAI returned 401. The key was revoked and replaced before further local inference; no real account credential was sent to a substitute provider.

## Harness and model checks

- Installed versions: Codex 0.160.0, OMP 18.8.7, Claude Code 2.1.285, OpenCode 2.0.20, llama.cpp 0.5.0/build 11146.
- Cheapest eligible cached Codex model selected: `gpt-6-luna` (cached input/output prices 0.1/0.5 per million). Native title generation hard-codes `gpt-5.6-luna` when available, otherwise inherits the selected model; neither is sol.
- Local/free model: cached `ggml-org/gemma-4-26B-A4B-it-GGUF`, Q4_0, approximately 14.6 GB, exposed only on authenticated loopback. No model download or global service change.
- Real local Chat Completions, Responses and Messages calls returned their requested readiness markers. Unauthenticated model listing returned 401 and authenticated listing returned 200.
- Claude Code `--bare`, tools disabled, private HOME/config and the real authenticated local Messages endpoint exited 0 for one model turn. Its actual reply was `AMUX_CLAUDELOCAL_READY`; the model dropped an underscore, so exact string fidelity is not claimed.
- OMP real OpenAI-Codex OAuth succeeded both as a direct CLI and through the ordinary managed Zsh wrapper. The wrapper emitted supported lifecycle reports; no OMP structured-question broker is claimed.
- Grok's existing unexpired OAuth fetched the actual model catalog (HTTP 200). The cheapest eligible cached price among that catalog was 1.25/2.5; a minimal real Chat Completions request returned HTTP 402, not inference success.
- A separate read-only official CLI billing request returned HTTP 200 and `creditUsagePercent: 100`. No credits were purchased and no global auth was refreshed. This is an exhausted inference entitlement, not a request to add an Anthropic translation layer.
- OpenCode 2.0.20 uses version-two `providers` configuration and native SQLite credential storage. The earlier private legacy `auth.json` setup left its credential table empty; its HTTP 401 did not prove rejection of the copied valid OAuth credential. Cloud OpenCode OAuth parity is not claimed.
- Correct native version-two configuration, the built-in `@opencode/ai/providers/openai-compatible` package and an environment-held local API key succeeded against the real local Gemma Chat Completions endpoint. `opencode run --standalone` exited 0 and returned `AMUX_OPENCODE_LOCAL_READY`.
- Codex CLI with an explicit private custom provider (`wire_api=responses`, authenticated loopback URL, environment-held key, no global auth/config) exited 0 and returned `AMUX_CODEX_LOCAL_READY`. This real local Responses inference is standalone harness proof, not complete-broker acceptance.
- Grok's actual Responses route also returned HTTP 402. Authentication/model discovery works; Chat/Responses inference and Codex/Grok compatibility remain blocked by exhausted entitlement. No purchase or global refresh was attempted.
- The owned local inference service was stopped after the completed checks. The private diagnostic daemon exited 0 and removed its socket; five of six captured owned process identities disappeared on shutdown. One stopped diagnostic backend required identity-verified SIGKILL of its private process group. That identity disappeared and its private auth/schema/rollout/root was removed. This fallback is not claimed as a passing product stop test.

Primary version-two OpenCode references: [provider configuration](https://opencode.ai/v2/docs/providers), [native provider CLI](https://opencode.ai/v2/docs/cli/providers).

## Outstanding complete-broker gates

All four exact real Codex cases were executed, not skipped. The three separate same-instance operation checks passed; final complete-broker acceptance did not. The final rerun failed at its first safe offer after the 15-second phase deadline because native/application settlement remained unknown. Its report records zero meaningful turns submitted and successful owned cleanup.

The complete-broker run did not reach all 32 scenarios, the stopped-native-buffer edge or native winning-answer presentation. Its seven recorded preliminary scenario observations are not seven accepted scenarios; the overall verdict remains `BLOCKED_OR_FAILED`. Separate genuine question/allow/deny/interrupt proof and 301 mechanical tests cannot replace the missing integrated gates.

Unknown application settlement actually blocked safe grant and first acceptance. No force-only result was substituted. This was revisited at the integrated complete-broker scenario as requested, not another early isolated fence spike.

Missing prerequisite: a supported native-frontend application-consumption/rejection acknowledgement for tracked native input batches, including input already buffered in the frontend. Provider request resolution, whole-turn completion, PTY delivery and quiet time do not supply that acknowledgement. The locked scope does not authorize an upstream patch or alternate harness; no such workaround was added.

Both completed herdr workspaces/tabs and all eight captured owned shell/OMP/MCP/reviewr identities were retired. Before removing either dirty worker worktree, every nonignored source member was archived and byte-verified; feature branches and private recovery archives remain. The completed private worker-session and harness/auth/schema/raw-output roots were removed. See [worker retirement](e03-worker-retirement.json). Parent integration remains intact and uncommitted; no merge/push/release or global daemon/config/auth change occurred.

## Final handoff validation

`cargo fmt --all -- --check` passed without mutation. A Ruby safe-YAML/JSON validator checked both checkouts: all eight story headers and task ledgers agree with execution status (`e03s01`–`e03s07` passing, `e03s08` failing); required reading exists; the complete-broker map retains all 32 scenarios, zero submitted turns and verified cleanup; both retired-worker archive digests match. Stale story headers were corrected before the validator passed.

At the final Build handoff, both checkouts remained in Build with complete acceptance blocked and historical review counters preserved. Original checkout branch metadata remained `fix/substrate-defects`; implementation was uncommitted in `/Users/addomam/projects/amux-e03-build` on `feat/e03-agent-interaction`. Only handoff metadata and evidence were synchronized into the original checkout, not implementation source.

## Subsequent publication authorization

The user explicitly requested commit, merge and push after receiving the blocked-acceptance report. Commit and normal feature-branch push are authorized. Merge remains gated: `e03s08` has a real failing complete-broker check, and no independent Build PASS is claimed. Preserve the failing task statuses and exhausted historical review counters; publication does not establish runtime acceptance. Retain implementation on `feat/e03-agent-interaction`, without moving `main`, forcing a push or creating a release.

## Authorized fixes and delivery amendment — 2026-10-22

The user explicitly chose unmodified upstream Codex, accepted temporary native application-fence/optimistic-answer limitations and deferred full correctness acceptance to future ACP integration. This changes current landing scope, not the original failing test or safe-grant semantics. e03s08 and all four tasks remain failing; all32 is not accepted. Unknown still blocks safe grant. Future ACP must prove the missing contracts. See `specs/security/EXCEPTIONS.md`, the PLAN amendment and registered bug records.

Separately approved Rust 1.88 and narrowly updated time 0.3.45 → 0.3.47, time-core 0.1.7 → 0.1.8 and num-conv 0.1.0 → 0.2.2. Locked metadata confirms exactly one patched time and honest package minimums. Ratatui/Crossterm and the independent showcase's minimum are unchanged. Equivalent let chains fix 19 existing sites newly rejected by raised-MSRV Clippy without suppressions.

Post-fix `artifact://352` (Rust/Cargo 1.95.0): strict workspace Clippy, all-target check, workspace build and format check passed; 301 Rust tests passed in 29 suites with 10 ignored; exactly one independent real-terminal showcase passed; 8 OMP lifecycle tests passed (49 assertions); Zsh syntax and actual CLI help passed. Rust 1.88 execution and coverage were not measured.

Separate actual-binary private-daemon/outer-PTY smoke passed: native prompt and current-control indicator rendered; native input produced the stock-Zsh file effect; resize was exercised; native detach exited0 preserving the same shell/daemon; shutdown exited0 and the birth-qualified owned shell was gone. Root removed; no auth copied, model called or global configuration changed. First throwaway driver sent input before client authority was drawn; it was intercepted without replay. Corrected driver waited for drawn authority. See `specs/verifications/e03-fix-runtime.json`.

The dependency issue is fixed in the current resolution; GitHub alert ingestion/closure is not claimed. The accepted native limitation is not fixed. Self-audit and fresh e03-scoped dual-independent Build/security review precede the already-authorized main landing; historical review caps remain unchanged.

## Review-discovered focus accounting and observed Darwin shutdown correction

Fresh scoped round1 was FAIL: both independent general reviewers scored96.875% with the same single must-fix focus-accounting hole. The separate security specialist's no-findings report did not override that result. Retain `e03-build-review-round1.json`; historical review counters are unchanged.

Application focus now uses existing native dispatch accounting without resetting human inactivity; the obsolete unaccounted helper is removed. The paused-consumer regression failed before that correction. Actual outer-TUI focus/yield then proved no false safe offer but exposed a separate shutdown lockup: Darwin waitid returned CLD_STOPPED despite WEXITED, and the reaper held the cleanup mutex while waiting for a living child. A private C diagnostic and blocked-thread sample established the seam. Only true exit statuses now permit serialized reaping; non-exit notifications back off while preserving zombie/birth identity.

The permanent regression also requires clean shutdown within3s, without fixture fallback kills. It failed before the reaper fix and passed afterward in0.82s. Final `artifact://406`: all302 Rust tests passed (10ignored/29suites), strict workspace Clippy/check/build/format, one independent showcase, 8 OMP lifecycle tests/49assertions, Zsh syntax and actual CLI help passed.

Separate current-source real outer-TUI stop/focus/deliberate-yield/resume smoke passed in0.762s: focus remained unknown without human activity, no safe offer/new owner, actual old focus consumed after yield, clean exit0 detach/shutdown and birth-qualified shell gone; no forced cleanup required. Both failed and passing smoke evidence is retained in `e03-focus-runtime.json`. Private roots/readers were cleaned; no auth/models/global daemon or configuration changes. Refreshed self-audit is READY for fresh round2; no independent current-source or full complete-broker PASS is yet claimed.

## Round2/3 lifecycle remediation and final current-delivery proof

Round2 failed on actual removed-target registry retention and concurrent Codex cleanup completion; complete findings remain in e03-build-review-round2.json. Both were reproduced and corrected, including cleanup before fixture fallback. Round2-correction preflight passed304 Rust tests; actual controlling-PTY smoke passed native yield/safe accept/source/no replay/detach/shutdown with owned resources removed. See e03-lifecycle-runtime.json.

Round3 general reviews failed A93.75%/B96.875% on two unique late-settlement consumers; security independently passed with no unwaived HIGH, not overriding general failure. Preserve e03-build-review-round3.json. Private launch leases now survive unknown acknowledgement and retire after exact settlement/end without releasing a newer owner. Original private source remains available while unknown and is never resent. Removed targets retain only exact pending shell settlement lookup, not live context/agent state or filesystem resources. Stable report UUIDs cannot retarget to a same-name replacement.

Test-only RED checkpoint ad280c4 preserves both consumer reproductions. Final serialized artifact528 passed307 Rust tests/10 ignored/29 suites, strict workspace Clippy/check/build/format, exact independent showcase1, OMP8/49 assertions, Zsh syntax and actual help. Separate actual CLI/private-daemon/stock-Zsh smoke delayed the genuine native callback past the actual timeout: unknown/source0600 retained, original source executed once/exit0, automatic owner retired, normal safe acquisition succeeded, source cleaned, shutdown0 in0.462s, proxy joined, owned shells/socket/root gone without fallback. No auth/model/global changes. See e03-late-settlement-runtime.json.

Coverage is historical round2 partial collection, not current-source coverage or a complete PASS; exact tool failures/lower bounds and removed profiling resources remain in e03-current-delivery-coverage.json. The user selected approved Plan gates rather than the broader generic95% threshold. Fresh round4 within unchanged max5 must approve before landing. e03s08/allfour tasks and failed all32/native-fence acceptance remain unchanged.

## Round4 correction — runtime verified, final round5 required

Round4 A100%/security PASS could not override B96.875%/one native Force presentation must-fix. Complete reports: e03-build-review-round4.json. Real queued shortcut/Enter failed before correction (RED23a7016/artifact558), then passed with completed-frame/drained-input consent and exact pinned Paragraph wrapping. Formerly visible warnings resized to80x7 or clipped80x10 cannot force; fresh visible confirmation preserves process/no replay. No terminal library version replaced.

All34 ownership cases PASS567/8.77s; workspace307/10ignored/29suites and strict Clippy/check/build/fmt PASS570. Its final showcase command selected the wrong workspace package; corrected exact ignored showcase PASS1/3.46s, OMP8/49, Zsh syntax and actual CLI help completed577/exit0. Initial564 exposed an old Force text barrier, strengthened to actual modal title, plus one genuine callback timeout with cause NOT established. No timeout/expectation weakened; subsequent34/307 passed and genuine earlier rejection now yields direct diagnostics.

Separate actual continuously drained native TUI/private daemon/stock-Zsh smoke proved queued/invisible/clipped boundaries, retained human control, fresh takeover, unchanged shell PID/FRESH-only bytes. Detach0/shutdown0/0.244s; owned processes/socket/root/fds/reader gone without fallback. No auth/model/global changes. Initial undrained harness failed; its native -15/daemon0/root cleanup is disclosed, not product proof. Actual rendered surfaces/all attempts: e03-force-presentation-runtime.json. Self-audit READY; FINAL dual Build/security round5 of max5 required. Historical caps/failing e03s08/allfour tasks/all32 report unchanged; no merge/release/full-broker PASS yet.

## Final review5 — FAIL; cap exhausted, no landing

Independent A97.5%/one P1 must-fix, B100%/no findings, security PASS/no unwaived HIGH. Complete original reports: e03-build-review-round5.json. General A found unsafe stale-identity fallback signaling in opt-in smoke Owned::cleanup after failed discovery/reaping. No unrelated-process signal/exploit/live-model test was run; no correction is claimed. This privacy/resource boundary is not user-waived. BUG-e03-smoke-cleanup-identity.md remains failing. Runtime307/native Force proofs cannot override the AND-gate. Review cap exhausted5/5: human decision required; no round6 or counter reset, final fix commit, merge, push or release.

## Post-cap human-authorized repair — verified; human review pending

The user selected “Repair, then human review,” not a sixth automated review or immediate landing. The round5 P1 was reproduced with a real private process-table filesystem failure and two separately test-owned stale PID/group sentinels. RED ec6add1/artifact611 killed both with SIGTERM; no unrelated user process or forced OS PID reuse was involved.

Smoke cleanup now authorizes no fallback signals after failed discovery, removes group-wide fallback delivery, revalidates each nondirect PID and uses direct children only while try_wait proves them unreaped. Reaped direct IDs never enter numeric fallback; state errors fail closed and repeated validation is bounded. Stale survivor identities are labeled tracked, not owned or signal authority. The same cleanup serves normal/drop/watchdog paths; existing private roots/readers/terminal restoration/error propagation remain.

Actual-source local executable smoke630 passed healthy TERM-ignoring child/descendant teardown and discovery-failure sentinel preservation/direct-child teardown. Expected discovery/survivor failures remain errors, not success. All fixture children reaped and private roots removed before completion; no fallback, auth/model/global mutation. Temporary source/executable/object files removed before strict final preflight. Initial passing smoke621 and the intermediate exact regression are distinguished in e03-smoke-cleanup-runtime.json.

Final frozen-source artifact635 passed308 Rust tests/10 ignored/29 suites, strict workspace Clippy/check/build/format, independent showcase1/3.71s, OMP8/49 assertions, Zsh syntax and actual CLI help. Implementation SHA2569d944123a0ed5d65a20b66d9510128a9892b076b1633ab574f9d102928b10e5c. No subsequent implementation edit or current-source coverage claim.

Human review is now the external blocker. The verified fix remains uncommitted; no final landing commit/merge/push or forbidden round6. Original final review5 remains FAIL/exhausted5/5, all historical counters unchanged. e03s08/allfour tasks, failed all32 report, upstream native-fence/history limitations and fail-closed unknown grants remain unchanged. Failed acceptance SHA256cb71f0d2ba4b9d7f3f18b90548e9909f652c02326e3aaff4474c6fa0e894f70e; full round5 report SHA256c5f0cd72f950d61239bbb5f501a4213d53f403ada2013bc33e1483569e3c71df.

## Human delivery approval and overnight execution

The user replied “looks great” to the verified-repair/held-delivery report, authorized overnight work and permitted unsigned commits when signing is unavailable. Combined with the earlier explicit commit/merge/push request, this approves the repaired current limited delivery. It is a human delivery decision after exhausted5/5, not an independent post-repair PASS or a claim of source inspection. State/execution/security records now distinguish that authorization from immutable historical failed reviews and runtime snapshots.

Implementation is unchanged since actual local smoke630 and final full preflight635. Commit on the isolated feature branch, preserve its history, land only through the protected-main script in a clean disposable primary clone, verify normal remote receipts and configured CI if present, and retire only safely integrated owned resources. Use command-scoped unsigned signing configuration, no global changes, new live-model runs, release or force-push. Keep e03s08/allfour tasks and failed all32 evidence unchanged.

## Limited delivery landed and remotely verified

Feature f1f00851453dd90e3d924f150b3882f1c6406f41 was committed unsigned with the configured human author and normally backed up to origin. A clean primary clone fetched actual main93e5a8b2cfa8db8282b1fbad519b194f2ba2e688 and the exact feature. Fresh clone preflight654 passed308 Rust tests/29 suites/10 ignored, strict Clippy/check/build/format, independent showcase1/3.67s, OMP8/49 assertions, Zsh syntax and actual CLI help. No implementation changes since prior actual healthy/failure cleanup smoke630.

Installed protected-main land-branch.sh normally pushed code commit0bb8f2236f5a9c9df214c3ac43afe5ea820151b1. Independent ls-remote receipt matched; its complete tree6310c4f7b65e35dc38c0a3c59d5cb9ae72b26355 equals the verified feature exactly. Post-landing actual CLI help passed. Author Addison Omead Amiri <gitlog@addomam.net>, signatureN; command-scoped signing override only. No force-push, global configuration/auth/daemon change, live-model run, release or tag.

GitHub reports0 workflows,0 workflow runs,0 check runs and0 commit statuses: not configured, not CI green. The locked graph uses patched time0.3.47. GitHub still reported advisory1 open at observation; no dismissal/closure or cause for the remaining external status is claimed. Full machine receipt: e03-publication.json.

Implementation worktree was proven clean, wholly backed up and exactly integrated before normal removal; only owned target cache and empty .worktrees were ignored. Feature branch/history retained on origin. Original checkout remains fix/substrate-defects at6318845636bf33ac6800b0dadf2e6692e04bd399; both user scratch hashes match the preserved checkpoint. The disposable publisher clone is retired only after this metadata receipt is published and its remote identity confirmed. Historical failed reviews/caps, full-broker report and e03s08/allfour failing tasks are unchanged; no full acceptance PASS.
