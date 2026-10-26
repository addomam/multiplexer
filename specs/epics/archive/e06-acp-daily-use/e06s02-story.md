---
id: e06s02
type: story
maturity: 3
risk: P1
bcps: 4
status: passing
---
# Convenient explicit ACP launch

## 1 Identity
e06s02 under e06, explicit user three-item authorization.

## 2 Value
Convenient explicit ACP launch for daily ACP use, not a new integration protocol.

## 3 User
Human operator and local external controller.

## 4 Problem
MODIFIED: Before: ACP launch requires raw absolute argv and controller JSON. After: native picker and human CLI launch a selected profile on the original native attachment; existing explicit-controller automation remains available.

## 5 Scope
Only this story outcome; PLAN.md is the exact shared interface/ownership contract.

## 6 Requirements
MODIFIED: Before: ACP launch requires raw absolute argv and controller JSON. After: native picker and human CLI launch a selected profile on the original native attachment; existing explicit-controller automation remains available.

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
1. Profile catalog, explicit human/automation CLI and safe settings validation → verify: `cargo test --test acp e06s02_ -- --nocapture`
2. Actual native picker and built CLI launch/settings/effect/cleanup smoke → verify: `cargo test --test acp native::daily_use::e06s02_native_profile_launch -- --exact --nocapture`

## 17. Acceptance criteria
Given an installed or configured profile, real CLI/picker launches exactly one ACP agent in the selected project. Requested/current-or-unreported settings are visible. Missing executable/profile, invalid or unsupported settings never silently choose another agent/model.

## 18 Verification
Select nonzero expected cases; actual built CLI/controlling-PTY smoke and source-bound evidence, not tests alone. Live acceptance requires the separately approved bounds and cannot pass when ignored.

## 19 Out of scope
Crash restoration, native fence repair, second protocol, global changes and main merge/push/release.

## 20 Evidence and completion
All tasks start failing; update only after actual command/runtime proof and fresh review. Preserve failures, no historical cap reset.
