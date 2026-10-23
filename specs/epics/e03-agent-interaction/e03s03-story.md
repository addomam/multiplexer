---
story_id: e03s03
title: Native ownership UX across attached clients
maturity: 3
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 5
status: passing
---

## 1 Identity
Third slice; depends on e03s02. Manifest acceptance: interaction-16, interaction-17, interaction-24. Owns actual native surface proof, not merely reducer tests. Plan only.

## 2 User story
As an attached human, I can see who owns the selected terminal, deliberately request or force control, keep viewing while waiting and avoid accidentally injecting an intercepted key or yielding while typing.

## 3 Problem
Cached authority alone does not explain a lock, protect every application input path or make multiple attachments usable. A popup that captures ordinary typing could accidentally yield; a replay queue could submit a stale command after grant.

## 4 Goal
Ship observable native ownership controls through the existing amux wrapper, with one authority gate for keys/paste/mouse, deliberate transitions, recoverable errors and no new native body geometry.

## 5 Scope
Three-row header ownership chrome; configurable-prefix Ctrl-L/Ctrl-F; locked-input confirmation; nonmodal owner notice and deliberate response panel; wait/cancel/force; per-original-connection view reporting and multi-attachment arbitration; all application routes and local-view classification. s04 adds conditional warning/keep presentation.

## 6 Actors
Owning attachment, observing attachment, external owner/requester, native application and daemon. Each attachment observes one existing PTY/frontend; registering another view must never launch another agent or fork its conversation.

## 7 Preconditions
s02 supplies current control, claims/offers, explicit cancel/force, typed errors and recovery. Existing configured prefix, picker keys, selection/scrollback, mouse translation and detach behavior remain authoritative compatibility constraints. Initial attachment view is unknown until reported.

## 8 Dependencies
e03s02; native UX contract and wire provenance/view contract; `tests/pty_control.rs` created by s01. No new dependencies or frontend replacement. s08 retains real Codex authoritative presentation and buffered native application proof.

## 9 Module purpose callers and contracts
`src/tui/control.rs` centralizes ownership input decisions and presentation state. Reason for Depth: keys, paste, literal prefix, application mouse and local activity must not invent different authority rules. `src/tui.rs` routes actual terminal events and RPC completion, `src/tui/view.rs` renders existing chrome/dialogs, `src/client.rs` preserves typed errors, and daemon view admission binds controller to its original persistent connection. Keep hot input borrowed/cached; no per-key registration. Existing picker/local actions remain independent of input ownership.

## 10 Requirements
- **MODIFIED — ownership presentation. Before:** the native wrapper lacks the selected ownership header/control flows. **After:** active PTY header distinguishes owned-here, another owner's lock, unowned, pending and unknown/resynchronizing; another owner has lock icon plus distinct existing-chrome border/divider color and textual sanitized/clipped label with disambiguating ID. No private handle renders and no body rows/columns change.
- **ADDED — deliberate controls.** Configured Prefix then Ctrl-L requests directly or reopens the same pending claim/owner panel. Prefix then Ctrl-F opens explicit force confirmation immediately, without waiting three seconds. Bare Ctrl-L/F still reach an owner application. Ownership actions capture original active PTY, not picker highlight or later selection. Owner-request notice is nonmodal; only deliberately opening response panel and choosing Yield relinquishes the exact ordinary claim.
- **MODIFIED — input routes. Before:** wrapper application input paths do not implement the selected ownership prompt/no-replay UX. **After:** key/repeat, literal prefix, paste and application mouse pass the same authority gate. Observing input opens confirmation and is consumed; confirmation Enter is consumed too. Consume initiating mouse gesture follow-ups through release/drag so no old mouse-up lands after grant. No replay buffer. Local picker/search/copy/selection/scrollback remain usable while observing.
- **ADDED — waiting and uncertainty.** Show immediate waiting; after three seconds show awaited owner response and exact call ID/kind/elapsed/missing outcome blockers, with no automatic grant deadline. Escape hides progress but retains claim; reopening does not duplicate it. Async accept enables input only after current control commits. Cancel confirmation says revoke old authority/grant nobody; force names current owner/reservation and retained-call risk while work continues. Stale force requires fresh facts and renewed explicit confirmation.
- **ADDED — attachment lifetime.** Register each native controller on its persistent original connection, report strictly increasing nested view serial/selected-terminal/focus including null/unknown, cache per-PTY control and refresh after typed stale errors without replay. Disconnect marks departure only; it does not release/cancel. A fresh attachment gets a fresh identity. No implicit reconnect/recovery engine.

## 11 Interfaces and data
Consume private current controller capabilities and public ownership snapshots/events separately. Preserve claim target lifetime, compare receipt and async RPC state across panel hiding/window changes. Never obtain another requester's private handle from public events. Report actual native view on original connection; one-shot credentials cannot forge activity/focus provenance. Event gap refreshes snapshot plus own private state, showing gated unknown until current authority is established.

