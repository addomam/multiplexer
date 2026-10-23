---
bug_id: e03-late-launch-retirement
type: bug
context: e03 scoped Build review round3
status: fixed
independent_review: round5-no-findings-overall-gate-failed
severity: medium
scope: automatic-startup-control-late-settlement
review_evidence: specs/verifications/e03-build-review-round3.json
---
# Automatic launch ownership survives exact late settlement

Independent Build round3 A and B found the same bug: startup retires its private controller only in its execution thread. The five-second acknowledgement deadline retains an unknown call; a later real shell report settles that call after the thread has ended, without retiring the inaccessible automatic owner.

## Approved-contract correction

Retain private automatic launch leases after the execution attempt, then consume exact late accepted/rejected outcomes to release and close only that lease after its calls settle. Target end must retire the private registration without inventing settlement. Caller-owned startup control is not retired. Release of an old generation must not affect a newer forced owner. No polling/reacquisition scheduler or command replay.

The actual stock-Zsh delayed callback regression reproduced both defects (`artifact://513`, exit101,5.95s): ownership remained with the inaccessible automatic launch controller after its call settled, and the queued source finished exit127 because the timeout had deleted its file. Keep the original private source file while dispatch is unknown; this is completion of the original native operation, not replay. Definitive rejection or target removal must clean it.

## Red/green verification

Use an isolated public-wire proxy to delay a genuine stock-Zsh acceptance report beyond the actual deadline. Observe the unknown call and automatic owner before release. After the exact callback, require private ownership retirement, one execution, safe cooperative acquisition and no replay. Cover forced revocation before the delayed acceptance: genuine rejection must preserve the newer owner and shell process. No live model/auth/global configuration. Clean up the proxy, connections, owned daemon/PTY and private files on success and failure. Run approved full preflight and actual runtime smoke, then fresh round4 review; keep round3 FAIL and all historical caps.

## Exercised correction

The real delayed-acceptance regression completes the original source exactly once with exit0, removes its private file, retires automatic ownership and permits normal safe acquisition. The forced-owner boundary receives a real negative permission acknowledgement, preserves the newer control and shell PID, cleans rejected source and proves subsequent native shell input works. Final serialized approved preflight passed307 Rust tests/10 ignored/29 suites. Separate actual CLI smoke retained unknown through the deadline, forwarded the genuine report once, completed source0 and shut down0 in0.462s with no fallback and all owned resources removed. See e03-late-settlement-runtime.json. Round3 remains FAIL; round4 is required.
