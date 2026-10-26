---
bug_id: e03-native-settlement
type: bug
context: e03s08 complete-broker acceptance
status: accepted-deferred-to-future-ACP
severity: high
scope: native-application-settlement-and-answer-presentation
---

# Missing native input cut and authoritative answer reconciliation

## Problem and existing runtime proof

The exact integrated `real_complete_broker_handoff` test failed at its first safe offer after the 15-second phase deadline: zero meaningful model turns submitted, owned cleanup verified. The retained all32 report is `specs/verifications/e03-complete-broker-acceptance.json`. No live failure was rerun simply to reconfirm it.

Earlier real stopped-native evidence also proves that Enter already written into a paused frontend can answer a question after a falsely safe API grant. A separate actual race showed backend Blue while native history advertised losing Red. These are not fixed by the three separately passing live operation checks.

Security impact: LOW for the current fail-closed implementation; no new cross-user exploit path identified. Correctness/acceptance severity is HIGH. Removing the unknown guard without a genuine native fence would permit stale application effects after a safe-labelled grant.

## Verified root cause

The integrated setup confirms trust in the owned workspace through genuine raw input before its first meaningful model turn. Actual runtime input dispatch records that input as an unknown native batch. Generic settlement deliberately cannot declare that batch applied. Safe offer correctly waits for all retained prior-owner work. The missing native acknowledgement makes even this first transfer unavailable; suppressing trust input or changing the fixture would not solve later native interaction.

The pinned Codex 0.160.0 source separates terminal-driver/Crossterm input, frontend terminal events, widget/paste continuations, AppEvents, remote client commands, backend callback processing and Core submission. The observer client joins downstream of the native queues. No inspected supported external operation binds a native input cut and broker generation across these boundaries.

Codex does contain internal input discard/quarantine and event pause/resume functions. They are not exposed as a generation-bound external fence and do not retire all already-created application events, delayed widget work or native RPCs. PTY delivery, kernel queue emptiness, redraw, transport ping/pong, silence, whole-turn completion and a later observer RPC cannot establish this application cut.

Question presentation is independently optimistic: the native overlay queues a locally chosen response and a completed-answer history cell before authoritative acceptance. Remote resolution dismisses actionable prompts but supplies no winning answer or completed-history reconciliation. Also, `serverRequest/resolved` is emitted before the answer is submitted to Core; it proves request closure, not a general Core application acknowledgement.

## Required fix contract

1. A supported native integration must bind the live frontend/input stream, controller generation and ordered input cut; every pre-cut native byte/event and causally produced delayed mutation must be applied/settled or explicitly rejected before a safe grant.
2. Stopped native input, partial sequences, paste/widget continuations, application events and delayed RPCs must remain unknown until the actual native boundary participates. Old outcomes may settle old work but never restore old authority.
3. An authoritative request disposition must identify the actual winning answer/decision or a recoverable winner identity. The ordinary native UI must render/reconcile that disposition rather than advertise its losing local answer.
4. Preserve current unknown/settlement-unavailable until these interfaces exist. No force-only substitution, delay, screen-derived identity, UI replacement, replay, process interruption or restart is a fix.

The approved plan excludes an upstream patch or alternate harness. After reviewing the diagnosed missing seam, the user explicitly chose to keep Codex unmodified and accept the temporary limitations for current delivery, deferring full correctness acceptance to future ACP integration. This authorizes a delivery-scope exception, not an invented API, weakened safe grant or claim that the problem is fixed.

## Verification after an authorized implementation

Retain and pass the real revoked-writer/continuation and multi-attachment regressions; then rerun only the exact approved complete-broker case with private auth, cheapest compatible non-sol model, original watchdog/deadline/turn limits and full owned cleanup. It must reach the stopped/buffered-native and losing-answer race gates, prove bidirectional safe grants and all32 assertions, and retain redacted native presentation separately from structured outcomes. Mechanical tests and request-closure events are not substitutes.

Independent Build/security review remains a separate gate after actual runtime proof and self-audit. Historical exhausted review counters and passed specification/Plan records must not be reset or presented as runtime acceptance.

## Accepted temporary disposition

User direction: keep upstream Codex, document the correctness limitation and move on; close the issue only after future ACP integration actually supplies the required behavior. At that e03 delivery no ACP code existed. The separate e04 amux-owned ACP path now has local runtime proof, with real-adapter acceptance still gated; this is not an input cut or authoritative presentation repair for the upstream native TUI. The failed test and all32 map remain unchanged; user acceptance is separate from runtime evidence. See `specs/security/EXCEPTIONS.md` and the final integrated local revisit `specs/verifications/e04s04-native-fence.json`.

## Primary pinned source evidence

- [Native reader/event stream](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/tui/src/tui/event_stream.rs)
- [Internal discard/quarantine boundary](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/tui/src/tui/input_boundary.rs)
- [Application event-loop scheduling](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/tui/src/app/startup.rs)
- [Optimistic native question history](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/tui/src/bottom_pane/request_user_input/mod.rs)
- [Native pending-request resolution](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/tui/src/app/app_server_requests.rs)
- [Remote response writes](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server-client/src/remote.rs)
- [Request closure before Core submission](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server/src/bespoke_event_handling.rs)
- [Resolved-notification fields](https://github.com/openai/codex/blob/rust-v0.160.0/codex-rs/app-server-protocol/src/protocol/v2/notification.rs)
