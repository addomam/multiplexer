---
bug_id: e03-removed-shell-settlement
type: bug
context: e03 scoped Build review round3
status: fixed
independent_review: round5-no-findings-overall-gate-failed
severity: medium
scope: removed-PTY-exact-shell-settlement-consumer
review_evidence: specs/verifications/e03-build-review-round3.json
---
# Removal drops the consumer for outstanding shell acknowledgements

Independent Build round3 A traced daemon removal through Integration::remove: the ownership registry retains outstanding calls, but the integration record containing their exact execution-to-authority lookup is dropped. A genuine late shell report fails before reaching Authority::settle, permanently stranding the old call and its retained target. The existing direct-Authority regression did not exercise this production consumer.

## Approved-contract correction

Keep only exact unresolved execution/authority settlement lookup after removal, not shell context, environment, agents, FIFO or source files. Remove filesystem resources immediately. Allow only exact old accepted/rejected callbacks; removed acceptance is a negative permission acknowledgement, never fresh work. Drop each lookup after definitive settlement and the container when empty. Ordinary source, wake, context, ready and agent routes remain closed. Preserve old target identity even if a live terminal has its UUID as a display name; never retarget a late report.

## Red/green verification

Buffer a genuine stock-Zsh acceptance report at an isolated public-wire proxy, observe the real unknown call, close and remove the target, and forward the original report. Assert the old claim receives settlement but no offer; new work and unrelated reports stay rejected. A same-name replacement remains untouched. Verify settlement lookup reclamation and immediate private-root removal, not just the ability to call Authority directly. No model, auth or global changes. Clean up owned proxy/daemon/PTY/files, run full approved preflight and actual consumer smoke, then require fresh round4 review. Preserve round3 FAIL, the historical caps and failing complete-broker acceptance.

## Exercised correction

The corrected real-consumer regression failed before the fix (artifact517,6.31s): its original callback was incorrectly resolved to a same-name replacement. After the fix it passed6.26s in final artifact528. The removed target's FIFO/root are observed before removal and absent afterward; unrelated and unknown-ID reports fail while the original call remains unknown. The buffered genuine callback negatively acknowledges that exact old operation, clears its retained ledger without offering control, leaves replacement ownership untouched and consumes its settlement-only lookup. Duplicate callbacks fail integration-lost. All307 Rust tests passed; no model/auth/global changes. Fresh round4 approval remains required.
