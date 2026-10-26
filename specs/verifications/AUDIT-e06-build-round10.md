# E06 Build self-audit — final source round10

Type: implementation-self-audit. Context: ACP daily use on feat/acp-daily-use, base2195b05adf23ae26a80065ecdd6d9196e2915998.

**READY for independent source/security review; not source approval or live acceptance.**

| Area | Result and evidence |
|---|---|
| Correctness | ✓ Atomic complete native edits retain per-field rejection until deliberate correction; bg_344 actual consumer RED, bg_345/353 GREEN. Exact rendered new-target/session readiness replaces ambiguous old-anchor fences. No application input is replayed. |
| Input-reader correctness | ✓ Released crossterm0.29.0 bounded readiness cursor retains unread TTY tokens and remaining signals; zero-time level readiness before rereading borrowed blocking stdin avoids fd-flag changes and introduced bg_350 stalls. Existing real native resize/F10 case passes in full/default/focused suites and final actual binary. No sleeps, key retries, extra thread or alternate zero-time-broken backend in production. |
| Security/privacy | ✓ All92 scoped source paths secret-pattern scan has no matches. Exact ownership/permission/identity boundaries preserved; no auth/global configuration changes or provider calls in this repair. Unsafe BorrowedFd lifetime is tied to retained FileDesc; stack-only zero-time poll. Fresh independent security review remains mandatory. |
| Supply chain | ✓ Existing MIT crossterm0.29.0 revision36d95b26a26e64b0f8c12edfe11f410a6d56a812 copied losslessly; all60 baseline hashes matched before editing. Exactly57 remain identical; three changed files are Mio reader, existing Rustix event feature, redundant-parentheses warning cleanup. Root pins0.29.0, one dependency instance. No wholesale upstream-head upgrade/new registry package. |
| Performance/types | ✓ Bounded existing event batch, stack-only poll and allocation-free EOF error; no parent-shell flags, copying queues or unbounded work introduced. Rust types and strict Clippy/check pass. |
| Scope/design | ✓ E05 remains parked; native input defect was an ordinary integration blocker, not reopening the historical native-buffer settlement spike. Force/cancel/handoff semantics unchanged. No speculative framework, shims, suppression or unrelated refactoring. |
| Regression/actual surface | ✓ bg_353 exit0:449 workspace/338 default tests (overlap), strict Clippy/check/build/fmt, cursor showcase, two unknown-settlement guards and42 unique nonlive E06 cases. Actual CLI/controlling-PTY binaryc211a2cc exercises owner/observer settings resize/one-key close and42→57 on same tuple/PID; normal cleanup, sentinel survives, fallbackfalse. |
| Resource/cleanup | ✓ No new herdr sessions/worktrees. Throwaway constructor restored/deleted; all actual-smoke owned PTYs, shell hosts, drainers, peer, daemon/socket/private root cleaned normally. Keep unmerged feature worktree. |
| Provenance/docs | ✓ Existing README/CHANGELOG/PLAN updated; failed runs and full prior reviews preserved, never relabelled. New evidence stays in specs. No commit/merge/push/release. |

## Explicit heuristic applicability

- No bundled churn/slopcheck scripts: neither invented nor claimed run. Used actual installed release, verified60 baseline SHA256s, primary upstream issue/proposal research and real project checks. Public GitHub REST reads were dependency research; no issue creation or credentials. The generic audit template REST prohibition is not an actual CONVENTIONS.md rule; this deviation is explicit.
- Repository CONVENTIONS.md, not generic 300-line/20-line heuristics, is authoritative. Preserved existing large modules and byte-identical released files rather than unrelated decomposition. No new abstraction or Fowler-smell fix is justified by this change.
- Consumer-visible public/real-terminal regressions cover new behavior; no wiring-only per-function tests. Third-party unit suites were not separately executed; workspace/default and actual application surface are the exercised proof.
- No skipped gate rationalized as fixtures or old source approval: all seven task gates stay failing until final fresh source approval; live S01T2 additionally needs separately approved complete subscription-provider run. Current provider8/cap9/1 cannot cover the required max4 fresh run.

## Source review boundary

Final authorized E06 round10/max10: fresh blind A/B plus security, all92 frozen paths, no shared findings or runtime/audit/handoff input, no execution/auth/provider operations. Both source scores>=94, zero supported Must-fix/Should-fix and security PASS; collect/read/archive all reports and verify all frozen hashes before any edit. No round11 is authorized. Historical caps unchanged.
