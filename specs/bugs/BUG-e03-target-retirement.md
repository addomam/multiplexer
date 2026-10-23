---
bug_id: e03-target-retirement
type: bug
context: e03 scoped Build review round 2
status: fixed
severity: medium
independent_review: round5-no-findings-overall-gate-failed
---

# Removed PTYs remain in the live ownership registry

Review A-R2-01, evidence `specs/verifications/e03-build-review-round2.json`.

Problem: `Registry::end_target` ends authority but leaves the entry in `targets`. Real open mints a fresh ID, so replacement-only retirement never reclaims removed, empty-accounting terminals. View changes continue emitting control events for removed terminals; memory/event work grows with lifetime churn rather than retained topology.

Root cause: end and actual topology removal use the same registry operation, despite different retention responsibilities.

Fix plan: explicit registry removal/retirement on actual remove and unpublished discard; keep end-only behavior for retained closed terminals. Stop updating ended targets with new view facts. Existing Arc-bound authorities/claims and required retired history retain unresolved calls and truthful late settlements; never erase outstanding work or revive authority.

Verification: reproduce ghost events through the actual private daemon with repeated open/close/remove followed by a validated original-attachment view change. Verify the same path after the fix, plus ended removed-claim late settlement and stale-authority rejection. Run approved full preflight and actual consumer smoke. Keep this bug failing until exercised proof; fresh independent review required.

## Exercised result

The public-daemon regression failed before the fix (`artifact://447`) and passed afterward (3.01s). The existing ended-claim regression now removes the target before late settlement, rejects stale authority, preserves the old outcome without an offer, closes its controllers and proves the old allocation is reclaimed. Final approved preflight passed304 Rust tests/10 ignored/29 suites plus strict checks, showcase and OMP/Zsh (`artifact://463`).

Actual separate CLI/controlling-PTY smoke opened/closed/removed four terminals, observed no later ghost view events, completed deliberate native yield and explicit safe acceptance, executed stock-Zsh source, blocked an observer key without replay, detached and shut down cleanly in0.245s. No model/auth/global changes or force cleanup. See `specs/verifications/e03-lifecycle-runtime.json`. Fresh round3 approval is still required before landing.
