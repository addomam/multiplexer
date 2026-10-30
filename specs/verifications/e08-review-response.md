---
type: review-response
context: {project: amux, epic: e08, story: e08s01, rounds: [1, 2, 3, 4, 5, 6, 7], baseline_commit: cc468a3}
---
# E08 review response — R7 dual PASS; R1–R6 FAIL retained

A75/100 (6 must,2 should), B81.25/100 (3 must,3 should); both independently matched19/19 frozen R1 source hashes. Verbatim reports retained as e08-review-r1-A.json and -B.json; display-clipped captures were replaced from original agent raw output without changing scores/findings. All findings were read before source remediation. Used1/5 E08 rounds before fresh round2; no historical cap reset. Story/task acceptance remains failing until independent clearance.

| Item | Category | Disposition / owner |
|---|---|---|
| A-M1 native root/PID reuse | Must / HIGH confidence9 | Fix: ephemeral root lifetime distinguishes replacement even same PID; fresh actual process-birth pin. Parent consumer; RootObservation producer. |
| A-M2 / B3 close before association | Must | Fix: register saved intent and target epoch before native/ACP publication; no late completion may undo accepted close. Parent. |
| A-M3 / B1 project snapshots | Must | Fix: authoritative sequence-qualified project cache, harmonized journal writes and explicit project-save synchronization. Parent. |
| A-M4 / B4 native failed launch marker | Must (A), should (B) | Fix: typed diagnostic-preserving prepare settlement plus exact prepared cleanup; clear only conclusive failures, preserve unknown. NativeSettlement + parent caller. |
| A-M5 / B6 OMP FIFO open | Must (A), should (B) | Fix: no-follow nonblocking open before regular-file/header checks. Parent. |
| A-M6 / B5 persisted long names | Must (A), should (B) | Fix: internal saved names retain existing accepted256-byte domain without relaxing ordinary new launch128-byte boundary. Parent. |
| A-S1 ambiguous category options | Should | Fix: retain acknowledged exact choice IDs unless a unique equivalent semantic selection actually represents them. Parent. |
| A-S2 unrelated close invalidation | Should | Fix: per-target epoch/lifecycle registration, not a global invalidation counter. Parent. |
| B2 nondurable native switch | Must | Fix: active unrecoverable context is explicit observation, superseded intent invalidated rather than masked. RootObservation + parent. |
| Parent-P1 Darwin mutable boot time | Must / process exclusion correctness | Fix: immutable kern.bootsessionuuid; never mutate global clock to test. Primary XNU clock.c explicitly adjusts clock_boottime during settimeofday; Peer::status currently interprets boot mismatch as gone. |

Parent-P1 primary evidence: https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/clock.c (clock_set_calendar_microtime adjusts boot time, lines722/765–797; sysctl getter exports it at1483–1493). Native BSD process start fields come directly from stored p_start: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c#L701-L702. Read-only installed kern.bootsessionuuid observation succeeds (36-character UUID plus newline); no system clock/config/auth mutation. No actual clock-shift reproduction or new live provider call claimed.

No consider findings. No speculative feature, compatibility shim or generic retry subsystem is authorized. Parent exclusively owns shared daemon routing, metadata store changes, tests/recovery.rs, evidence and verification. Independent workers delivered only native observation producer/tests and Codex settlement internals, without checks. Parent integrated every must/should and the additional Darwin identity defect; R1's historical proof never transferred to repairs.

## Repaired-source proof before fresh round2

