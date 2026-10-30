---
type: review-brief
context: {project: amux, epic: e08, story: e08s01, round: 4, baseline_commit: cc468a3, security_sensitive: true}
---
# E08 independent round4 — complete feature and final settings integration

## User outcome and authority

On amux relaunch, automatically reopen all previously open supported exact saved conversations, with cwd/project/name, then WAIT for fresh user input. No picker, latest/new fallback, continuation/notification prompt, interrupted turn/tool/subagent retry, permission answer, queued terminal input, old authority/handle/responder/RPC replay or OMP --restore. Restore context, not running process state.

Repo /Users/addomam/projects/amux-acp, feature branch feat/agent-conversation-recovery, baseline cc468a3. Preexisting dirty E07/startup work is intentionally preserved. Review E08 and regressions introduced by it, not a redesign of existing native best-effort interaction. No installation/commit/merge/push/tag/release/global daemon/config/auth change or live-provider run. E05 stays parked. Historical native-buffer/application settlement stays unknown/fail-closed; force cannot substitute for ordinary safe-grant proof and never implicitly interrupts/kills admitted work. Historical caps remain unchanged.

## Mandatory source/evidence reading

Read CONVENTIONS.md and E08 capsule PLAN.md, epic.yaml, e08s01-story.md and e08s01-tasks.yaml under specs/epics/e08-conversation-recovery. Read specs/product/SCOPE_E08.yaml, specs/security/epics/e08/THREAT_MODEL.md, specs/verifications/AUDIT-e08-e08s01.md, e08-review-response.md, e08-recovery.json and e08-recovery-r4-runtime.json. Acceptance and all tasks intentionally remain failing until fresh independent clearance.

The authoritative frozen20-source set is specs/verifications/e08-review-r4-source.json. Independently recompute ALL20 source SHA-256 values with actual read-only checksum tooling; report matched/required/mismatched/unverified. Do not substitute parent digest output. Source is frozen; parent may update metadata outside the20-file set. Debug executable f92e997a87e9c96b1971de6153926fa7c4f02a29f1daf7cc878724823b3de784 was measured before and after all4 current actual smokes. Installed executable remains unchanged.

Review all relevant sections of src/recovery.rs, src/daemon/recovery.rs, src/daemon.rs, src/acp.rs, src/codex.rs, src/integration.rs, integrations/omp/amux.ts and src/runtime.rs; module/dependency registration and README/CHANGELOG; tests/recovery.rs, tests/integration.rs, tests/fixtures/acp_peer.rs, tests/agent_interaction.rs, tests/acp/native.rs and tests/pty_control/native.rs. Relevant existing settings consumers src/acp_profiles.rs and src/acp/settings.rs are unchanged but must be traced too. Do not review only a diff or prior fixes.

## Historical findings and current repair scope

Read ALL6 complete historical reports: e08-review-r1-A.json/-B.json, e08-review-r2-A.json/-B.json and e08-review-r3-A.json/-B.json. Historical combined gates all FAIL. R1 A75/B81.25; R2 A84.375/B93.75 (B lacked hash tools); R3 both96.875 and20/20 hashes, with A1must and B1should. B met the numerical threshold but reported incorrect behavior; every should remains required. Three of maximum5 E08 rounds were consumed BEFORE this fourth pair. No historical local runtime/full PASS transfers to new source.

R1/R2 clearances are consolidated in e08-review-response.md. Challenge the whole implementation, especially:

- Closed bounded private journal, canonical socket namespace, no-follow/nonblocking regular-file checks, stable flock/inode/bytes conflicts and atomic file+directory sync. Immutable boot+birth-qualified live/gone/unknown exclusion; never signal old reported peers. Durable pending-launch exclusion; unknown settlement or uncertain writes block duplicates.
- Intent and target epoch staged before closable publication; fallible native snapshot before publication/application admission. Accepted close cannot be undone by rollback or a late checkpoint. Target-local checkpoint scope acquired before candidate/native/ACP observation; queued newer notifications are not dropped. Selected root quit shares scope and invalidates even before candidate persistence; nested/displaced/retired roots remain inert.
- Coherent sequence-qualified grouped project metadata across save/config/automatic unsaved main-shell cwd, outside topology guards. Cache survives retained journal groups and prunes when neither journal nor runtime needs it.
- Native root lifetime is not numeric PID. OMP unavailable/nondurable current context and sticky managed-Codex replacement block old trusted intent; never manufacture B or reopen A as fallback. Native cleanup retains uncertain exact instance/backend/resources/capacity. Preparation admission and shutdown share scope; stop/cancel precedes cleanup snapshot and daemon request-handler drain.
- ACP advertised load/resume only, exact session/history/auth retry/settings and fresh ownerless lifetimes. No new-session/work/input/permission fallback or replay. Saved accepted256-byte labels versus ordinary new128-byte validation remain distinct.

R3 repairs are intentionally narrow and integrated:

1. **A-M1 default composition:** internal restored opening replaces only the owned local profile.defaults with record.settings before existing prepare(profile,None) validation/executable resolution. It does not remerge launch defaults. Ordinary new opening still calls the existing merge with requested settings. Launch fingerprint checks stay mandatory. The duplicate saved-settings clone in reopen request was removed; no public override flag or new API.
2. **B-S1 ambiguous model projection:** count all reported category entries just as the model resolver does; omit the synthesized semantic model alias when its modern category is ambiguous and keep exact acknowledged choices. Preserve distinct valid legacy-mode-first behavior rather than blanket-dropping semantic settings.

The model-free peer now supports a single thought-level setting and mixed legacy/two-modern-model metadata. Two public relaunch regressions were added for actual effects, not projection copies or incidental defaults. No production input/ownership policy changed.

## Current executed proof — DO NOT rerun

READ ONLY. Do not run builds/tests/lint/formatters/smoke/amux/agent/provider/helper binaries, read global auth, mutate files/config/Git, create worktrees/branches or contact the other reviewer. Read-only checksum binaries and source/evidence inspection ARE permitted and REQUIRED. Do not read any current R4 peer output/report. Historical combined reports are allowed.

- **bg_38/artifact3471:** examples +581 workspace tests/31 suites/12 ignored +strict all-target Clippy/fmt/build PASS in137.80s wall. Tool aggregate0.00s is not real test duration. No live ignored test ran; an intentionally ignored owned fake-backend helper may be launched by its private fixture.
- **artifact3463 →3465:** both new actual relaunch cases fail before production fixes (0/2), then pass (2/2). Full focused3465 passes21 recovery,38 ACP,29 journal/coordinator,9 integration and8 Codex plus strict Clippy/build in22.68s. Stock OMP8 tests49 assertions and Zsh syntax pass separately.
- Native final regressions include deterministic managed-Codex replacement/cold restart without another launcher call, sticky unavailable A, retained-capacity accounting, and real gated version-probe/post-spawn preparation shutdown. The actual prior shutdown RED bg_34 and focused3413 repair proof are retained; the current full includes these paths.
- Native fixture failures remain qualified: baseline/3357 actionable Yield; bg_36/artifact3417 stale auth mouse paint. A removed owned-process probe demonstrated old readiness against unconsumed input. Mouse-only read-only paint fence plus observed unsupported selection repairs it; no retry/replay or production gate relaxation. Initial overbroad fence reset Tab/Enter focus and is recorded, not suppressed.
- **e08-recovery-r4-runtime.json:4 complete actual current-binary checks.** Built CLI settings proof preserves42→47→52 and342→647→952 through two reopenings and both default alias forms, no startup prompt/cancel/new-session RPC; genuinely new conversation applies its profile default for105. Fresh lifetimes, exact saved identities and complete normal child/daemon/socket/root cleanup with unrelated sentinel survival. PID/PPID-only descendant collection reads no global command arguments. A first throwaway observer assumed an ok envelope although the retained helper already returns result; private fixture cleanup/observer-only correction is recorded, not a production defect.
- Separate actual CLI/Menu proof restores two contexts42/17/one command before deliberate57, ownerless fresh IDs and real Menu Ready labels/no picker. Actual installed OMP18.13.3 opens the exact native file in two new lifetimes without message/work replay or --restore. Original persistent ctty attachment types real /new at same PID, observes new identity; restart never reopens A or invents unsaved B.
- All4 actual checks bind to the same executable hash before/after. TUI/host/daemon exits0, live-host termios, joined drainers, observed owned peers/PTYs/socket/root absent and unrelated sentinels survive before fallback. CLI fixtures use cleared private env and deterministic absolute peers; native OMP additionally uses private state/fake key and sandbox network/auth denial. Provider0, no real auth/global/install/Git delivery. The exact task5 runtime-evidence command exits0.

Limits: no global clock shift, exact CPU pause in old registration gap or claim every historical row individually failed old source. No Linux/minimum-Rust/new genuine-provider/installed native Codex0.165 acceptance. Existing quantitative coverage waiver unchanged; no bundled scripts or fabricated script/coverage gate. Native application settlement remains historical unknown/fail-closed.

## Required independent report

Complete correctness, race/lifecycle/protocol identity, conventions, F.I.R.S.T behavioral quality, design/Fowler/clarity, performance/bounds/resources and full security review. Each finding needs exact current file:line, causal consumer-visible scenario, must-fix/should-fix/consider, priority and confidence. Supported HIGH/CRITICAL security confidence>=8 blocks. Exclude intentional malicious same-UID trusted-owner rewriting, not genuine unsafe objects/protocol metadata/lifecycle inputs. Actual CONVENTIONS.md governs; generic20-line/300-line heuristics are not repo rules.

Give an explicit finite denominator of distinct reviewed criteria, must/should counts and score100*(total-must-should)/total; no inflation or test/coverage conflation. PASS requires zero must and score>=94; parent addresses all should. Include source-hash measurements, each historical finding's clearance or remaining defect, exact actions executed/not executed and limits. Full report text, no edits. Parent must read BOTH complete raw reports before any remediation. This is round4 of max5; never reset caps or mistake local proof for combined acceptance.
