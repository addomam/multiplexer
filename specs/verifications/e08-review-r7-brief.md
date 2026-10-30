---
type: review-brief
context: {project: amux, epic: e08, story: e08s01, round: 7, maximum_rounds: 10, baseline_commit: cc468a3, security_sensitive: true, depth: full_maturity}
---
# E08 independent round7 — complete-feature review

## Authorization and blindness

The user explicitly raised ONLY E08 implementation review maximum5→10 and said to continue. Six pairs remain consumed; this is round7/10, never a reset. All R1-R6 combined FAILs/scores/reports/source bindings remain. Other historical design/implementation/provider caps, E05 parking and delivery restrictions are unchanged. Old5/5 exhaustion statements are historical; see e08-review-cap-extension.json.

Review independently. Do NOT contact or read the current peer. Historical full reports are required. Parent must read BOTH complete current reports before remediation or acceptance. The21 manifest paths stay frozen while either reviewer runs; metadata outside them may change.

Repository /Users/addomam/projects/amux-acp, feature branch feat/agent-conversation-recovery, baseline cc468a3. Preexisting dirty E07/startup changes are intentionally preserved. Review the whole E08 feature and introduced regressions, including unchanged consumers, not only the last repairs. Do not reopen settled requirements, demand native best-effort redesign or substitute a cosmetic architecture exercise.

## User outcome and hard boundaries

Relaunch automatically reopens every supported previously open exact saved agent conversation, retaining cwd/project/name, then WAITs for fresh user instruction. No recovery picker, latest/new fallback, continuation/notification prompt, interrupted turn/tool/subagent retry, permission answer, queued input or old controller/handle/responder/RPC replay. Native OMP --resume FILE, never --restore. Restore context, not processes or permission to work.

Native application-buffer settlement remains historically unknown/fail-closed. Force is not ordinary safe-grant proof and never implicitly interrupts/kills work. E05 remains parked. No new live models/auth reads/global daemon/config/install/commit/merge/push/tag/release.

## Required reading and source certification

Read CONVENTIONS.md; specs/epics/e08-conversation-recovery/{PLAN.md,epic.yaml,e08s01-story.md,e08s01-tasks.yaml}; specs/product/SCOPE_E08.yaml; specs/security/epics/e08/THREAT_MODEL.md; specs/verifications/{AUDIT-e08-e08s01.md,e08-review-response.md,e08-recovery.json,e08-review-cap-extension.json,e08-review-r7-source.json,e08-recovery-r7-runtime.json,e08-r6-generalize-sweep.json}.

Read ALL TWELVE COMPLETE historical report bodies: e08-review-r1-A.json/-B.json through e08-review-r4-A.json/-B.json; lowercase e08-review-r5-a.json/-b.json; e08-review-r6-A.json/-B.json. Extract full embedded report strings, not clipped JSON previews. Story/tasks intentionally remain failing until current dual clearance.

Independently measure ALL21 source hashes in e08-review-r7-source.json using real read-only checksum tooling; report matched/required/mismatched/unverified. Parent values are not independent measurements. Source paths cover secure journal, daemon/coordinator, ACP/Codex/native integration/runtime/TUI/OMP extension, dependency/module registration, fixtures/recovery/native/integration tests and README/CHANGELOG. Read relevant complete sections and trace unchanged CLI/client readiness, project/settings and input consumers. Read-only baseline diff is allowed, no Git mutation.

Current debugf36e705d49d4378515a89e599ad11cba7cec2abe91b8a372f7ff983ca4adccdf; installed1ad4b01497b0fcbe7b82a4579dcc551b18eab55278b853736ea91c6372c4e73f unchanged. Independent executable hashing optional; source certification mandatory.

## Complete contracts to challenge

