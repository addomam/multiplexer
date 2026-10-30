---
type: review-brief
context: {project: amux, epic: e08, story: e08s01, round: 5, baseline_commit: cc468a3, security_sensitive: true}
---
# E08 independent round5 — final permitted complete-feature review

## User outcome and scope

On amux relaunch, automatically reopen every previously open supported exact saved conversation in its saved cwd/project/name, then WAIT for fresh user input. No recovery picker, latest/new fallback, continue/notification prompt, interrupted turn/tool/subagent retry, permission answer, queued input, old control/handle/responder/RPC replay or OMP --restore. Restore context, not process state or permission to work.

Repository /Users/addomam/projects/amux-acp, isolated feat/agent-conversation-recovery, baseline cc468a3; preexisting dirty E07/startup work is intentionally preserved. Review the complete E08 feature and introduced regressions, not a redesign of native best-effort interaction. E05 stays parked. Historical native-buffer/application settlement stays unknown/fail-closed; force is not ordinary safe-grant proof and never implicitly interrupts/kills admitted work. No new live models/auth reads/global daemon/config/install/commit/merge/push/tag/release. Historical caps do not reset.

## Hard review cap and blindness

Four E08 independent pairs were consumed BEFORE this pair. This is the FIFTH AND FINAL permitted pair, not a reset. R1/R2/R3/R4 combined gates remain FAIL even where one reviewer passed. The parent must read BOTH full current reports before acceptance or remediation. Do not contact or read the other current R5 reviewer. Historical combined reports are allowed and required. No source edits during review; metadata outside the frozen20-file set may change.

## Required reading and source certification

Read CONVENTIONS.md; E08 PLAN.md, epic.yaml, e08s01-story.md and e08s01-tasks.yaml under specs/epics/e08-conversation-recovery; specs/product/SCOPE_E08.yaml; specs/security/epics/e08/THREAT_MODEL.md; specs/verifications/AUDIT-e08-e08s01.md, e08-review-response.md, e08-recovery.json and e08-recovery-r5-runtime.json. Acceptance and all tasks intentionally remain failing pending fresh independent clearance.

Authoritative final source: specs/verifications/e08-review-r5-source.json contains20 SHA-256 entries. Independently recompute all20 with real read-only checksum tooling; report matched/required/mismatched/unverified. Parent digests are not independent measurement. Current debug binary047b421bedfbc48b4e0639241f0475cc2148e4bae0a81aa895ba6e0a0d088ec7 was measured before and after all5 successful actual checks. Installed executable hash remains unchanged.

Read relevant complete sections across all20 paths: src/recovery.rs, src/daemon/recovery.rs, src/daemon.rs, src/acp.rs, src/codex.rs, src/integration.rs, integrations/omp/amux.ts, src/runtime.rs, module/dependency registration, README/CHANGELOG, tests/recovery.rs, tests/integration.rs, tests/fixtures/acp_peer.rs, tests/agent_interaction.rs, tests/acp/native.rs and tests/pty_control/native.rs. Trace unchanged settings/project/quota consumers as needed. Do not limit review to a diff or prior fixes.

Read ALL8 complete historical reports e08-review-r1-A.json/-B.json through e08-review-r4-A.json/-B.json. R1 A75/B81.25; R2 A84.375/B93.75 (B lacked digest tooling); R3 A96.875 must1/B96.875 should1; R4 A96.875 must1/B100 no findings. Current complete reports preserve those failures without transferring a historical local runtime/source PASS.

## Current critical contracts

Challenge the whole implementation:

- Bounded closed private journal, canonical socket namespace, no-follow/nonblocking regular-file checks, stable flock/inode/bytes conflicts, atomic file+directory sync and uncertain-write freeze. Immutable boot+birth-qualified live/gone/unknown exclusion; never signal old reported peers. Durable pending-launch exclusion; unknown settlement blocks duplicates.
- Intent and target epoch staged before closable publication; native fallible snapshot before publication/application admission. Close/retire/selected-root quit fence late commits/rollback. Checkpoint scope acquired before candidate/native/ACP observation so queued newer acknowledgements are not discarded. Nested/displaced/retired roots stay inert.
- Coherent sequence-qualified project metadata across save/config/automatic main-shell context changes, outside topology guards; cache retained for active runtime or retained journal groups and pruned otherwise.
- Native root lifetime is not numeric PID. OMP unavailable/nondurable current context and sticky managed-Codex replacement block old trusted intent; no guessed B/A fallback. Exact uncertain native instance/backend/resources remain charged. Preparation admission and shutdown share scope; stop/cancel/prepare join precede cleanup snapshot and request-handler drain.
- ACP advertised load/resume only, exact history/session/auth/settings routing, no new-session/work replay, fresh ownerless lifetimes. Saved256-byte renamed names versus ordinary new128-byte boundary stay distinct.
- R3 settings composition: saved acknowledged settings replace only the owned local profile defaults before existing validation/executable resolution; ordinary new launches still merge. Fingerprint checks remain mandatory. Ambiguous modern model categories retain exact options without unresolvable legacy model alias; valid legacy-mode-first behavior stays unchanged.

### Final R4 native quota repair

The single startup journal snapshot is cloned once and moved into the recovery worker. With NO native records, ordinary synchronous first-shell and daemon-start hook behavior is unchanged. With native records, restore through existing ordinary quota BEFORE allocating a disposable blank shell. After attempts, create a fresh fallback only when no native terminal exists; never reuse/close a published human terminal or exempt/increase quota. Shutdown suppresses new fallback/hooks. Daemon-start hooks follow the first available native/fallback shell. The private reopening method now borrows its owned Arc and receives the pre-start snapshot; its sole caller is migrated.

