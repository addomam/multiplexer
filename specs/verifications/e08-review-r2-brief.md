---
type: review-brief
context: {project: amux, epic: e08, story: e08s01, round: 2, baseline_commit: cc468a3, security_sensitive: true}
---
# E08 independent round2 — complete feature and corrective integration

## Goal and limits

User authorized implementation: on amux relaunch, reopen all previously open **supported exact saved conversations**, with cwd/project/name, then wait for fresh user input. No recovery picker, new/latest fallback, continuation/notification prompt, interrupted turn/tool/subagent retry, permission answer, queued terminal input, old handles/control/RPC responder replay or OMP --restore. Load context, not running process state.

Repository: /Users/addomam/projects/amux-acp; isolated feat/agent-conversation-recovery, baseline cc468a3. Preexisting dirty E07/startup fixes are preserved. Review E08 and regressions it introduces, not a redesign of existing native best-effort interaction. No commit/install/global daemon/config/auth change, live-provider run, merge/push/tag/release. E05 stays parked; historical E03 native/application settlement remains unknown/fail-closed, force is never proof of a safe ordinary grant and never implicitly interrupts/kills admitted work. No historical review/provider cap reset.

## Frozen source and reading

First read CONVENTIONS.md, the E08 PLAN.md/epic.yaml/e08s01 story+tasks in specs/epics/e08-conversation-recovery, specs/product/SCOPE_E08.yaml, specs/security/epics/e08/THREAT_MODEL.md, specs/verifications/AUDIT-e08-e08s01.md, e08-review-response.md and e08-recovery.json. State/execution/tasks intentionally remain failing pending independent clearance.

**Authoritative source set:** specs/verifications/e08-review-r2-source.json lists19 source SHA-256 values. Independently recompute those exact hashes; report matched/total. Debug binary c9bce8c7c4b8be8b00c29aee9501f8bf399a2daf1ccf8de3c165989c82ba9781; installed binary remains unchanged. Source changes are frozen during review; parent may only update evidence/ledgers outside this manifest.

Core files: src/recovery.rs (bounded typed journal, file safety/locking/fsync/conflicts, boot/birth process pins); src/daemon/recovery.rs and src/daemon.rs (record/startup/close/teardown and shared routing); src/acp.rs (advertised load/resume, exact session/history/auth/settings and peer cleanup); src/codex.rs (native backing store and exact retained-instance failed-launch cleanup); src/integration.rs and integrations/omp/amux.ts (root lifetime/current context). src/runtime.rs exposes existing native start identity; src/lib.rs/Cargo.toml/Cargo.lock register the journal/SHA dependency. Relevant deterministic tests: tests/recovery.rs, tests/integration.rs, tests/fixtures/acp_peer.rs, native handoff driver fences in tests/acp/native.rs and tests/pty_control/native.rs. README/CHANGELOG/CONVENTIONS are in the19-source set too. Use existing callers/types and direct references as needed.

## R1 and corrective focus

R1 A75 (6must/2should), B81.25 (3must/3should) FAILED; verbatim historical reports e08-review-r1-A.json and -B.json. One of maximum5 E08 rounds is consumed before this round. Consolidated dispositions are in e08-review-response.md. Check the full feature independently, not just whether those findings disappeared.

Critical transitions to challenge:
- Numeric PID equality is not a lifetime. Native root replacement refreshes actual birth-qualified pin; Darwin immutable kern.bootsessionuuid replaces wall-clock-adjustable boottime. Primary XNU proof is linked; no global clock-shift experiment is claimed.
- Saved intent and target-local epoch are staged before closable native/ACP publication. Native fallible snapshot precedes publication/application command admission. Close retires intent before cleanup; missing-row rollback must not resurrect it. Unrelated target close must not discard another checkpoint.
- Sequence-qualified active-project cache harmonizes every persisted row in a group, including project-save/new conversation and out-of-order observation. Check bounded retention and lock ordering.
- Native preparation failures distinguish conclusive no-child/complete retained-instance cleanup from unknown. Missing map/handles is not proof; retained resources/identity uncertainty block. Preserve original public diagnostics and admitted work.
- Unsupported/nondurable/invalid active native root is explicit observation, not None. Last trusted reference may remain **typed inactive** but cannot reopen as fallback; valid current metadata/header/fresh pin can clear the block. Nested, displaced, late shutdown/heartbeat/end and retired registration must not replace/revive selected root.
- Nonblocking/no-follow native backing-file open checks regular/owner/header before reading; unsafe FIFO cannot stall other recovery or shutdown.
- Internal reopening accepts already-valid256-byte saved names, without broadening ordinary new ACP explicit128-byte validation. Exact acknowledged options survive duplicate semantic categories.
- Keep privacy, bounds, launch namespace binding, safe per-daemon attempts, failure visibility and resource accounting; no generic retry or speculative API.

