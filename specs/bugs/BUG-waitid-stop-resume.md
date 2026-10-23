---
bug_id: waitid-stop-resume
type: bug
context: actual e03 native focus and owned-cleanup integration smoke
status: fixed
severity: high
scope: PTY-root-reaping-and-shutdown
---

# Non-exit waitid notification stranded the cleanup lock

The real native focus/yield smoke reached retained unknown, deliberate owner yield, no safe offer/new owner, and actual old-focus consumption after resume. Native detach exited0, but private daemon shutdown exceeded its deadline. A process sample showed main blocked in `Terminal::close` acquiring the process-tree mutex while the reaper held that mutex inside blocking `child.wait`; the original shell was still alive.

A separate private C diagnostic reproduced the OS seam: on this Darwin host `waitid(P_PID, owned_child, ..., WEXITED | WNOWAIT)` returned success with `si_code=CLD_STOPPED` (5), not exit. The old code treated every successful return as an observed exit, acquired the cleanup mutex and blocked waiting for a living resumed shell. This is an actual existing lifecycle defect, not the accepted missing Codex input fence.

## Fix

Zero-initialize siginfo storage and accept only CLD_EXITED, CLD_KILLED or CLD_DUMPED before acquiring the process-tree cleanup lock and reaping. Non-exit notifications retain WNOWAIT and back off20ms before retry, avoiding a busy loop while the child is stopped. Actual exit still retains the original zombie/birth identity until descendants are discovered and reaping is serialized with cleanup. Do not simply unlock around every reap and reintroduce PID-reuse signaling races.

## Verification and cleanup

The focus regression explicitly requests shutdown and requires exit0 within three seconds; fixture fallback kill cannot satisfy it. Before the reaper fix it failed at that assertion (bg_104, exit101,10.60s test runtime). After the fix, it passed in0.82s and all302 Rust tests/10 ignored, strict workspace preflight, one independent showcase and eight OMP tests passed (`artifact://406`). The actual outer-TUI stop/focus/yield/resume smoke passed in0.762s, including clean shutdown/reaping without force cleanup. See `specs/verifications/e03-focus-runtime.json`. Fresh independent round2 remains required.

The failed smoke's private shell was birth-verified, then explicitly killed only as owned fixture cleanup; this released the blocked reaper and daemon exited0. Its TUI had already exited0. Reader joined and the private root/sample were removed. No user/global daemon, auth, configuration or model was touched. The temporary C probe's owned child was killed/reaped. All probe/build/profile/download resources were subsequently removed after evidence extraction and a no-open-reference check; see e03-current-delivery-coverage.json. Linux execution is not claimed. Round2/3 later failed separate lifecycle findings; fresh round4 is the current landing gate.
