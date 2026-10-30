---
type: implementation-plan
context: {project: amux, epic: e08, baseline_commit: cc468a3, authorization: user-implement-it}
---

# E08 — Automatically reopen agent conversations

## Authorization and outcome

User: “implement it”, following explicit clarification: all previously open supported agents load their correct saved conversations when amux is relaunched and wait for fresh user input. No picker, per-session hunting, automatic continue/notification prompt, interrupted-turn/tool/subagent retry, permission response or input replay. This is the narrow user-authorized exception to the prior no-restoration convention, not PTY/process resurrection.

Implementation is isolated on `feat/agent-conversation-recovery`, preserving every existing uncommitted E07/startup repair. No commit/merge/push/install/release, global daemon/config/auth changes or live provider checks. E05 stays parked; historical E03 limitations and all review/provider caps remain unchanged.

## Prior art and external contracts

- Herdr0.9.5 persists agent-native session ID/path plus cwd and reconstructs a resume command; it does not revive dead processes. https://herdr.dev/docs/session-state/ and https://herdr.dev/docs/add-herdr-support/
- ACP v1 schema1.12.0 already supplies stable load and resume requests: `loadSession` gates `session/load` with history updates before response; `sessionCapabilities.resume` gates `session/resume` without history. No unsupported request or `session/new` fallback. https://agentclientprotocol.com/protocol/v1/session-setup
- OMP18.13.3 `--resume` reopens context; `--restore` actively retries interrupted work/subagents and is forbidden here. A session ID/path can exist before its backing file is durable. https://github.com/can1357/oh-my-pi/blob/v18.13.3/docs/session-operations-export-share-fork-resume.md
- Codex native `resume THREAD_UUID` uses thread ID, not its separate provider/tree session ID. Existing managed Codex broker already has `resume_thread`; its private backing store must be checkpointed into an agent-native durable store without persisting auth/config in the journal.
- Grok is integrated through ACP; ordinary uninstrumented arbitrary terminal commands have no trustworthy identity and are not scraped or replayed. Native supported identity capture uses the existing OMP extension and managed Codex broker, not a new generic tool API.
- Dependencies: [OK] existing pinned ACP schema, serde/json, libc, UUID, parking_lot; add pinned RustCrypto `sha2 = 0.10.9` to bind saved opaque session identities to the exact resolved profile argv without storing potentially secret argv. Stream length-framed arguments into SHA-256; do not implement cryptography or allocate serialized command copies. Primary API/registry evidence: https://docs.rs/sha2/0.10.9/sha2/ and https://crates.io/api/v1/crates/sha2/0.10.9 (not yanked, MIT/Apache-2.0).

## Impact assessment — Risk: High

- `src/daemon.rs`: owns projects/views, startup/teardown, explicit close, agent reports, snapshots and broker event drains. Callers are CLI, TUI, OMP reports and local API clients. Preserve fresh target/authority identities, per-PTY ownership, no I/O under authority/topology/PTY guards, normal startup readiness and owned cleanup.
- `src/acp.rs`: `Broker::reserve` is called from daemon and local units. Extend startup intent to optional exact session ID; preserve all admission, auth, settings, frame/resource and lifetime contracts. Only initialization/open-response paths change; restored history is not fresh input.
- `src/integration.rs`, OMP extension and `src/codex.rs`: existing native report/managed-launch paths gain bounded durable conversation metadata; root/main only and exact process pin, never stale/nested-agent or provider-ID guessing.
- `src/runtime.rs`: reuse existing native process-start identity through one narrow helper; no terminal behavior or signal policy change.
- `src/recovery.rs`: one durable metadata module. Reason for depth: contain untrusted-file validation, lifetime writer lock, exact process/boot pin and atomic fsync/rename so broker/runtime state never becomes a disk schema.
- Coverage: existing ACP/native/ownership/readiness/projects/integration/stock OMP/Zsh suites plus new consumer recovery integration tests and journal boundary units. New gaps are multi-conversation cold relaunch, same-conversation/no-new-work, intentional close versus daemon shutdown, unavailable/unsupported sessions, corruption/path/permissions/concurrency, and exact live-peer exclusion.