- Focused3354:26 recovery-filtered units (25 store +1 actual-process stale-pin),17 public recovery scenarios,9 native integration,6 Codex units, OMP8/49 and Zsh syntax pass.
- Full3374:571 workspace tests/31 suites/12 ignored, examples, strict all-target Clippy, fmt check and build pass in128.07s.3352's old boot validator and3357's second disabled-Yield fixture race are retained; bg_24 verifies that fixture's actionable repaint repair.
- Final executable c9bce8c7c4b8be8b00c29aee9501f8bf399a2daf1ccf8de3c165989c82ba9781 is measured before/after all three actual smokes: two exact ACP contexts in the real Menu, native OMP exact-file reopening, and real OMP /new from its original persistent physical attachment followed by no old-context restart. Normal TUI/host/daemon exits, live-host termios, owned peers/PTYs/socket/root absent and unrelated sentinel survival are observed. Provider0/auth paths denied; no force-only native-fence workaround.
- Typed inactive native records retain the last trusted reference but are ineligible while current context is unavailable; a fresh valid header/process observation can unblock only the current context.
- Source ordering closes publication-before-association; the public starting-load close test is real, but no exact CPU pause in the old registration gap is claimed. No global clock mutation or individual RED claim for every added row.
- Source freeze: e08-review-r2-source.json; self-audit/F.I.R.S.T refreshed in AUDIT-e08-e08s01.md; current receipts in e08-recovery.json. Fresh dual-blind round2, not these local PASSes, determines independent acceptance.


## Round2 — all findings read; FAIL, used2/5

A84.375 (2must/3should), B93.75 (1must/1should), no consider. Full verbatim reports are e08-review-r2-A.json/-B.json. A independently matched19/19 source hashes; B lacked a digest-capable tool, so19 were unverified, not mismatched. R2 runtime/full evidence is preserved in e08-recovery-r2.json, not accepted for new source.

| Consolidated item | Category | Corrective disposition |
|---|---|---|
| B-M1 / A-S2 stale native pin and dropped newer same-target checkpoint | Must HIGH8/10 / should | One target-local checkpoint mutex; take candidate/native/ACP observations only after acquisition, so a competing already-drained notification cannot be silently discarded. Keep independent close/retire epoch fencing. Parent. |
| A-M1 first/replacing-root quit before candidate persistence | Must MEDIUM9/10 | Same checkpoint scope for root quit; consult selected integration root independently of recorded candidate. A recognized selected shutdown advances epoch/forgets even before a candidate exists; displaced shutdown stays inert. Parent. |
| A-M2 managed Codex replacement retains A | Must MEDIUM9/10 | Explicit recoverable:false known-replacement observation and sticky replacement state; typed inactive blocks allowed for native OMP/Codex only. No B identity guess, A fallback or work replay. Native worker + parent. |
| A-S1 / B-S1 project-config and automatic unsaved context | Should | Wire both mutation producers after releasing State; synchronize coherent sequence-qualified journal metadata. Cache only active or retained-journal groups, prune after final record removal. Two public real recovery cases, including structured shell cd. Parent + project-test worker. |
| A-S3 uncertain native cleanup loses charge | Should MEDIUM9/10 | Remove exact native instances only after conclusive settlement across cleanup/drop/publication/connect/worker-start paths; retain uncertain backends, capacity and diagnostics. Native worker. |
| B hash-tool limitation | Review certification prerequisite, not code defect | Next dual-blind pair uses digest-capable code-reviewer profiles, both retaining full security focus; never treat parent hashes as B's independent measurement. |

All corrective production/fixture changes are integrated. Focused3405 passes19 public recovery cases,29 journal/coordinator units,8 native Codex units,9 native integration cases and stock OMP8/49/Zsh syntax. Three actual-coordinator regressions exercise queued current-root observation and late first/replacement-root quit commits without production pause hooks; no individual old-source RED is claimed. The real deterministic managed-Codex replacement/cold restart refuses old A without another launcher call. Final native preparation/shutdown proof3413 passes both pre/post-backend gates after bg_34 exposed request-drain-before-stop nontermination; broker stop and preparation join now precede connection drain.

## Source-bound proof before round3

The pre-final full578 pass3408 is historical. Full bg_36/artifact3417 then stopped at the existing auth mouse fixture: a removed one-off stopped-owned-process probe confirmed stale armed-paint admission before the Prompt click was consumed. Mouse-only visible read-only fencing plus actual unsupported-choice observation repairs the driver; the initial broader fence reset keyboard focus and is preserved as a failed attempt. All3 exact native auth routes pass4.08s without production gating changes or activation retry/replay.

