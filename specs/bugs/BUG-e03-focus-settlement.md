---
bug_id: e03-focus-settlement
type: bug
context: e03 scoped Build review round 1
status: fixed
severity: high
scope: application-focus-input-settlement
---

# Focus reports escaped retained application-input accounting

Both independent Build reviewers found the same non-waived correctness defect. A validated original-attachment view change forwarded application-requested CSI I/O through `protocol_input_authorized`. That queue entry rechecked authority, but set native accounting false. An application paused before consuming focus bytes could therefore retain old-generation input while an empty ledger allowed a falsely safe offer.

This is distinct from the accepted Codex limitation: unknown must block safe grant, not be omitted. Focus is not human activity, but an application-requested focus event is application input, not an automatic terminal query reply.

## Fix

Route the daemon producer through existing `Terminal::input_authorized`, which rechecks authority at dispatch and registers the native unknown batch without recording human activity. Remove the obsolete unchecked-accounting protocol helper; automatic emulator replies are unchanged. No new route, allocation per event, retry, replay or settlement shortcut.

## Observed evidence

`forwarded_focus_blocks_safe_handoff_without_counting_as_human_activity` launches a real private shell consumer, enables focus reporting, stops it before consumption and sends a validated original-attachment focus event. Before the fix, it failed waiting for retained native forwarding (job bg_100, exit101). After the fix, it passed (bg_101): focus is retained unknown, human activity revision unchanged, yield leaves no owner/offer, the resumed consumer actually receives old focus bytes, and the same unknown batch remains.

The test also caught the separate reaper deadlock once strengthened to require clean shutdown, rather than accepting fixture force cleanup. After both fixes, it passed in0.82s, all302 Rust tests/10 ignored and strict workspace preflight passed, one independent showcase and eight OMP lifecycle tests passed (`artifact://406`). An actual outer-TUI focus/yield/paused-consumer smoke passed in0.762s, including exit0 shutdown and owned reaping without forced cleanup. See `specs/verifications/e03-focus-runtime.json` and `BUG-waitid-stop-resume.md`. Fresh independent round2 remains required; no complete-broker PASS is claimed.