The old test treating a placeholder as genuine quota use was deleted. Two new meaningful cases cover original-shell limit1, explicit extra-open rejection, unknown-identity fallback and excess-record admission/retry. No public override flag, new protocol, generic retry, source pause hook or ownership/input change.

## Executed parent evidence — DO NOT rerun

READ ONLY. No builds/tests/lint/formatters/smoke/amux/agent/provider/helper binaries, auth reads, filesystem/config/Git mutations or branches/worktrees. Actual read-only checksum binaries ARE permitted and required for source certification. Parent owns all runtime verification; no concurrent checks.

- **bg_40/artifact3512:** examples,582 workspace tests/31 suites/12 ignored and strict all-target Clippy/fmt/build PASS in137.51s wall. Tool aggregate0.00s is not runtime. No live ignored test ran; an intentionally ignored owned fake-backend helper may be launched by its fixture.
- **Quota RED3501 → GREEN3506:** both new cases fail before production correction (0/2), then pass (2/2). Focused3506 passes all22 recovery,29 journal/coordinator and strict Rust in16.07s.3503's new assertion guessed a native DTO cwd field; deleted, never re-pinned. Exact cwd remains actual-native acceptance.
- Current full includes prior settings RED3463/GREEN3465, native replacement/cold restart/no extra launcher, retained-capacity accounting, real gated pre/post-backend shutdown, exact ACP auth/identity/history/settings, native control and input-discard boundaries. Stock OMP8 tests49 assertions and Zsh syntax pass separately.
- **e08-recovery-r5-runtime.json:5 complete actual current-binary proofs:**
  1. GENUINE installed OMP launched through its original persistent physical ctty attachment in the original single shell, then exact file/session/cwd/project ID/name reopened under UNCHANGED terminal_limit1. Both daemon-start hook snapshots observe exactly one terminal; the second observes the restored native shell. New peer/target lifetimes, no message/work replay, no --restore. Normal TUI0/live-host termios/host0/drainer join, daemon0, owned peer/PTY/socket/root absence and unrelated sentinel survival.
  2. Built CLI saved settings42→47→52 and mixed legacy/exact models342→647→952 through two reopenings without startup prompt/cancel/session-new; a genuinely new default-driven conversation gives105. Normal owned descendant/daemon/socket/root cleanup before fallback; PID/PPID-only collection, no global command arguments.
  3. Separate two exact ACP contexts42/17/one command before fresh57, fresh ownerless IDs/stale-target rejection, actual Menu Ready entries/no recovery picker.
  4. Installed native exact-file reopen in two fresh lifetimes, unchanged messages and exact identity/cwd/project/name.
  5. Real /new from original persistent physical attachment at the same native PID; new identity observed, restart never reopens stale A or invents unsaved B.
- All5 bind to binary047b421b... before/after. CLI fixtures use cleared private environments and absolute deterministic peers. Native OMP additionally uses private state/fake key and sandbox network/auth denial. Provider0, no globals/install/Git delivery. Exact task5 evidence command exits0.
- Fresh isolated/network-auth-denied `/opt/homebrew/bin/omp --version` measured `omp/18.13.3`; native metadata `omp_version:18.8.7` is a STATIC extension SDK compatibility pin, not the CLI version. This distinction is recorded.

### Preserved observer errors, not hidden production defects

3516's added quota observer wrongly required independent project context to equal the agent cwd.3519/3525 compared an entire mutable unsaved-project snapshot as a frozen identity. Retained pre/post values prove the SAME project ID/name/exact agent cwd; only the existing automatic unsaved-main context policy refreshed normally. The copied metadata comparison was removed while semantic identity/cwd checks stay. No production routing change/defect is claimed. Observed failed-fixture native PIDs/root are gone. No source changed after the final full gate/freeze.

Earlier native fixture failures and causal repairs are retained: actionable Yield versus Preparing Yield, stale auth mouse paint, and an overbroad read-only fence that reset Tab/Enter focus. No activation retry/replay or production admission weakening. The historical native preparation shutdown RED bg_34 and its joined/cancellable repair are separately recorded.

## Verification limits

No actual global clock shift, exact CPU pause in every historical gap or every-row historical RED claim. Pending-marker crash test injects a stopped-journal marker, not an actual spawn-to-pin crash. No Linux/minimum-Rust/new genuine-provider/installed native Codex0.165 acceptance. Quantitative coverage waiver unchanged; no bundled scripts or fabricated script/coverage PASS. Historical native/application settlement remains unknown/fail-closed; force is not safe ordinary-grant proof. E05 stays parked and historical caps remain unchanged.

## Required independent output

Complete correctness, lifecycle/races/protocol identity, conventions, F.I.R.S.T behavioral quality, design/Fowler/clarity, performance/bounds/resources and full security review. Findings need exact current file:line, causal consumer-visible scenario, must-fix/should-fix/consider, priority and confidence. Supported HIGH/CRITICAL security confidence>=8 blocks. Exclude intentional malicious same-UID trusted-owner rewriting, not real unsafe filesystem/protocol/lifecycle inputs. Actual CONVENTIONS.md governs, not generic20-line/300-line heuristics.

Report a finite explicit denominator of distinct criteria and score100*(total-must-should)/total, without inflation or coverage conflation. PASS needs zero must and score>=94; parent addresses all should. Include independently measured hashes, each historical issue's clearance/remaining defect, exact actions performed/not performed and limits. Full report text; no edits. Stay blind until both reports finish. This final pair consumes round5/5; never authorize a sixth review/reset or substitute source approval for runtime/delivery authorization.
