---
id: e06s01
type: story
maturity: 3
risk: P0
bcps: 5
status: passing
---
# Real OMP ACP compatibility

## 1 Identity
e06s01 under e06, explicit user three-item authorization.

## 2 Value
Real OMP ACP compatibility for daily ACP use, not a new integration protocol.

## 3 User
Human operator and local external controller.

## 4 Problem
ADDED: The installed OMP ACP server and amux share the actual live agent, genuine form and exact permission responders; real requested settings are acknowledged, never inferred.

## 5 Scope
Only this story outcome; PLAN.md is the exact shared interface/ownership contract.

## 6 Requirements
ADDED: The installed OMP ACP server and amux share the actual live agent, genuine form and exact permission responders; real requested settings are acknowledged, never inferred.

## 7 Preserved requirements
Force/cancel/conditional semantics, exact original responders, no replay, stock Zsh/native OMP, historical caps and native limitations unchanged.

## 8 Dependencies
Delivered e04; e06 shared bootstrap/profile interfaces and feature-branch baseline before production edits.

## 9 Module purpose callers and contracts
See PLAN.md and ../../IMPACT_LATEST.md; parent owns integration and shared registrations.

## 10 Design
Reuse existing broker, NativeControl and original controller/agent connection; no generic provider/plugin/second authority.

## 11 Interfaces
Closed bounded settings and concrete profiles; original provenance; canonical configuration.requested/reported.

## 12 Data
Runtime memory-only; configuration on disk; public evidence metadata-only.

## 13 Errors
Unsupported/missing/unknown fails explicitly; no model fallback, automatic task action or replay.

## 14 Security
No new supported security findings in affected paths; apply ../../security/epics/e06/THREAT_MODEL.md.

## 15 Resources
Observe owned child/endpoints/workers/native host/drainer/termios/socket/root and unrelated sentinel before fallback.

## 16 Implementation steps
1. Protocol and genuine settings interop with offline regression boundaries → verify: `cargo test --test acp e06s01_ -- --nocapture`
2. Actual installed-server and bounded real-subscription same-session native/API acceptance → verify: `cargo test --test acp e06s01_live_omp -- --ignored --exact --nocapture`

## 17. Acceptance criteria
Given one OMP ACP session, native/API answers and exact permission deny/allow resume its original work. Denied file effect is absent, allowed effect exists, tuple/PID stable, force never interrupts, and owned cleanup precedes fallback.

## 18 Verification
Select nonzero expected cases; actual built CLI/controlling-PTY smoke and source-bound evidence, not tests alone. Live acceptance requires the separately approved bounds and cannot pass when ignored.

## 19 Out of scope
Crash restoration, native fence repair, second protocol, global changes and main merge/push/release.

## 20 Evidence and completion
All tasks start failing; update only after actual command/runtime proof and fresh review. Preserve failures, no historical cap reset.