Current full bg_37/artifact3429 passes579/31/12ignored, examples and strict Clippy/fmt/build in133.18s. Fresh20-source manifest e08-review-r3-source.json binds all3 actual CLI/Menu/native OMP smokes to debug4f1d134f7f6afa5c3ddfd1ab5676dac1df3cca9e7862659f3d9b64101391c34d measured before/after; complete cleanup receipts are e08-recovery-r3-runtime.json. Refreshed self-audit/FIRST passes locally. E08Round3A/B are independently reviewing blind, used3/5; story/tasks remain failing pending clearance. R1/R2 remain FAIL, provider0, no auth/global/install/Git delivery changes and historical caps unchanged.

## Round3 — both complete reports read; combined FAIL, used3/5

A96.875 (1must/0should), B96.875 (0must/1should), both independently matched20/20 frozen sources. B meets the numerical zero-must/94 threshold but reports incorrect behavior and its should-fix remains required; A blocks combined acceptance. Full verbatim reports are e08-review-r3-A.json/-B.json. Neither source is edited until both reports have been read; this condition is now satisfied.

| Item | Category | Corrective contract |
|---|---|---|
| A-M1 profile defaults remerged into saved choices | Must P2 confidence0.99 | Resolved configured launch identity stays validated, but restored acknowledged settings are authoritative and must not merge original launch defaults again. Ordinary new-launch merging stays unchanged. |
| B-S1 legacy fallback plus ambiguous modern model category | Should P2 confidence0.97 | Omit the unresolvable synthesized model alias; count all category entries as the model resolver does and retain exact acknowledged choices. Preserve distinct valid legacy-mode-first resolution instead of blanket-dropping semantic settings. |

Parent owns the coupled daemon/settings serialization and behavioral regressions; no independent edit slices. Prove both actual relaunch paths and effects without provider calls, then rerun focused/full/actual runtime/self-audit and fresh independent R4. Frozen R3 full579/runtime evidence remains historical; all acceptance stays failing and max5/historical caps remain unchanged.

## Round3 corrective local proof

Both new public regressions are RED before production edits at3463 (0/2) and GREEN at3465 (2/2). Restored configured profiles replace their local launch defaults with the saved acknowledged settings before existing preparation/validation; ordinary creation still merges defaults. The redundant saved-settings clone in the internal reopen request is removed. Model serialization omits only the ambiguous semantic model alias while retaining exact options and existing legacy-mode precedence.

The default-composition case proves42→47→52 across two reopenings/alias forms, no pre-input work and a genuinely new default-driven105. Mixed legacy/ambiguous model metadata preserves342 then deliberate647. Focused3465 passes all21 recovery,38 ACP,29 store/coordinator,9 integration and8 Codex cases plus strict Clippy/build in22.68s. Stock OMP8/49 and Zsh syntax also pass. New full/actual runtime/audit and R4 remain; R3 combined FAIL and all earlier failures/caps stay preserved.

## Round4 current-source proof and independent review

Full bg_38/artifact3471 passes581/31/12ignored with examples/strict Clippy/fmt/build in137.80s. Frozen20-source e08-review-r4-source.json binds current debugf92e997a87e9c96b1971de6153926fa7c4f02a29f1daf7cc878724823b3de784 to all4 actual checks before/after. Complete e08-recovery-r4-runtime.json proves the repaired settings through the built CLI, exact ACP contexts in actual Menu, native OMP exact reopen and original persistent attachment /new/no stale restart, with observed normal owned cleanup/sentinel survival.

The new settings observer's envelope assumption failed once; private fixture cleanup and observer-only correction are retained as a harness error, not production RED. No product source changed after the full gate/freeze. Self-audit/FIRST and exact task5 evidence guard pass locally. E08Round4A/B are reviewing blind, used4/5; all acceptance remains failing pending clearance. R1/R2/R3 reports, failures and historical caps remain unchanged; provider0 and no globals/auth/install/Git delivery.

