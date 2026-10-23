# e03 round-2 lifecycle remediation impact

Target: current authorized e03 delivery, independent Build/security round2 findings. No Elaborate/Plan requirements reopened.

## Dependents

- `Registry::end_target`: daemon PTY-exit callback, explicit close, actual remove, unpublished-open discard and final shutdown; existing end/replacement/late-settlement unit tests. LSP references returned none despite these source-proven callers; exact fallback search was required.
- New explicit target-removal consumer: actual `remove` and unpublished discard only. Close/exit retain a visible ended terminal; removal retires it outside the live map. Existing Arc-bound claims/authority must still inspect and settle old work.
- `Instance::close`: preparation cancellation/worker failure, dropped unbound preparation, launch failure, PTY exit and Broker shutdown. Serialize cleanup callers without holding topology/authority/state locks across join or process teardown.
- `tests/agent_interaction.rs` cleanup fixture: current `Host::drop` fallback can hide incomplete daemon cleanup. Check owned backend/resource removal before dropping Host.

## Affected stories

- e03s01–02: target lifetime, stale authority, retained late outcomes and bounded accounting.
- e03s03–04: removed-target view-event fan-out; no conditional grant after termination.
- e03s05–07: same-instance owned cleanup and shutdown completion.
- e03s08 stays failing under the existing narrowly accepted Codex limitation; neither fix supplies the missing application fence.

## Verification gaps and intended corrections

- Add a public-wire open/close/remove churn regression that rejects later ghost view events, plus the retained ended-claim/late-settlement boundary.
- Add a deterministic concurrent end/shutdown regression: hold the first close in a real worker join, invoke Broker shutdown, release the worker, then check actual owned process/descendant reap and private-root removal. No live provider/auth/model.
- Strengthen the existing real-daemon cleanup regression to observe successful daemon exit and resource removal before fixture fallback.
- Run focused red/green checks, the approved full preflight and actual consumer smoke; record exact failures and results. Keep historical and round1/round2 review failures intact and require fresh round3 reviews.

## Risk: High

Shared lifecycle ordering and privacy/resource cleanup; preserve unknown-call retention, generation rejection, noninterrupting force/cancel, stock Zsh/OMP and birth-qualified termination. Boring dedicated completion mutex plus explicit live-map retirement; no new policy, provider abstraction, persistence or retries.

## Round3 late-settlement correction

General reviews failed on two additional lifecycle consumers; security round3 passed without overriding those failures. Preserve all prior findings and caps.

- `daemon.rs`: retain only private automatic launch leases after the execution attempt; retire them through normal release/close on exact late outcomes or target end. Registration races require an immediate settled/ended check. Caller-owned managed control stays external.
- `control.rs`: allocation-free authority queries for private-lease retirement and exact outstanding-call retention; neither query grants input or certifies native application drain.
- `integration.rs`: retain unknown source files until authoritative outcome/removal, and retain only outstanding execution/authority lookup after target removal. Remove filesystem/context/agent resources immediately. Active-to-retired publication is atomic under the existing integration lock; no new input path.
- Report routing: a stable UUID remains an exact target identity even if a replacement has that UUID as its display name. Release topology lock before report handling/retirement; no topology/ownership lock spans filesystem or socket I/O.
- New actual public-wire regressions delay genuine native acceptance past the real deadline and continue the original callback after sender termination. Red startup proof: artifact513, inaccessible owner and exit127. Red removed-report proof: artifact517, callback incorrectly resolved to a same-name replacement. Initial artifact515 was a fixture snapshot-shape error, corrected before the valid reproduction.

P0 compatibility: preserve unknown calls, noninterrupting force/cancel, no replay/reacquisition policy, stock Zsh/OMP and all accepted native-fence limitations. Parent remains sole integration owner; these edits share daemon/report routing and are sequenced, not delegated.