## 12 Main flow
Two actual attachments display one PTY. Observer's paste opens locked prompt with no target bytes; Escape leaves viewing. Dedicated request creates one claim and owner receives nonmodal notice during typing. Owner explicitly opens response panel and yields. Requester may hide/reopen progress; on settled offer it asynchronously accepts and header changes only on committed control. A later fresh key is forwarded, never the intercepted paste/Enter.

## 13 Alternate and error flows
Active-window change does not retarget an open action. Cancel/accept race refreshes actual winner rather than calling stale cancel release. Changed owner invalidates force confirmation; user must reconfirm. Stale input error stops forwarding and refreshes without killing wrapper or process. Another reservation displays contention, not adoption. Competing first attachments acquire atomically if settled; selecting somebody else's PTY never steals. Detach preserves authority/process; explicit recovery force uses public compare facts rather than another attachment's credentials. Exit/replacement invalidates target actions and preserves existing exit card.

## 14 Nonfunctional constraints
No blocking daemon cooperation or synchronous waits in native event loop. Ownership dialogs must preserve target coordinates, configured prefix, picker shortcuts and three-row header/no-footer geometry. Labels are terminal-control-sanitized and clipped. Tests must distinguish native control flow evidence from shell transport or synthetic reducer evidence; actual rendered captures are required.

## 15 Implementation steps
1. Add `src/tui/control.rs` and wire header/view reporting through `src/tui.rs`, `src/tui/view.rs` and typed client handling, with actual two-attachment rendered ownership/provenance tests -> verify: `cargo test --test pty_control`
2. Route keys/repeats/literal prefix/paste/application mouse through shared authority decisions, preserving local viewing and consuming complete intercepted gestures without replay -> verify: `cargo test --test pty_control`
3. Implement deliberate request/owner-response/progress/wait/cancel/force panels with captured target and responsive async acceptance/recovery -> verify: `cargo test --test pty_control`
4. Complete multi-attachment selection/disconnect/exit/error handling, update existing native shortcut/help documentation and prove compatibility geometry/process lifetime -> verify: `cargo test --test pty_control`

## 16 Verification script
Each named future integration case launches actual amux TUI attachments on controlling PTYs against a private real daemon and observable native target. Drive physical terminal event encodings, inspect rendered lock/icon/text/chrome and target effects, and retain redacted visible captures separately from assertions. Capture owned/locked/pending/unknown states at fixed geometry. Exercise default and alternate configured prefix; literal prefix, repeat, bracketed paste and application mouse press/drag/release cannot leak from an observer or replay after grant. Local scroll/search/copy work without acquiring; protocol replies still progress. Type ordinary y/Enter as notice arrives: owner must not yield. Open response panel deliberately, hide/reopen requester progress and assert same claim. Hold owner or settlement past three seconds, verify exact blockers and usable wait/cancel/force. Change selection/owner during confirmation, test stale responses and require deliberate re-confirmation. Detach one real attachment and assert the same daemon-owned process continues with server ownership retained. Use per-case deadlines, restore terminal modes and reap only owned trees/roots. Unit event injection may supplement but cannot replace these surface cases; no Codex/model calls here.

## 17. Acceptance criteria
1. **interaction-16:** Actual observer attachment renders lock icon plus distinct existing chrome border/divider color and text without body geometry shift; dedicated shortcut requests once, intercepted application input opens confirmation, and neither opening/confirming input nor mouse follow-up replays after grant.
2. **interaction-17:** Actual pending panel exposes owner-response and request-level ID/kind/elapsed/unknown blockers after three seconds; hide/reopen retains claim, timers never grant, and wait/cancel/force remain responsive before and after threshold.
3. **interaction-24:** Two real attachments share one existing native PTY/process; second must request via same claim/yield/accept protocol, owner gets nonmodal identifying notice, and observer key/paste/literal-prefix/application mouse effects are absent while local viewing remains usable.

## 18 Out of scope
No native UI replacement, footer/body border, hidden input retry, shadow agent, conditional consent timer implementation or generic event framework. Complete broker native buffer and real harness losing-answer presentation remain s08 work, not a new early spike or waived guarantee.

## 19 Risks
P0: a separate paste/literal-prefix/mouse-up path bypasses gate; assert actual target effects across full gestures. P0: nonmodal owner notice accidentally consumes yield; type through arrival. P0: async panel retargets another PTY or forces changed owner; capture target and require fresh confirmation. P0: injected reducer tests mistaken for UI proof; require controlling-PTY attachments and visible captures.

## 20 Evidence and handoff
Failing ledger only; no surface/test/command has run in Plan. Later evidence must name controlling PTYs, target process identity, captured states, absent/present target effects and cleanup. Hand off actual view/activity routing to s04; s08 still gates first acceptance with real same-instance presentation and application accounting. All 32 scenarios remain required.