## Round4 — both complete reports read; combined FAIL, used4/5

A96.875 (1must/0should), B100 (0must/0should), both independently matched20/20 frozen sources. Full verbatim reports are e08-review-r4-A.json/-B.json. B's independent PASS does not override A or transfer to new source. All previous lifecycle/settings findings clear both; no supported HIGH/CRITICAL security finding was added.

| Item | Category | Corrective contract |
|---|---|---|
| A-M1 native population plus unconditional initial shell exceeds unchanged quota | Must P2 confidence0.99 | Snapshot startup recovery records once. With native records, restore before allocating a disposable blank shell; keep ordinary quota admission. Create a fresh fallback only if no terminal exists and shutdown has not started. Never reuse/close a published human shell or silently expand the limit. No-native startup remains unchanged; daemon-start hooks still follow the first available shell. |

Parent owns the coupled bootstrap/routing and regressions. Prove one original-shell OMP context reopens under unchanged limit1 and genuinely reduced-capacity failure remains explicit/retryable. New full/actual runtime/audit and the final allowed R5 pair are required. All acceptance stays failing, maximum5/historical caps unchanged; no providers/auth/globals/install/Git delivery.

## Native quota corrective local proof

The old placeholder-dependent admission case was deleted; new unchanged-limit1/fallback and genuine excess-intent/retry cases fail before the production correction at3501 and pass at3506. The intermediate3503 failure was an invented top-level native snapshot cwd field in the new test; that incidental assertion was deleted, not re-pinned. Exact cwd remains an actual-native acceptance criterion.

Snapshot journal records once before startup allocation and move them into the worker. Without native records, preserve the ordinary synchronous shell/hooks path. With native records, restore under normal quota before optional fresh fallback; never reuse/close a published human terminal, relax quota or replay work. Shutdown guards fallback and hooks. All22 recovery plus29 store/coordinator and strict Clippy/fmt/build pass3506 in16.07s; final full/five actual checks/audit/R5 remain pending. No new provider/auth/global/install/Git action or cap reset.

## Final quota-correct current-source proof

Full bg_40/artifact3512 passes582/31/12ignored, examples and strict Clippy/fmt/build in137.51s. Frozen20-source R5 debug047b421bedfbc48b4e0639241f0475cc2148e4bae0a81aa895ba6e0a0d088ec7 is measured before/after all5 actual checks. Genuine original-attachment OMP at unchanged limit1 and once-per-start hook snapshots pass, alongside the four prior settings/CLI/Menu/native behaviors and complete normal owned cleanup.

3516's new context-equals-agent-cwd gate and3519/3525's copied whole-project equality were observer errors, not production defects: retained values/source prove stable project ID/name and exact cwd while the existing unsaved main-shell context policy updates normally. Remove that incidental copied comparison, retain semantic identity checks, and observe failed-fixture PIDs/root gone. Actual CLI omp/18.13.3 is measured privately with auth/network denied; the extension's18.8.7 field is a static SDK pin.

Final self-audit/FIRST and exact task5 evidence guard pass locally. The last allowed independent R5 pair remains required; all acceptance stays failing, used4/5. No historical PASS/cap reset or new model/auth/global/install/Git delivery action.

## Final independent round5 dispatched

E08Round5A/B review the frozen20-source implementation independently and blind after full582, five pinned actual runtime checks, stock checks, local self-audit/FIRST and the exact runtime guard pass. Used5/5; no sixth round or cap reset is authorized. All story/task acceptance remains failing until both complete current reports are read and clear. Installation/Git delivery/new live models remain separately unauthorized.
## Final R5 findings and corrective ownership

Both complete reports were read and archived before changes. A96.875/100 has one must-fix; B93.75/100 has one must-fix and one should-fix. Each independently matched20/20 frozen hashes. Combined FAIL, used5/5; no sixth round or cap reset is authorized.

