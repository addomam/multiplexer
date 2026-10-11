# Independent review record

Installed bigpowers `request-review` dual-blind AND gate: both scores ≥94 and zero must-fix; maximum five attempts. Independent reviewers received identical self-contained briefs, no peer reports. Integration owner ran checks once; reviewers were instructed to inspect read-only, not run overlapping builds.

| Round | Result |
| --- | --- |
| 1 | Independent scores 86 and 87; FAIL. Findings remediated and behavior verified. |
| 2 | Review providers returned usage-limit errors; no passing report counted. |
| 3 | Fallback completion reports unavailable after evaluator kernel loss; no passing report counted. |
| 4 | Independent scores 88 and 92; FAIL. Eight concrete findings remediated: queued wake/raw ordering, atomic default parenting, immediate save context, TUI input errors, timestamp-only Git churn, ZDOTDIR changes/removals, inherited GIT_COMMON_DIR and palette query overrides. A further competing immediate/queued wake deadlock was reproduced and fixed before the final pair. |
| 5 | Reviewer A 96.67 (29/30 rubric items), Reviewer B 95 (19/20). Both FAIL: one must-fix, same concurrent-removal ancestry race. |

Final finding: terminal removal cloned ancestry before cleanup and later rewrote surviving children with stale parent data. Concurrent ancestor removals could publish a deleted parent; concurrent promotion/reparenting could create a self-cycle. Downstream ancestry traversal could panic or hang.

Response: `Daemon::dispatch("remove")` now takes the current removed Entry from the state map under the same lock as child rewrites, returning not-found if another request already removed it. No stale ancestry is used after cleanup. Permanent real-daemon test `concurrent_ancestor_removal_keeps_every_surviving_parent_valid` reproduced a dangling parent before the fix (separate artifact 181, exit101); passed after the fix over 24 three-terminal races, with cycle/missing-parent checks and a subsequent public reparent operation. Final full preflight passed: [exact output](preflight-e01.log).

Owner self-audit and mechanical/runtime verification find no known remaining implementation defect. This is not an independent post-fix PASS. **Review cap exhausted (5/5). Human decision required.** No sixth round, merge, release tag or deployment performed. Local feature work remains available for human inspection; correctness tests do not waive the review gate.