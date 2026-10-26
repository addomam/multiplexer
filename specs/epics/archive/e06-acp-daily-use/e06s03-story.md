---
id: e06s03
type: story
maturity: 3
risk: P0
bcps: 5
status: passing
---
# Truthful same-session reattachment

## 1 Identity
e06s03 under e06, explicit user three-item authorization.

## 2 Value
Truthful same-session reattachment for daily ACP use, not a new integration protocol.

## 3 User
Human operator and local external controller.

## 4 Problem
MODIFIED: Before: broken observation can exit the TUI and lost authority requires manual detach/attach. After: native transport recovery observes the original surviving target, uses fresh attachment provenance and fences all stale/unknown input, with clear lost/ended state and no agent/daemon restart.

## 5 Scope
Only this story outcome; PLAN.md is the exact shared interface/ownership contract.

## 6 Requirements
MODIFIED: Before: broken observation can exit the TUI and lost authority requires manual detach/attach. After: native transport recovery observes the original surviving target, uses fresh attachment provenance and fences all stale/unknown input, with clear lost/ended state and no agent/daemon restart.

## 7 Preserved requirements
Force/cancel/conditional semantics, exact original responders, no replay, stock Zsh/native OMP, historical caps and native limitations unchanged.

## 8 Dependencies
Delivered e04; e06 shared bootstrap/profile interfaces and feature-branch baseline before production edits.

## 9 Module purpose callers and contracts
See PLAN.md and ../../../IMPACT_LATEST.md; parent owns integration and shared registrations.

## 10 Design
Reuse existing broker, NativeControl and original controller/agent connection; no generic provider/plugin/second authority.

## 11 Interfaces
Closed bounded settings and concrete profiles; original provenance; canonical configuration.requested/reported.

## 12 Data
Runtime memory-only; configuration on disk; public evidence metadata-only.

## 13 Errors
Unsupported/missing/unknown fails explicitly; no model fallback, automatic task action or replay.

## 14 Security
No new supported security findings in affected paths; apply ../../../security/epics/e06/THREAT_MODEL.md.

## 15 Resources
Observe owned child/endpoints/workers/native host/drainer/termios/socket/root and unrelated sentinel before fallback.

## 16 Implementation steps
1. Recover attachment transport without mutation replay or automatic reacquisition → verify: `cargo test --test acp e06s03_ -- --nocapture`
2. Actual native loss/pause/reattach/ended-target and no-duplicate-agent smoke → verify: `cargo test --test acp native::daily_use::e06s03_native_reconnect -- --exact --nocapture`
3. Integrated runtime regressions and fresh source review → verify: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo check --workspace --all-targets && cargo build --workspace && cargo fmt --all -- --check`

## 17. Acceptance criteria
Given admitted work/pending interaction, disconnect or pause/reattach preserves tuple/PID/original work. Old drafts, held keys, consent, claims and lost replies are never replayed/rebound. Fresh explicit input can continue work after normal authority acquisition. Missing/ended target stays truthful; owned client resources cleanly exit.

## 18 Verification
Select nonzero expected cases; actual built CLI/controlling-PTY smoke and source-bound evidence, not tests alone. Live acceptance requires the separately approved bounds and cannot pass when ignored.

## 19 Out of scope
Crash restoration, native fence repair, second protocol, global changes and main merge/push/release.

## 20 Evidence and completion
All tasks start failing; update only after actual command/runtime proof and fresh review. Preserve failures, no historical cap reset.