| Finding | Category | Corrective responsibility |
|---|---|---|
| R5 A-M1 mutable project metadata invalidates captured startup intent | Must | E08MutableRecoveryIntent owns daemon/recovery.rs and tests/recovery.rs; preserve every launch/settlement/project-ID exclusion while accepting only descriptive project changes. |
| R5 B-M1 initial attachment steals deferred recovery capacity | Must | Parent owns daemon startup_pending producer; E08StartupAttachment owns initial TUI wait/input discard. Explicit opens keep ordinary quotas; no controller or shell is allocated while waiting. |
| R5 B-S1 startup hooks follow failed fallback creation | Should | Parent stops deferred startup scheduling on failure; startup completion is published on every worker exit. A real FIFO project-hook fence will distinguish failed startup-hook execution from mere absence of its output. |

Previous582-test/five-runtime receipts stay bound to historical debug047b421b. They do not certify corrective source. All story/task acceptance remains failing. Finish reachable code and local verification; formal independent clearance requires a separately authorized/manual path after the exhausted cap, not reuse of a historical PASS or a disguised sixth review.


## Final corrective proof — local PASS, independent clearance blocked

All three R5 findings are corrected. Parent integrated both nonconflicting slices and owns daemon/protocol/verification; no additional reviewer was dispatched. Mutable project metadata is excluded only from exhaustive optimistic launch matching; staged records use current metadata. Initial TUI wait owns no controller or terminal, drains input and cannot compete for native capacity. Fallback failure exits before startup hooks, and scope-bound startup completion releases the wait.

Final3584 passes focused24 recovery/31 store-coordinator plus full586/31/12ignored/examples/strict Rust in149.08s total;3589 separately passes exact38 ACP/9 integration/8 Codex. Stock OMP8/49 and Zsh pass.3557's test-only Clippy failures were repaired without suppression. The redundant private staged-copy assertion is removed; public durable-metadata/history/fresh-effect gates remain, and full/current-binary runtime proof was repeated afterward.

Seven pinned actual checks pass at debug0a3ba8b4, including cold TUI at limits1/2, exact selectors, physical keys/fragmented paste discarded across completion, resize/safe quit, genuine offline OMP context, original-shell hooks, settings and actual Menu. Normal exits, live-host termios, joined drainers, owned PID/socket/root absence and sentinels are recorded. New observer failures were a correctly rejected overlong private socket path and an incidental idle-word gate deleted without repinning; no production workaround.

Current evidence: e08-recovery-final-source.json (21 paths), e08-recovery-final-runtime.json (seven checks), e08-final-generalize-sweep.json and e08-recovery.json. All prior full reports/FAILs remain. Installed binary unchanged, provider0, no auth/global/installation/Git delivery. Cap exhausted5/5; formal acceptance remains failing pending external/manual or separately authorized independent clearance, not an automatic sixth round or reused historical PASS.

## User-authorized cap extension

“Raise the review cap to 10 and keep going.” applies only to E08 implementation review. Maximum rises5→10; used5 remains5, and no previous failure, score or source binding is reset. Continue with a fresh blind pair at R6/10. The current21-source/debug0a3ba8b4 binding is unchanged from the full586/seven-actual-proof checkpoint. Other historical review/provider caps, native-fence unknown/fail-closed behavior, E05 parking and auth/global/install/publication restrictions remain unchanged.

## Independent R6 dispatched

Fresh E08Round6A/B review the full unchanged21-source feature independently and blind. Used6/10, not a reset. Parent remeasured every source and debug/installed executable before dispatch; debug0a3ba8b4 and installed1ad4b014 remain unchanged. Local audit, full586/strict/stock and seven actual proofs are current. Both full reports must be read before source remediation; all acceptance stays failing until dual clearance. No new model/auth/global/install/publication action.

## R6 combined FAIL and corrective ownership

Both full reports read before changes: A100/100 no findings, B90.625/100 two must/one should; both independently match21/21 frozen paths. Used6/10, all earlier pairs retained. A's PASS does not override B's findings or certify changed source.

