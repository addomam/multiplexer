---
type: review-brief
context: {project: amux, epic: e08, story: e08s01, round: 3, baseline_commit: cc468a3, security_sensitive: true}
---
# E08 independent round3 — complete current feature and integrated repairs

## Goal and authorization

On relaunch, automatically reopen every previously open supported agent's exact saved conversation with cwd/project/name, then WAIT for fresh user input. No recovery picker, new/latest fallback, automatic continue/notification prompt, interrupted turn/tool/subagent retry, permission answer, queued terminal input, old control/handle/RPC responder replay or OMP --restore. Restore context, not live process state.

Repo /Users/addomam/projects/amux-acp, isolated feat/agent-conversation-recovery, baseline cc468a3; substantial preexisting E07/startup changes are intentionally preserved. Review E08 and introduced regressions, not a redesign of existing native best-effort interaction. No install/commit/merge/push/release/global daemon/config/auth changes or live providers are authorized. E05 stays parked. Historical native-buffer/application settlement remains unknown/fail-closed; force is not ordinary safe-grant proof and never implicitly interrupts or kills admitted work. Historical review/provider caps do not reset.

## Required reading and frozen inputs

Read CONVENTIONS.md; specs/epics/e08-conversation-recovery/PLAN.md, epic.yaml, e08s01-story.md and e08s01-tasks.yaml; specs/product/SCOPE_E08.yaml; specs/security/epics/e08/THREAT_MODEL.md; specs/verifications/AUDIT-e08-e08s01.md, e08-review-response.md, e08-recovery.json and e08-recovery-r3-runtime.json. Ledgers intentionally remain failing pending independent acceptance.

Authoritative source: specs/verifications/e08-review-r3-source.json contains20 SHA-256 values. Independently recompute every listed source hash with read-only shasum or another actual digest tool; report matched/required and any unverified files. Parent digest output is not your independent measurement. Debug binary4f1d134f7f6afa5c3ddfd1ab5676dac1df3cca9e7862659f3d9b64101391c34d was measured before/after all3 current smokes; installed binary is unchanged. No source edits during review; metadata outside the20-source set may be updated by parent.

Core source: src/recovery.rs, src/daemon/recovery.rs, src/daemon.rs, src/acp.rs, src/codex.rs, src/integration.rs, integrations/omp/amux.ts and src/runtime.rs, plus journal registration/dependency files. Relevant behavioral tests include tests/recovery.rs, tests/integration.rs, tests/fixtures/acp_peer.rs, tests/agent_interaction.rs and native fixtures tests/acp/native.rs and tests/pty_control/native.rs. README/CHANGELOG are frozen too. Read full relevant sections, not only a diff or prior findings.

## Historical findings — challenge their actual clearance

R1 A75 (6must/2should), B81.25 (3must/3should) and R2 A84.375 (2must/3should), B93.75 (1must/1should) all FAILED. Complete verbatim reports e08-review-r1-A.json/-B.json and e08-review-r2-A.json/-B.json; consolidated dispositions in e08-review-response.md. R1 matched19/19 both, R2 A19/19 while B19 unverified because its security-only tool profile could not hash. Both current reviewers are full code reviewers with security focus and digest tools. Two of maximum5 E08 rounds were used before this third pair. No PASS transferred to new source.

Challenge the WHOLE feature, including these repaired transitions:

- Bounded closed owner-only journal, canonical socket namespace, regular/no-follow/nonblocking reads, immutable boot+exact birth-qualified peer exclusion, flock/inode/bytes conflicts and atomic file+directory sync. Never kill old reported peers. Unknown settlement or uncertain writes block rather than duplicate a native agent.
- Saved intent and target-local epoch precede closable publication. Fallible native snapshot precedes publication/application admission. Close retires intent before cleanup and no late rollback/checkpoint may revive it. Unrelated targets must not invalidate each other.
- R2 target checkpoint scope is acquired BEFORE candidate/native/ACP observation. Queued newer notifications must not silently disappear on candidate mismatch. Selected root quit shares scope and can invalidate a captured candidate even when no current candidate exists; displaced/retired/nested roots stay inert.
- Sequence-qualified coherent project metadata across all retained group records, including project-save, project-config and automatic main-shell context mutation, with no filesystem work under topology guards. Cache survives retained-journal groups but is pruned when neither runtime nor retained record needs it.
- Native root lifetime is not numeric PID. OMP unavailable/nondurable current context and managed-Codex known nonauxiliary replacement explicitly block old trusted intent; sticky Codex replacement survives later diagnostics and cleanup. Never manufacture B's identity or reopen A as fallback. Later valid metadata needs actual current header/pin.
- Exact retained native preparation cleanup: no missing handles/map entry treated as proof. Uncertain children/backend/resources remain charged across cleanup/drop/connect/publication/worker-start paths. Preparation admission cannot exceed capacity through retained failures. Shutdown publishes stop/cancellation and joins in-flight preparation BEFORE snapshot and daemon request-handler drain.
- ACP advertised load/resume only, correct exact session/history/auth retry/settings, no new-session fallback or work replay. Fresh lifetimes and ownership after reopening. Accepted saved256-byte names retain identity while new ACP launch128-byte boundary stays unchanged; ambiguous semantic categories retain exact acknowledged option IDs.
- Native input/ownership policy is unchanged. Native fixture fixes fence actionable paint, never replay discarded activation. Mouse auth fixture waits for preceding Prompt navigation through visible read-only paint and observes the unsupported choice before switching; Tab/Enter focus remains unchanged.

