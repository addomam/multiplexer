---
bug_id: BUG-e06-native-fixture-cleanup
status: fixed
severity: high
scope: test-fixture
title: ACP native fallback cleanup can block preflight indefinitely
---

## Reproduce
Original unchanged-production preflight bg_263/artifact2015 compiled and passed126 library/16 general acceptance tests. ACP native complete acceptance did not finish for2595.28s; parent finally terminated only pinned test57960 (exit101/signal15). This is failing-before evidence, not a repeated reproduction or PASS.

## Isolate
Mac sample of test57960 shows remaining native complete acceptance thread in std Child::wait during Drop, symbolicated as Attachment::drop native.rs374. Its sole observed child59010 was the exact private fixture daemon, so the symbol does not establish which Child handle was involved. Original endpoint/root/PID evidence is in e06-baseline-stall.json; both stack captures are private temporary artifacts. No model/auth/provider execution occurred.

## Hypothesize and verify
Confirmed cleanup defect: fixture Drop paths use unconditional blocking Child::wait after best-effort signals, so failed cleanup can consume the entire preflight deadline instead of reporting failure. Native fallback additionally stops the PTY drainer before host/group teardown; that ordering can obstruct Darwin terminal teardown. Do not claim the original application/assertion failure is known. Preserve it when bounded cleanup permits reporting.

## Fix plan
Keep drainer alive until the original host/group has been signalled and observed reaped; then stop/join it. Replace fixture-only unconditional waits with deadline-bounded try_wait, preserving already-reaped statuses, avoiding signals to observed-ended/reused children, and reporting unknown cleanup as a failure (not a clean oracle). Apply to ACP Host, Attachment and Sentinel fallback paths. Do not weaken normal detach/termios/owned-cleanup assertions or alter production ownership.

## Verification
Run exactly one existing native complete case with --nocapture to surface original failure or confirm normal completion after the repair; then full unchanged-production preflight. No new fixture-text/mock-echo permanent tests. Actual CLI/PTY E06 acceptance and source-bound regressions remain mandatory after feature integration. Bug stays open until these gates succeed.

Command: `cargo test --test acp native::acceptance::e04s04_native_complete_two_surface_forms_permissions_and_running_handoffs -- --exact --nocapture`

Verified on the kept feature branch: exact repaired native case bg_268 passes1/2.35s, then repaired baseline bg_269 passes291 tests plus strict Clippy/build. Final integrated bg_299/artifact2131 passes426 workspace and315 default tests (overlap), strict checks and native fail-closed guards; independent final built-CLI/controlling-PTY smoke confirms normal termios/host/drainer/peer/daemon/socket/root cleanup and sentinel survival without fallback. Evidence: `../verifications/e06-baseline-repair.json`, `../verifications/e06-final-regressions.json`, `../verifications/e06-native-cli-smoke.json`. Original application/assertion cause remains unknown; this resolution is for the confirmed unbounded/ordered fixture-cleanup defect, not a retroactive PASS.

## Resource disposition
Private daemon59010 was exact parent/root/birth-bound before TERM; it exited. A subsequent KILL guard refused defunct/nonmatching identity, so no KILL was sent. Test57960 then explicitly terminated. A final ps for57960/59010/59025/59027 returns no processes (bg_267). This was manual fallback, never normal no-fallback proof. Original dirty checkout, global daemon/config/auth and historical caps untouched.
