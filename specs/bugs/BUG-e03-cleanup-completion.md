---
bug_id: e03-cleanup-completion
type: bug
context: e03 scoped security review round 2
status: fixed
severity: high
independent_review: round5-no-findings-overall-gate-failed
---

# Concurrent managed close does not wait for teardown completion

Review SEC-R2-01, evidence `specs/verifications/e03-build-review-round2.json`.

Problem: `Instance::close` uses `stop.swap(true)` to return early for subsequent callers. Stop-requested is not cleanup-completed. A detached PTY-exit callback can still be joining its worker or terminating its owned backend when Broker shutdown returns; daemon process exit can abandon that work and private-root removal.

Root cause: duplicate cleanup initiation was guarded, but completion was not serialized. Existing `Host::drop` forcibly cleans fixture resources before some assertions and cannot prove daemon-owned completion of this race.

Fix plan: a dedicated per-instance cleanup-completion mutex serializes close callers through the full existing teardown. Set stop before socket shutdown/join; no topology/authority/state lock spans joins or process cleanup. Preserve birth-qualified owned termination and truthful incomplete-cleanup state.

Verification: deterministic no-model regression blocks the first terminal-end cleanup in a real worker join, concurrently calls Broker shutdown, then releases the worker and checks owned backend/descendant reap and private-root removal before any test fallback. Strengthen the existing public-daemon shutdown case to inspect completion before Host drop. Observe failure before the fix, then focused/full checks and actual private-daemon smoke. No real auth/model/global configuration. Keep this bug failing until proof; require fresh independent security/general review.

## Exercised result

The deterministic Broker end/shutdown regression failed with premature return before the fix (`artifact://446`) and passed afterward (0.49s). It holds a real worker join, then verifies actual owned backend/descendant disappearance and private-root removal before its resource fallback. The strengthened actual-daemon shutdown case passed1.43s and now checks managed-root/process removal before dropping Host. No credential/model access is needed.

Final approved preflight passed304 Rust tests/10 ignored/29 suites, strict checks, showcase and OMP/Zsh (`artifact://463`). Separate actual CLI/TUI lifecycle smoke ended with detach0/shutdown0, all owned shells gone and its private root removed, without forced cleanup. See `specs/verifications/e03-lifecycle-runtime.json`; the focused proof is retained in `artifact://456`. Fresh independent round3 security/general approval is still required before landing.