## Parent verification — read evidence, DO NOT rerun

READ ONLY: no builds/tests/lint/formatters/smoke/amux/agent/provider/helper binaries, filesystem mutations, global auth reads, branches/worktrees/config/Git changes. Read-only checksum binaries and source/file/metadata inspection ARE permitted and REQUIRED for independent source hashes. Do not contact the other current reviewer or read any current R3 peer output/report. Historical combined findings are allowed.

- bg_37/artifact3429: examples,579 workspace tests/31 suites/12 ignored, strict all-target Clippy, fmt check and build PASS,133.18s wall. Tool's aggregate0.00s is not real test duration. No live ignored test ran; the intentionally ignored private fake-backend helper may be launched by its owned fixture.
- Focused3405:19 recovery,29 journal/coordinator units,8 native Codex units,9 integration, stock OMP8 tests49 assertions and Zsh syntax PASS. Real deterministic managed-Codex nonaux replacement/cold restart refuses old A without another native launcher call.
- bg_34 exposed real native preparation shutdown nontermination. Final3413 passes real private daemon version-probe and post-spawn startup gates with daemon exit0, owned child gone and private cx roots absent before fixture cleanup; native interaction24/1ignored,8 Codex,19 recovery and strict checks/build also pass.
- bg_36/artifact3417 stopped at native auth mouse fixture. A removed owned-process diagnostic proved old readiness returned against old armed paint while native process group was stopped and preceding Prompt click unconsumed. A first global paint fence reset Tab/Enter keyboard focus and failed; corrected mouse-only fence plus actual unsupported-selection observation passes all3 exact native auth routes in4.08s. No production input gate change/retry/replay.
- e08-recovery-r3-runtime.json: SAME current binary measured before/after all3 actual checks. Two exact ACP contexts42/17/one command before fresh57, fresh ownerless identities/stale-target rejection and actual Menu Ready entries/no picker. Actual installed OMP18.13.3 exact-file reopening in two new lifetimes, unchanged messages/no --restore. Original persistent physical ctty attachment performs real /new at same native PID; new ID observed, old A never reopened on restart, unsaved B not invented. Actual TUI/host/daemon exits0, termios while ctty host alive, drainer joined, owned peers/PTYs/socket/root absent and unrelated sentinels survive before fixture fallback. Private HOME/state/fake key, sandbox network/auth denial, provider0.

Limits: no exact CPU pause in old registration gap/global clock shift; no claim every new row individually failed old source. No Linux/minimum-Rust/new genuine provider/installed native Codex0.165 acceptance. Existing quantitative coverage waiver unchanged; no bundled scripts or invented script gate. R1/R2 and pre-final578 full proof remain source-qualified historical evidence, not current acceptance.

## Required report

Review correctness, repository conventions, behavioral F.I.R.S.T quality, design/clarity/Fowler smells, performance/bounds/resource cleanup, edge cases and security with full security focus. Findings need current exact file:line, causal consumer-visible scenario, category must-fix/should-fix/consider and confidence. Supported HIGH/CRITICAL security confidence>=8 blocks. Exclude intentional malicious same-UID trusted-owner rewriting, not genuine unsafe objects/lifecycle inputs. Actual CONVENTIONS.md governs; do not promote generic20-line/300-line heuristics into repo rules.

Give a finite explicit denominator of distinct reviewed criteria, must/should counts and score100*(total-must-should)/total; no denominator inflation or coverage conflation. PASS requires zero must-fix and score>=94; parent addresses every should too. Include clearance/remaining defect for historical consolidated issues, independently measured source matches, exact actions executed and not executed, verification limits. Full report text, no edits. Parent will read BOTH complete raw reports before any remediation. Maximum5 E08 rounds remains hard.