- Closed private journal4MiB/512 records; canonical socket namespace, no-follow/nonblocking owner-only regular files, stable writer lock/inode/exact-byte conflicts, atomic file+directory sync and uncertain-write freeze. No transcript/auth/env/screen/draft/work/permissions/responders/authority in recovery metadata; native backing stores separate.
- Immutable boot UUID plus birth-qualified live/gone/unknown exclusion, fresh root-lifetime pin even same PID, never signal saved old peers. Durable launch_pending precedes recovered spawn; unknown settlement blocks. Rollback requires conclusive exact-instance settlement; uncertain resources stay retained and charged.
- Saved association and target epoch before closable publication; native fallible snapshot before publication/application admission. Close/retire/selected-root quit fence late commits/rollback without resurrection. Checkpoint scope precedes current candidate/native/ACP observation, target-local rather than global.
- Coherent sequence-qualified project metadata for save/config/automatic main-shell context changes outside topology guards; cache retained for active targets or journal groups and pruned otherwise. Exhaustive launch matching excludes only descriptive project name/context, retaining project ID and every identity/launch/peer/pending/block field. Staged candidates use current metadata; per-conversation cwd remains exact.
- ACP advertised load/resume only, exact opaque identity/history/auth/settings, substitutions rejected, no session/new/work replay, fresh ownerless runtime/control identities. Saved acknowledged settings override local defaults before normal validation; ordinary new creation still merges defaults, launch fingerprint stays mandatory. Preserve exact ambiguous-category choices and valid legacy-mode-first behavior. Internal saved-name256 versus ordinary new128-byte boundary unchanged.
- Native root selection excludes nested/displaced/retired reports; unavailable OMP context and sticky Codex replacement block A, never guess B or silently fall back. Exact fixed quoted OMP resume. Managed native Codex existing0.160 restriction remains; genuine installed0.165 is NOT claimed supported, and ACP is separate.
- Native preparation admission/cancellation/join, conclusive cleanup and retained capacity stay bounded and outside topology/authority/PTY guards. Ordinary quota before optional blank fallback, never take/exempt/increase a published human terminal. Ephemeral startup_pending gates initial TUI/exact selectors before implicit open/controller registration; waiting/registration events drain without replay, safe quit/resize works, explicit launches and exact Resume continuity retain paths.

## R6 findings and current repairs

R6 A100 no findings, B90.625 two must/one should, each21/21 hashes; combined FAIL. Both complete reports read before remediation. A's PASS does not certify changed source. Independently confirm these causal paths and check the whole feature:

### B-M1: persistent import completeness/retry

The durable destination no longer uses ephemeral direct-to-final-name copying. Source native header validation returns the owned no-follow descriptor rewound. Copy bounded complete bytes into a private undiscoverable temporary file; check source size/completeness, rewind and validate the copied header/identity, synchronize data, publish via exclusive atomic rename, then synchronize pinned ancestry. No overwrite or hard-link window. On error remove only owned temporary output. Darwin renameatx_np(RENAME_EXCL), Linux renameat2(RENAME_NOREPLACE); other platforms fail closed. No new dependency or auth-copy behavior change.

Five new parent Codex cases cover whole history/modes, existing target/inode preservation, nested aliases, non-directory objects and real partial write/retry. The last reexecutes exactly itself in an owned deadline-bounded child, applies/restores RLIMIT_FSIZE and SIGXFSZ ONLY there, requires real EFBIG, proves no discoverable partial/temp, source intact and full exact retry. Existing lifecycle/store tests remain.

### B-M2: destination aliases before side effects

Pin store/session/date directory FDs, no-follow openat/mkdirat on components before writes. Existing owned provider date directories retain their modes; root/sessions keep private policy. No chmod through aliases, no reliance on post-copy ancestry rejection. Tests preserve complete outside contents/modes at multiple alias depths and unexpected-object paths; actual CLI rejects alias before backend launch. Intentional malicious same-UID rewriting excluded, unexpected aliases remain in scope.

### B-S1: startup/project hook FIFO

Reserve nonempty deferred-native daemon-start HookJob before request admission. Private one-shot readiness gate postpones its effects until successful first native/fallback shell; failure/stop/unwind drops sender and skips effects. Worker waits without daemon/runtime guards. Ordinary no-native and empty-hook paths stay unchanged; explicit project-open/new stay responsive under normal quota, bounded queue/backpressure diagnostics unchanged. Initial TUI startup/input guards remain.

Public gated preparation regression admits open/new, observes responsive stock shells, then proves daemon setup precedes once-only dependent project effects and ordinary quota. Shutdown regression leaves native probe gate unreleased, cancels startup/project effects and observes normal daemon exit and owned probe/shell absence. Previous failed-fallback/independent ACP/FIFO fence remains. Queue reservation is not execution: failed fallback never enables startup effects.

## Executed parent proof — DO NOT rerun

READ ONLY. NO edits/builds/tests/lint/formatters/smoke/amux/agent/provider/helper execution, auth reads, filesystem/config/Git mutation or branches/worktrees. Read-only checksum binaries ARE allowed and REQUIRED. Parent owns all checks and runtime verification.

