---
bug_id: e03-smoke-cleanup-identity
type: bug
context: e03 scoped Build review round5
status: fixed
severity: high
independent_review: exhausted-5-of-5-no-post-repair-pass-human-delivery-approved
review_evidence: specs/verifications/e03-build-review-round5.json
verification_evidence: specs/verifications/e03-smoke-cleanup-runtime.json
---
# Opt-in smoke teardown can signal stale owned identities

## Evidence and cause

Independent reviewer A identified P1/must-fix in tests/codex_native_smoke.rs:348–365. Owned::cleanup recorded discover() failure but still signaled previous groups/known snapshots after Child::try_wait() could reap the original children. A failed /bin/ps refresh left liveness acting as ownership authority. The watchdog uses this cleanup too. The original finding was source-derived; the later controlled RED reproduction below signaled only this test's own sentinels, never unrelated user processes.

The user accepted specific upstream Codex limitations, not unsafe cleanup. A97.5%/one must-fix plus B100% remains FAIL despite security's zero-unwaived-HIGH PASS. Scoped round5/max5 is exhausted; no sixth automated review or reset. After the correction and verification below, the user approved limited delivery with “looks great” and overnight/unsigned-commit authorization; see EXCEPTIONS.md.

## Implemented correction

Discovery failure no longer authorizes fallback signals from stale snapshots. Group-wide fallback signals are removed; each nondirect PID needs successful birth/ancestry refresh before delivery. All direct-child IDs are excluded from numeric fallback, including reaped handles; direct delivery requires try_wait() == Ok(None). Child-state errors are failures, not permission to kill. Repeated per-PID validation has a ten-second allowance per signal phase, with existing bounded discovery. Diagnostics, survivor errors, reader/terminal/filesystem cleanup and private-group tracking remain; tracked identities are explicitly not signal authority.

Deterministic no-model regression, actual local owned-process smoke and approved full preflight succeeded. This repairs the reported failure path, not full e03 acceptance or an atomic OS PID-handle guarantee. Final independent review remains FAIL at exhausted5/5; the subsequent user delivery approval permits the earlier requested commit/merge/push without a new independent PASS.


## Human-authorized repair and subsequent delivery approval

The user selected “Repair, then human review.” Fix and verify using only isolated local processes; do not call live models or dispatch round6. Preserve all failed review counters/evidence, then hold merge/push for explicit human review. Reproduce discovery failure using an actual private process-table filesystem error, not a fake success. Exercise both stale group/PID candidates against separately owned sentinel children and retain normal owned cleanup. Revalidate each PID before fallback delivery, omit group-wide fallback signals, bound repeated discovery work, and use direct child handles only while their state proves unreaped. Discovery uncertainty must remain a cleanup error, never signal authority.

## Verification

- RED checkpoint ec6add1, artifact611: the actual private process-table filesystem failure let both stale group/PID candidates kill their separately test-owned sentinels with SIGTERM. No mocks, real provider calls or forced OS PID reuse.
- GREEN: the permanent regression preserves both sentinels, terminates the still-unreaped registered child and removes its private root. It passed the exact local run and is included in the final frozen-source workspace run; e03-smoke-cleanup-runtime.json distinguishes those snapshots.
- Separate actual-source executable smoke, artifact630: healthy teardown terminates a TERM-ignoring owned child and descendant; failed discovery preserves two unrelated-to-Owner sentinels and still terminates the held direct child. Failure remains an error, not successful cleanup, and stale survivor identities are labeled tracked rather than owned. Both roots and all fixture processes are removed/reaped without cleanup fallback; no auth/models/global mutations.
- Final frozen-source preflight artifact635: 308 Rust tests/29 suites/10 ignored, strict workspace Clippy/check/build/format, independent showcase1/3.71s, OMP8/49 assertions, Zsh syntax and CLI help passed. Implementation SHA256: 9d944123a0ed5d65a20b66d9510128a9892b076b1633ab574f9d102928b10e5c. No subsequent source edits.
- e03s08/allfour tasks, the failed all32 report, accepted native-fence/history limitations and all historical review counters remain unchanged. Human review must address this repair before any landing decision.

Subsequent disposition: the user replied “looks great” to the verified repair report and authorized overnight work/unsigned commits. This resolves the pending delivery decision, not historical automated failures or full-broker acceptance. No independent human source inspection is claimed.