| Finding | Category | Corrective ownership |
|---|---|---|
| B-M1 incomplete persistent rollout import poisons later discovery/retry | Must | E08AtomicNativeImport, codex.rs: complete bounded temporary copy and sync before no-clobber publication; real failed-write/retry history proof. |
| B-M2 nested destination alias causes outside chmod/write before rejection | Must / MEDIUM CWE-59 confidence9 | Same owner: descriptor-relative pre-mutation validation; preserve outside contents/modes and preexisting targets. |
| B-S1 project hooks can precede deferred daemon-start hooks | Should | E08StartupHookOrdering, daemon.rs/tests/recovery.rs: reserve startup queue order with readiness gate; preserve explicit admission, failed-fallback suppression and shutdown cleanup. |

Parent owns integration, runtime checks, docs/evidence. No checks run mid-flight. Archived586/seven-actual proof remains bound to old debug0a3ba8b4, not corrective source. All story/tasks stay failing pending actual repair proof and a fresh dual pair. No new models/auth/globals/install/Git delivery.

## R6 corrective proof and fresh R7

All three R6 findings are locally corrected. Native import uses validated/rewound descriptors, complete bounded temporary copying, copied-header validation, synchronized exclusive publication and pinned ancestry. Nested destination aliases/non-directories cannot cause outside changes; existing histories and provider date modes remain. Startup reserves a nonempty gated FIFO job before project admission; failure/stop/unwind cancels its effects without blocking explicit opens or holding runtime guards.

Full bg45/artifact3638 passes593 distinct workspace cases/31 suites/12 ignored plus one guarded child verdict; tool aggregate594/32 includes that child. Strict/examples/build pass138.03s.3634's successful explicit build precedes all nine currentf36e705d runtime cases;3640 passes named protocols and stock OMP8/49/Zsh. A real guarded-child EFBIG leaves no partial discovered history and retries complete data; actual CLI import safety and dependent hook arithmetic42 pass alongside all seven earlier runtime paths with normal owned cleanup/provider0.

3625's missing existing trait import and3630's copied-header EOF error were repaired without suppressions or assertion changes. Nine earlier observations on an unrecompiled binary after3630 are archived as stale-build observations and never transferred; all nine were rerun after successful3634 build. Current audit/FIRST, source21 hashes and exact task5 guard pass. No current independent PASS yet.

Fresh E08Round7A/B now review the whole frozen21-source feature independently and blind. Used7/10; no reset, all R1-R6 complete reports/FAILs retained. Both full current reports must be read before remediation/acceptance. No new auth/model/global/install/Git-delivery authorization.

## R7 complete dual clearance

Both complete independent blind reports were read and archived before acceptance changed: e08-review-r7-A.json and e08-review-r7-B.json. Each scores100/100 on32 finite criteria with zero must, should or consider findings and independently measured21/21 matching source hashes. Both clear all three R6 causal paths and the complete feature's consumers; neither report alone or any historical PASS supplies the combined gate.

The parent observed unchanged21-source and debugf36e705d checksums after both reports. The installed executable remains1ad4b014 at its recorded ~/.cargo/bin/amux path; an initial check used a nonexistent ~/.local/bin/amux path and failed only for that path, then the recorded executable was measured successfully. No installation occurred.

Current593 workspace cases plus one guarded child, strict/stock checks, exact task5 guard and nine actual checks remain bound to the unchanged reviewed source/build. All six task commands have executed successfully; all eight story criteria are covered by the current evidence and retained behavioral regressions. Ledger/story/epic now pass local implementation acceptance, not release or new live-provider/native0.165/Linux acceptance.

Used7/10; all six earlier combined FAILs, findings, full reports, source manifests and failed attempts remain. No cap reset or eighth round. No outstanding implementation finding; commit/install/merge/push/release require separate authorization. Native application settlement remains unknown/fail-closed and E05 remains parked.