## Baseline

Before production edits, `cargo test --workspace` failed existing `native::e04s03_native_cooperative_question_handoff_discards_old_form_draft_on_reacquisition` at “offer during original pending question” (`artifact://3266`). ACP123 passed/1 failed/2 ignored in that run; chained Clippy did not run. Preserve this failure; no clean baseline or new recovery defect is claimed. Parent must classify/repair an actual preexisting fixture/runtime cause rather than hiding/retrying it into a PASS.

## Implementation slices and ownership

Parent is sole integration owner and owns shared daemon routing/startup, module registration, runtime helper, acceptance tests, docs/specs and verification.

1. Durable store: isolated worker owns only new `src/recovery.rs` and its units. Typed metadata, private bounded no-follow file, lifetime writer lock, socket-scoped durable path, atomic replace with file/parent sync, no unchanged-heartbeat writes. Exact PID/start/boot pin excludes surviving peers without killing them.
2. ACP: isolated worker owns `src/acp.rs` and deterministic peer `tests/fixtures/acp_peer.rs`. Optional load/resume intent, replay-time session routing, same-intent auth retry, explicit capability failures, resource peer PID, model-free durable peer modes. Parent migrates the daemon reserve call.
3. Native: isolated worker owns `src/codex.rs`, `src/integration.rs`, `integrations/omp/amux.ts`. OMP exact ID/file/cwd/durability reports and main-only recovery metadata; Codex exact thread/model/durable native backing-store references without auth/config in journal. Parent owns all new launch orchestration.

Workers read `local://amux-recovery-contract.md`. No mid-flight builds/tests/lint/formatters. Conflicting edits are sequenced through parent. Native surfaces use the existing authorized startup path, not intercepted input replay. ACP recovery creates ownerless fresh lifetimes (a short-lived startup controller, if used, is retired without stale grants).

## Lifecycle and failure invariants

- Record only trusted supported launches with exact conversation reference and durable backing-store evidence. Configured ACP profile references reuse trusted config rather than copying raw argv or secrets. Low-level arbitrary argv is not persisted as an automatic executable recipe.
- Persist no env, auth, transcript, screen, prompt drafts, queued input, owner/controller handles, pending RPC/responders, permissions or tool results in the journal. Agent-native saved conversations are distinct from journal metadata.
- Recover each eligible conversation once per daemon start, preserve grouping/names/cwd, make fresh runtime IDs and no owner. No project recipe/hook replay introduced by recovery. Existing ordinary stock Zsh/OMP startup remains unchanged.
- A failed/missing directory, executable, conversation, unsupported capability, live/unknown old peer or invalid journal remains explicit and visible. Preserve saved intent; never silently open a new/latest conversation or repeat a recovery loop.
- Session-specific explicit close/remove/native quit forgets that record after the accepted operation. Daemon teardown preserves previously open records for relaunch; freeze teardown-induced exits so they cannot erase them. Detach to a still-live daemon never starts another process.
- Dirty journal writes happen outside topology/authority/PTY guards, are bounded and are never hidden as successful durability. Invalid existing files are not overwritten. Recovery record deduplication prevents duplicate launch of the same exact identity.
- Sync a launch-inflight marker before creating any recovered agent process. A second crash cannot reuse the old gone-process pin as permission to duplicate the new process. Clear only after a fresh peer pin is durable or failed-start cleanup is conclusive; unknown remains blocked. Invalidate late close observations across native header work, including a not-yet-recorded candidate.

## Steps and verification

1. Implement durable metadata boundaries → verify: `cargo test --lib recovery::tests`.
2. Implement ACP exact-session load/resume and no-work initialization → verify: `cargo test --lib acp::tests`.
3. Implement native identity capture/backing-store durability → verify: `cargo test --test integration && cargo test --lib codex::tests` plus stock OMP lifecycle tests.
4. Integrate startup/recording/explicit-close/teardown → verify: `cargo test --test recovery`.
5. Exercise the built CLI/TUI using isolated deterministic agents and, where useful, the real installed OMP against a private synthetic transcript with OS networking denied. Verify at least two conversations/cwds/names, fresh IDs/ownerless views, same saved arithmetic/content, no prompt/tool/permission action before deliberate input, no fallback on missing/unsupported records and normal process/socket/root cleanup → verify: recorded actual smoke receipt in `specs/verifications/e08-recovery.json` (not accepted before exercised).
6. Run complete regressions and strict checks once integration is ready → verify: `cargo build --examples && cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check && cargo build` plus the existing stock OMP/Zsh commands.
7. Audit/review affected identity, privacy and lifetime paths; no new supported security findings; preserve historical caps and failures. Update all actual evidence/task states only after success.