- bg_45/artifact3638: full workspace593 distinct parent cases/31 suites/12 ignored PLUS one guarded child test verdict. Tool aggregate594/32 includes that child, not an extra workspace case. Raw root library has223 tests; separate filtered child has1. Strict all-target Clippy/fmt/examples/build PASS,138.03s wall. Aggregate0.00s is not runtime.
- artifact3634:13 parent Codex cases +1 child,26 recovery, strict Rust/examples/actual CLI build PASS20.81s. Explicit successful build precedes current smokes.
- artifact3640: named38 ACP/9 integration/13 parent Codex +1 child, stock OMP8 tests49 assertions and Zsh syntax PASS,3.99s total, Bun246ms. No live ignored test ran; only owned guarded fixture helpers may execute.
- All NINE actual checks in e08-recovery-r7-runtime.json bind debugf36e705d before/after:
  1. Built CLI native import: entire history/source retained, aliased destination rejected before outside mode/content changes or backend launch, preexisting durable data preserved, ordinary owned date directories usable, no failed target/profile roots or auth/config in durable store.
  2. Built CLI during real native preparation: explicit project open admitted, daemon setup precedes dependent project arithmetic42, once-only ordering/reuse and no extra shell.
  3-4. Actual controlling-PTY cold ordinary/exact-selector attachment at limits1/2: no blank slot, safe quit/resize, keys/fragmented paste discarded across completion, genuine offline exact OMP context, no message replay.
  5. Genuine installed OMP in original persistent physical shell, exact file/session/cwd/project ID/name reopened under unchanged limit1; two startup-hook snapshots each show one terminal, restored native in the second.
  6. Built CLI settings42→47→52 and342→647→952 over two reopenings; genuinely new default105; no startup prompt/cancel/new-session.
  7. Two exact ACP contexts42/17, fresh ownerless identities/stale-target rejection, actual Menu entries, no recovery picker.
  8. Genuine native exact-file OMP reopening in fresh lifetimes without work/messages/--restore.
  9. Original persistent physical attachment /new at the same native PID; new ID seen, no stale-A or invented unsaved-B reopen.

Successful smokes observe normal TUI/host/daemon exits, live-host termios, joined drainers, owned peer/PTY/socket/root absence and unrelated sentinel survival BEFORE fallback. PID/PPID-only descendant observation, never global command arguments. Provider0. ACP uses cleared private env/absolute deterministic peers. Genuine OMP18.13.3 uses private state/fake key, sandbox network denial except Unix IPC and global-auth-path denial. Extension18.8.7 is an SDK pin, not measured CLI version. Managed Codex fixture advertises supported version then exits before provider work; this is not successful real native0.165/provider acceptance.

### Failed attempts kept distinct

3625 missed an existing parent PermissionsExt trait import; restored without suppression after LSP offered no import fix. Parent strengthened copied-header identity validation before publication;3630 caught its EOF cursor error in real tests, fixed by rewinding before validation without changing assertions. The parallel pipeline stopped before CLI rebuild, so nine observations on earlier binary6c250b0a were explicitly rejected as current proof and archived in e08-recovery-r7-stale-binary-runtime.json. Current3634 succeeds and builds BEFORE all nine currentf36e705d checks. Do not transfer stale-binary PASS. Historical original settings/quota/preparation REDs and observer/fixture mistakes remain in evidence; no invented old-R6-source RED for every finding.

## Review and reporting requirements

Assess complete correctness, lifecycle/races/identity, producer-to-consuming-dispatch dataflow, security, resource bounds/cleanup, actual conventions, F.I.R.S.T quality, design/Fowler/clarity. Findings need exact current file:line, concrete consumer-visible causal scenario, must/should/consider, priority/confidence. Supported HIGH/CRITICAL security confidence>=8 blocks. No cosmetic/speculative issues or generic20-line/300-line heuristics.

Return FULL report, specific historical issue dispositions, independently measured hashes matched/required/mismatched/unverified, finite distinct-criterion denominator and score100*(total-must-should)/total (not inflated or conflated with coverage), actual actions performed/not performed and limits. PASS requires zero must and score>=94 independently; parent addresses all should. Neither historical PASS nor source review alone grants combined/runtime/delivery acceptance. Stay blind until both complete.

No global clock shift/exact CPU pause at every old boundary; marker crash case is stopped-journal injection, not actual spawn-to-pin crash. No Linux/minimum-Rust/new genuine-provider/native0.165 acceptance. Quantitative coverage waiver unchanged; bundled scripts unavailable, no fabricated script PASS. Native application fence stays unknown/fail-closed, E05 parked. This consumes7/10 with all earlier six pairs retained.