## Verification supplied by parent — do not rerun

**READ ONLY. DO NOT run build/tests/lint/formatters/smoke/provider/agent binaries, read global auth, create branches/worktrees or mutate files/config/Git.** Parent already owns the final runtime verification; concurrent repeated checks would add interference. Read supplied raw output/evidence and perform source/hash analysis only.

- bg_25, artifact://3374: examples +571 workspace tests/31 suites/12 ignored +strict all-target Clippy/fmt/build PASS,128.07s wall. Tool aggregate0.00s is not real test timing. No live ignored test ran.
- bg_22, artifact://3354:26 recovery-filtered units (25 journal +1 actual-process stale pin),17 public recovery,9 native integration,6 Codex, OMP8/49 and Zsh syntax PASS.
- bg_24 targeted real native safe-offer fixture +strict checks/build PASS after3357 captured Preparing Yield. No production input/ownership relaxation.
- e08-recovery.json: debug hash measured **before and after** final actual CLI/Menu, native exact-file reopen, and real original-attachment OMP /new+restart checks. Two distinct ACP contexts retain42/17/one command each before fresh57; fresh ownerless identities/stale target rejection; real Menu lists both with no picker. Actual OMP18.13.3 loads same exact file in two new processes without message/work replay or --restore. Physical original attachment /new observes new conversation at same PID; old A does not reopen after restart, unsaved B is not invented. Successful TUI/host/daemons exit0; termios checked while real ctty host alive; drainer joined; owned peers/PTYs/socket/root absent and unrelated sentinel survives before fallback. Network/auth sandbox, private state/fake key, provider0.
- Failure history is retained and source-qualified.3352 boot validator migration and3357 fixture readiness were fixed;3361/3363/3367/3369 are throwaway harness failures, not production defects. R1's555-test/runtime proof is historical only.

Coverage limits: no exact CPU pause in old publication-registration gap (ordering plus real starting-load close tested), no actual global clock mutation, no claim each new row had functional RED on R1, no new genuine provider/installed native Codex0.165/Linux/minimum-Rust acceptance. Existing quantitative coverage waiver unchanged; no bundled bigpowers scripts or invented mechanical gate.

## Required independent output

No contact with the other current reviewer and **do not read any current R2 peer report/output**. Historical R1 combined findings/response are allowed. Stay blind until both reports finish.

Report correctness, conventions, F.I.R.S.T behavioral quality, design/clarity/Fowler smells, performance/resource cleanup, edge cases and security. Findings require concrete current file:line, causal consumer-visible scenario, category must-fix/should-fix/consider and confidence. HIGH/CRITICAL security findings at confidence8/10+ block; exclude intentional malicious same-UID trusted-config rewriting, not real unsafe object/lifecycle inputs. Do not turn generic20-line/300-line skill heuristics into repository rules; actual CONVENTIONS.md governs. Name coverage limitations, not invented execution.

Give an explicit finite denominator of distinct reviewed criteria, must/should counts and score100*(total-must-should)/total; no denominator inflation or coverage confusion. PASS requires zero must-fix and score>=94. Include each historical consolidated issue's clearance or remaining defect, hash matches, exact actions executed and those deliberately not executed. Return full report text, no edits. Parent reads both entire verbatim reports before any remediation; max5 rounds remains hard.
