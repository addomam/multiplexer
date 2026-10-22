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