No bundled bigpowers scripts exist in this repository; no fabricated script-based consistency gate is claimed. Parent checks capsule/commands/interfaces against the actual repository and parses YAML ledgers. Release/publication/installation and real-provider acceptance remain separate authorization decisions.

## Round1 corrective implementation

This implements the same approved outcome, not a new requirement/Plan cycle. R1 failed A75/B81.25; both complete source-bound reports and every consolidated disposition are retained in `specs/verifications/e08-review-response.md`. E08 used1/5 rounds; historical caps remain unchanged.

- Stage intent and a target-specific epoch before publishing any closable recovered target. Finish fallible native snapshots before publication/application admission; accepted close cannot be undone by late rollback.
- Synchronize sequence-qualified project metadata across every saved conversation in a group. Do not cancel an unrelated target's checkpoint. Internal reopening accepts the existing256-byte renamed-name domain, while ordinary ACP new-launch128-byte validation stays unchanged; concrete acknowledged option IDs survive ambiguous semantic categories.
- Track native root lifetime separately from numeric PID. Refresh exact birth pins on replacement. An active unavailable/nondurable root blocks the old trusted reference instead of reopening the wrong conversation; later valid current context requires a fresh verified pin/header.
- Classify definitely pre-backend native failures and retain the exact prepared instance through serialized cleanup. Clear launch markers only with conclusive settlement; unknown process/resource cleanup remains blocked. Open native backing objects nonblocking/no-follow before regular-file/header checks.
- Darwin pins use immutable kern.bootsessionuuid, not mutable kern.boottime. Validation and reboot fixtures use canonical UUIDs. No global clock experiment is authorized.
- Preserve repair failures:3352 rejected every new UUID through the old validator;3357 pressed disabled Preparing Yield in a second native fixture. Fixed validator migration and the fixture's actionable repaint fence, not ownership policy. Current-source proof and round2 are separate from R1's historical555-test/runtime PASS.
- Final native runtime proof includes installed OMP18.13.3 /new from the original persistent physical attachment, same live PID/new conversation, no old-context restart and no model call. Do not use force-only takeover or claim the historical native application fence is solved.

## Round2 corrective implementation

R2 failed A84.375/B93.75; full reports and frozen-source571-test/runtime evidence are archived separately. Both were read before fixes. Used2/5; R3 must use two digest-capable independent code reviewers with full security focus (R2's security-only reviewer could not hash19 sources).

- Serialize checkpointing per target **before** candidate, native or ACP observation; preserve close/retire epochs. The next queued checkpoint observes the latest state rather than dropping an already-drained newer event on candidate mismatch. Root quit shares that scope and consults selected integration root independently of candidate persistence.
- Managed Codex exposes sticky known-replacement unavailability and rechecks after backing I/O; parent blocks prior native intent rather than presenting A as B. Typed native blocks cover OMP/Codex, never ACP.
- Retain exact uncertain native instances/backends and capacity across cleanup/drop/connect/publication/worker-start failure. Remove only conclusive cleanup; serialize native preparation admission to prevent post-spawn retention from exceeding its slot budget. This does not serialize live instance operations or PTY processing.
- Wire project-config and automatic main-shell context mutation into the same recovery synchronization after releasing State. Sequence cache survives while active targets or retained journal groups need it; no cache for unopened/unrecorded projects.
- Add three actual-coordinator/current-peer regressions for queued current root and late first/replacement-root quit, two public unsaved-project relaunch scenarios (config and structured shell cd), a real deterministic native-daemon replacement/cold-restart refusal, and native retained-capacity cases. No production pause hooks or individual old-source RED claims.
- Preserve setup failures: bg_26 private diagnostic visibility;3396 oversized temporary socket namespace;3399 missing version1 test envelopes;3401 incidental inner error-type assumption;3403 whole-config/default serialization comparison. Reuse existing diagnostic API, short owned /tmp fixture plus setup RAII, proper envelopes and semantic capacity/unsaved-group assertions. Delete incidental wording/type/default expectations, never re-pin them.

## Final owned-startup settlement

Final source audit found that shutdown could snapshot before a preparing child was mapped, and the daemon waited for request departure before publishing broker stop. Joining the existing preparation scope requires stop/cancel before request drain, not a timeout increase or detached cleanup.

The new real private native-daemon regression gates version probing and post-spawn socket startup separately. bg_34 observed actual shutdown nontermination at the latter boundary; reordered native stop/join and cancellable backend connect/initialize fix the cause. bg_35/artifact3413 then passes both phases with daemon exit0, gated owned PID gone and no private cx roots before fixture removal, plus native24/1ignored, Codex8, recovery19 and strict checks/build.

The earlier full578/31/12ignored pass at3408 is pre-final-fix evidence only; rerun the complete gate for current source. No new independent round or live provider/auth/global action occurred. The native cleanup regression's full private JSON projection copy was removed, retaining actual native-ID/peer identity and the returned backing path's survival after process/private-root cleanup.

## Final native authentication fixture ordering

Full gate bg_36/artifact3417 stopped at the existing native mouse authentication case. A one-off owned-process diagnostic proved its readiness helper accepted the old armed frame while the native process group was stopped and the preceding Prompt click remained unconsumed; the probe was removed. Mouse preparation now fences through a visibly different read-only surface and observes the actual unsupported selection before returning to the primary choice. No activation is retried/replayed and no production input gate changed.

The initial global read-only fence reset Tab/Enter keyboard focus and failed that route; it was narrowed to mouse-only. All three actual native auth routes then pass in4.08s, including exact alternate mouse selection and method-specific arithmetic effects. Current full bg_37/artifact3429 passes579/31/12ignored and strict checks/build in133.18s. Twenty-source R3 freeze and all3 pinned actual CLI/Menu/native OMP smokes pass; fresh independent R3 is running and story/tasks remain failing until clearance.

## Round3 settings-boundary correction

Both complete independent reports were read before remediation. R3 combined FAIL: A96.875 must1, B96.875 should1, both hashes20/20; used3/5. Fix the existing approved exact-settings contract, not requirements or Plan scope.

- Keep configured launch identity/fingerprint validation, but apply saved acknowledged settings authoritatively on internal reopening, without remerging profile launch defaults. Ordinary new-launch behavior is unchanged.
- Do not inject a legacy display-model fallback when its modern category is ambiguous. Count category entries exactly as the existing resolver does and retain concrete choices. Preserve the resolver's distinct valid legacy-mode-first behavior rather than blanket-dropping semantic settings.
- Add public model-free relaunch/effect regressions for nonempty profile defaults and mixed legacy/ambiguous modern models. Parent owns both coupled paths; no conflicting delegation. New source requires fresh full/runtime/audit and R4; R3 proof remains historical and tasks stay failing.

Both new public relaunch regressions fail before production repair at3463 and pass afterward at3465. The profile-default case preserves42→47→52 across two reopenings and both default alias forms, while a genuinely new conversation applies its default for105. Mixed legacy/two-model options preserve342 before fresh647. Focused3465 also passes all21 recovery,38 ACP,29 store/coordinator,9 integration and8 Codex tests plus strict Clippy/build. Stock OMP8/49 and Zsh syntax pass separately. Full/current actual runtime/audit and R4 are still required; no PASS is transferred from frozen R3.

Current repaired-source full bg_38/artifact3471 passes581/31/12ignored plus examples/strict Clippy/fmt/build in137.80s. R4's20-source manifest and all4 actual checks use debugf92e997a measured before/after. The additional built-CLI settings proof preserves42→47→52 and342→647→952 through two reopenings while a genuinely new default produces105. Original Menu/native OMP checks and complete normal owned cleanup also pass; provider0. Fresh independent R4 runs blind, used4/5; tasks remain failing and historical gates are not reset.

## Final native startup capacity correction

R4 combined FAIL A96.875 must1/B100 no findings, both hashes20/20; both complete reports read before remediation. Used4/5, final R5 not started. An unconditional fresh shell currently consumes one slot before native recovery, so an original-shell OMP session cannot reopen at unchanged limit1.

Snapshot old records once before bootstrap. Preserve synchronous ordinary first-shell/startup-hook behavior when no native records exist. Otherwise let native restoration use normal quota first, then create a new fallback only if no terminal exists and shutdown has not begun; schedule daemon-start hooks after a shell is available. Never steal/reuse a published human terminal, close admitted work, exempt placeholders from counting or enlarge quota.

Add unchanged-limit1 original-shell relaunch coverage and make genuine reduced-quota failure/retry explicit. Preserve fallback for unavailable native records, once-per-start attempts, no work replay, owned cleanup and exact saved identity. New source needs complete full/actual runtime/audit and final independent R5; frozen R4 proof remains historical.

Quota regressions are RED3501 (0/2) before bootstrap correction and GREEN3506 (2/2) afterward. The old admission case that treated a placeholder as genuine quota use was removed. New cases cover an original-shell native context at unchanged limit1, explicit rejection of another terminal, unknown native identity blocking with fresh fallback, and excess native intent under an actually smaller record budget followed by later retry.3503's guessed native snapshot cwd field assertion was deleted, never re-pinned; exact cwd remains covered by actual native reopening.

The startup journal snapshot is cloned only once and moved into the worker. No-native startup stays synchronous. Native recovery precedes optional fallback allocation; quota admission is unchanged and no existing terminal is reused/closed. Shutdown suppresses new fallback/hooks. Focused3506 passes all22 recovery,29 store/coordinator and strict Clippy/fmt/build in16.07s. Prepare a separate genuine installed-OMP original-attachment check at unchanged limit1, including once-per-start hook snapshots; then complete the final full/runtime/audit/R5 gate.

Final current-source bg_40/artifact3512 passes582/31/12ignored, examples and strict Rust in137.51s. Twenty-source R5 candidate binds debug047b421b measured before/after all5 successful actual checks. Genuine installed omp/18.13.3 reopens from its original persistent physical shell at unchanged limit1; both startup-hook snapshots contain exactly one terminal and the second is the restored native shell. Exact file/cwd/project ID/name, no work replay, normal ctty/termios/process/socket/root cleanup and unrelated sentinels pass.

The new quota observer's3516 context-equals-cwd assumption and3519/3525 copied whole-project equality were harness errors: retained values confirm stable ID/name/cwd and normal mutable unsaved-main context refresh. No production routing defect/fix is claimed. SDK18.8.7 is a static extension pin; CLI version was measured separately with network/auth denied. Final R5 is not started; all tasks remain failing, used4/5, historical caps unchanged.

## Final R5 corrections and local completion

Both complete final reports were read and archived before remediation: A96.875 must1, B93.75 must1/should1, each20/20 hashes. Combined FAIL; the five-round cap is exhausted. No sixth review or cap reset was started. Approved requirements remain unchanged.

- Compare every captured launch/settlement field exhaustively, excluding only descriptive project name/context; retain project ID and stage current sequence-qualified metadata. Deterministic startup-gated public coverage preserves two later exact histories, fresh arithmetic and explicit-close exclusion.
- Publish ephemeral startup_pending until recovery/fallback ordering settles, including scope-bound completion on early exit. Initial attachment waits without a new shell/controller, discards waiting events and registration-time input, handles resize and permits safe quit. Explicit launch/ordinary quota and exact resumed-attachment continuity are unchanged.
- Failed fallback returns before daemon-start hooks. A real later project hook fences the FIFO worker; independent ACP remains usable, with actual fresh arithmetic, even when the first PTY cannot be created.

Final bg_43/artifact3584 passes focused24 recovery/31 store-coordinator, then full586/31/12ignored/examples/strict Clippy/fmt/build in149.08s total. OMP8/49 and Zsh syntax also pass;3589 separately passes exact38 ACP/9 integration/8 Codex commands. First3557's two test-only Clippy failures were corrected without suppression. A redundant private staged-Candidate.project copy assertion was removed; public durable metadata/history/fresh effects and all launch/retirement exclusions remain. The full gate and all seven actual checks were repeated afterward.

All seven actual checks in e08-recovery-final-runtime.json pass against debug0a3ba8b4 measured before/after. New cold-attachment quota1/spare-capacity cases exercise physical input/fragmented paste, exact-selector waiting, resize, safe quit, genuine sandboxed OMP context and normal termios/process cleanup without replay. The five previous CLI/settings/Menu/native behaviors pass again. Installed1ad4b014 is unchanged; provider0.

The cold fixture's long macOS TMPDIR correctly hit the managed socket bound; use private canonical/tmp, not a production workaround.3565's literal native idle-word assertion was incidental and deleted, never re-pinned; exact context/messages/effects remain checked. No final old-source runtime RED or successful managed-native Codex provider run is invented.

Final21-source binding is e08-recovery-final-source.json. Local audit/FIRST and defect-class sweep pass within their stated limits; independent post-remediation clearance remains unavailable under the exhausted5/5 cap. Story/tasks stay failing and delivery is not authorized. Finish no further implementation before human/manual or separately authorized clearance resolves that gate; do not reset historical caps or transfer a PASS.


## Authorized independent-review continuation

User explicitly raises E08 implementation review maximum5→10 and says to continue. Retain all five consumed pairs and every R1-R5 finding/score/FAIL; fresh E08Round6A/B is round6/10. Other historical caps, requirements, E05 parking and provider/auth/global/install/publication restrictions do not change. The unchanged21-source/debug0a3ba8b4 binding is remeasured before dispatch, preserving existing586/strict/stock/seven-actual proof without unnecessary reruns. Read both full current reports before remediation and retain failing acceptance until the dual gate clears.

## R6 import and hook-order corrections

Both full R6 reports were read before remediation: A100 no findings/B90.625 two must/one should, both21/21 hashes; combined FAIL retained, used6/10 before fresh R7.

Repair persistent native import as a bounded validated-descriptor transaction: undiscoverable private temp, complete size/copied identity validation, sync, exclusive atomic publication and pinned ancestry sync. Descriptor-relative no-follow destination traversal rejects aliases before outside effects and never chmods preexisting provider date directories. Guarded child real EFBIG/retry and complete/no-clobber/alias/mode cases protect the boundary.

Reserve deferred daemon-start FIFO order before project requests, but gate effects until first-shell success. Failure/stop/unwind drops the one-shot; explicit project open/new stay ordinarily admitted and stock shells responsive. Preserve no-native/empty hooks and failed-fallback behavior. Tests prove ordered once-only setup-dependent effects and shutdown without releasing the native probe.

Current full bg45/3638 passes593 distinct workspace cases plus one guarded child verdict,31 parent suites/12 ignored, strict/examples/build138.03s.3634 passes focused13 parent Codex plus child/26 recovery and explicitly builds before all nine currentf36e705d actual cases;3640 covers named protocol/stock checks. Missing trait/EOF integration failures and rejected stale-build observations remain separate, never substituted for current proof. All nine current cases pass/provider0/normal cleanup; no genuine native Codex0.165 or new live-model claim.

R7's21-source freeze, brief and runtime receipts are e08-review-r7-source.json, e08-review-r7-brief.md and e08-recovery-r7-runtime.json. Fresh R7A/B run blind, used7/10; all earlier failures stay. Acceptance remains failing pending the current dual gate; no installation or Git delivery.

## R7 local implementation acceptance

Both complete blind R7 reports were read and archived before acceptance: A100/B100, zero findings, each21/21 independently matched source hashes. All three R6 repairs and whole-feature consumers clear; all six earlier combined FAILs remain. Used7/10, no reset or eighth review.

Unchanged current source/debugf36e705d binds the593 workspace cases plus guarded child, strict/stock checks, all six task verification commands and nine actual cases. Ledger/story/epic pass local implementation acceptance. This does not install, commit, merge, push, release or expand live-provider/Linux/native0.165 claims; separate delivery authorization remains required.
