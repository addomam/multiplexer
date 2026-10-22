# Native PTY control interaction

Status: implemented in the isolated e03 Build; the native ownership/conditional suite passed 29 real controlling-PTY cases. Complete-broker Codex acceptance remains failing. Complements the [wire contract](../contracts/pty-control.md), [ownership decision](../adr/0001-cooperative-pty-ownership.md) and [capability decision](../adr/0002-capability-control-interface.md). [Planning context](../planning-context.yaml) owns acceptance criteria; [Build evidence](../verifications/e03-integration-build.md) records actual commands and remaining limits. Routine bindings remain as designed; CLI porcelain remains deferred.

The incomplete-broker native-buffer counterexample is deferred by user direction to final complete-broker validation if still unresolved. It does not block completing this UX, and it does not authorize silently declaring unknown settlement safe.

## Existing conventions to preserve

The [current wrapper verification](../verifications/UI-ratatui.md) establishes a three-row header, full-width native body, no footer/body border, full-screen picker overlay, local selection/scrollback, and detach without killing the child. Ownership uses existing header/tab chrome and its border/divider color: do not consume new body rows/columns, shift mouse coordinates, replace the native agent UI or manufacture agent input.

The [current input router](../../src/tui.rs#L846-L1057) uses a configurable prefix (default Ctrl-b), prefix twice for a literal prefix, and a picker whose Ctrl-N/O/P/X/R/D/Q actions already have meanings. New ownership suffixes use currently unassigned Ctrl-L and Ctrl-F. Bare Ctrl-L/F still reach an owning native application normally; ordinary picker search letters remain search text.

## Display and bindings

Each active PTY's header shows an icon plus a textual ownership state. An owned-here label is distinct from another controller's lock; pending, unowned, and unknown/resynchronizing are not represented as successful ownership. Another controller's state includes a clipped, terminal-control-sanitized label and disambiguating controller ID. Never expose private handle references. Use theme ownership/warning colors on existing chrome, but never color alone.

`Prefix` below means the configured prefix, not hardcoded Ctrl-b. Ownership actions target the captured **active PTY**, not whichever row becomes highlighted after the picker opens. The dialog names its target. A later active-window change does not retarget an outstanding action.

| Action | Native binding / behavior |
|---|---|
| Request control from an observing attachment | Prefix, Ctrl-L. Explicit request immediately creates one cooperative claim; no redundant locked-window confirmation. |
| Reopen a pending claim or open the owning control panel | Prefix, Ctrl-L. Repeated invocation inspects the same claim; it does not force or submit duplicates. |
| Explicit force | Prefix, Ctrl-F, or the Force action in progress. Open the risk confirmation, then commit only after deliberate confirmation. Available before or after the three-second progress threshold. |
| Attempt ordinary application input while observing | Open the locked-window confirmation. Enter requests control; Escape keeps viewing without creating a claim. Never replay the triggering or confirming input. |
| Hide progress and keep waiting | Escape / Keep waiting. The claim remains active and can be reopened. Hiding is not cancellation. |
| Cancel pending ordinary takeover | Cancel takeover action, then confirmation that this revokes pre-transfer authority and grants nobody control. Do not label this action merely Close. |
| Owner responds to another requester | Visible nonmodal notice identifies requester; Prefix, Ctrl-L opens its response panel. Explicit Yield commits `yield-to` for that exact ordinary claim. Keep working dismisses the panel without yielding or cancelling the request. |
| Explicit hand-back / release | Owning panel: yield to an existing ordinary reservation, otherwise explicitly release. Release does not select or automatically restore a former orchestrator. |

A newly arrived owner-request notice must not capture a user typing an ordinary `y`/Enter and accidentally yield. Its notice is nonmodal; responding requires opening the ownership panel deliberately. Buttons must make Keep working versus Yield unambiguous. No response leaves the requester waiting for the owner, not falsely informed that the lock was released.

Illustrative progress surface; layout is design intent, not a captured frame:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 760 280" font-family="monospace" font-size="14">
  <rect x="8" y="8" width="744" height="42" rx="6" fill="none" stroke="var(--warning)"/>
  <path d="M25 25v-5a5 5 0 0 1 10 0v5 M23 25h14v13H23z" fill="none" stroke="var(--warning)"/>
  <text x="48" y="34" fill="currentColor">project / agent</text>
  <text x="380" y="34" fill="var(--warning)">Locked · orchestrator #a17</text>
  <text x="24" y="78" fill="var(--muted)">Native terminal stays at its existing geometry.</text>
  <rect x="80" y="102" width="600" height="160" rx="5" fill="none" stroke="var(--border)"/>
  <text x="100" y="130" fill="currentColor">Control requested — agent PTY #t42</text>
  <text x="100" y="157" fill="var(--muted)">Waiting for 1 forwarded question answer</text>
  <text x="100" y="183" fill="var(--muted)">request #q9 · 3.2s · outcome not yet known</text>
  <text x="100" y="211" fill="var(--warning)">Force may let earlier calls land. The turn keeps running.</text>
  <text x="100" y="242" fill="currentColor">[Keep waiting]  [Cancel takeover]  [Force…]</text>
</svg>
```

## Request, wait, cancel and force

1. Capture PTY lifetime, current public observation and this attachment's controller credential. A dedicated request or confirmed input attempt submits a single `ownership-claim` with `when: now`. The current owner remains authorized until yield/cancel/force.
2. Cache the returned claim privately and observe it/events without blocking the native event loop. A second request action reuses it. A reservation held by someone else is shown as contention, not silently adopted; explicit forced creation can override it with a fresh compare receipt.
3. Show immediate waiting status. At three seconds, expand progress to the actual blocker: owner response, owner consent, unresolved call IDs/kinds/elapsed time/unknown outcomes, or reserved acceptance. Do not invent a completion ETA or a five/ten-second grant deadline. Progress remains visible while stalled, with wait/cancel/force available.
4. A ready offer is still not ownership. The attachment asynchronously accepts its own current offer; only a committed current `ControlHandle` enables input. Claim reads do not accept. If the view changes while waiting, the claim remains tied to its original PTY; no input is replayed on grant.
5. Cancellation uses the exact pending ordinary claim. If cancellation wins, show unowned and permit fresh acquisition; do not show Controlled here or claim nothing happened to old forwarded work. If grant won first, refresh authoritative state and show actual ownership; do not reinterpret the stale cancellation as release.
6. Force confirmation names the observed owner/reservation and discloses retained unresolved calls and the fact that the turn/process continues. Commit with that observation. On `stale-observation`, show current facts and require a new explicit confirmation; never force a replacement owner automatically.

A local RPC timeout, hidden panel, detach or socket loss is not a successful cancellation, failed acquisition or release. Recover this controller's private state before choosing another mutation. Loss of current-state certainty shows a gated/resynchronizing UI rather than forwarding under a cached claim or optimistic success label.

## Input routing

Local wrapper actions are classified before application input: picker navigation/search, local selection/copy/scrollback and ownership panels remain usable while observing. The existing literal-prefix route is application input and must pass the same authority gate as ordinary keys.

All actual application-input paths share current control validation: keys, repeats, literal prefix, paste and application mouse events. A locked attempt opens the control prompt rather than forwarding bytes. Consume a prompt-opening key/paste/click and its confirmation; never queue it for later replay. For mouse interaction, consume the initiating gesture's follow-up events so an old mouse-up/drag does not reach the application after grant. A later fresh deliberate gesture/input can be forwarded under current authority.

Necessary automatic terminal replies and internal lifecycle/settlement reports continue through daemon-classified protocol paths. They are not forged by labelling a user input request as protocol traffic. Focus observations are attachment facts, not user typing; one view losing focus is not proof that all views are backgrounded.

Revoked authority is an expected recoverable UI state, not a fatal wrapper error. Stop forwarding, refresh facts, show the appropriate locked/unowned/pending view and leave the PTY process running. Do not retry a possibly applied input or mutation. Typed server error codes, not flattened message parsing, drive this transition.

## Conditional orchestrator reacquisition

Only an explicit external request can arm idle/background acquisition. The suggested idle/warning values remain 60 seconds and 5 seconds, configurable by that request. Agent output, report heartbeats and view polling do not count as human activity.

The owner reads the public reservation's `conditional` observation through its current control read, ownership snapshot or event. It carries the actual condition parameters, eligibility, explicit consent readiness and current clock; no requester ClaimHandle/OfferHandle is shared. Use the requested `warning_ms`, never a hardcoded five seconds. A fresh complete snapshot restores this presentation after an event gap.

- For idle acquisition, wait for `ready_for_consent`, draw the keep-control warning using the advertised duration, then send conditional consent with the displayed activity revision and current native view serial. The pre-consent idle clock reaching zero means warning readiness, not warning expiry; animate the warning countdown only from the accepted consent's authoritative warning clock. If those facts changed, refresh rather than acknowledging a stale display. Countdown/drain/offer do not revoke human control. Missing consent from an unresponsive attachment is a blocker, not automatic permission.
- During the warning, a key explicitly keeps control and is consumed by the warning; clear any partial prefix/modal state so it cannot become a later application command. While consent acknowledgement is pending, explicitly send `keep` for any warning interaction before forwarding paste/application mouse or reporting local viewing activity, so this visible attempt aborts rather than silently returning to idle watching. After consent is recorded, native paste/application mouse admission aborts the attempt atomically as activity and may proceed under the still-current human authority; local viewing gestures report activity and remain local.
- Keep/activity aborts this attempt without rotating/reviving the human token or rearming a timer. The orchestrator may make a new request, but amux does not do so itself.
- A background request declares owner-only or all-live-view scope. Native client view reports have increasing serials, and selection/focus/membership revisions are rechecked at acceptance. Unknown focus blocks affirmative background eligibility; opening the picker does not background the selected PTY by itself.
- Explicit release can hand back immediately: yield to an ordinary reservation, otherwise release and let the orchestrator make a fresh immediate claim. No inactivity delay is imposed merely because a human previously owned it. A conditional claim bound to the released owner is superseded as specified by the wire contract.

If activity and conditional acceptance race, the daemon ordering point decides: activity first keeps the same human token; acceptance first gates later old-token input and shows the changed owner. UI countdown expiry alone never flips the input gate.

Render background consent from its advertised owner/all-view condition, not from an idle-warning guess; its clock is explicitly absent (`kind: none`, `remaining_ms: null`). A zero warning clock while waiting for settlement/acceptance becomes waiting status, not a locally completed takeover. Local countdown interpolation is presentation only; ownership and input authority change only on the daemon's committed grant.

## Attachments, identity and lifecycle

Each amux attachment registers its own native controller on its persistent connection. Multiple amux clients observe **one existing daemon-owned PTY/native agent process**; attaching a viewer must not launch a second agent/native client or fork the conversation. A controller may hold per-PTY capabilities for multiple windows; selection changes only the active view, unless an explicit eligible background request transfers one of those PTYs.

On selecting/attaching an unowned PTY, ordinary fresh acquisition may grant if eligible. Competing first attachments are arbitrated atomically: first grant owns, others observe. Selecting a PTY already owned by someone else never steals it. Unknown/outstanding work still needs settlement or explicit force.

View selection/focus/membership reports carry the controller's monotonic serial and go only through its original native connection. An observer's typing/selection does not reset the actual owner's inactivity interval. One closed view is marked departed rather than fabricating a global FocusLost or owner consent.

Detach preserves the native PTY, agent turn and process. Connection loss itself does not cancel pending claims or release server authority; explicit release/cancel/withdraw remains distinct and can be chosen before detaching. A stranded owner/reservation is recoverable through explicit force with current public comparison, never another client's possession of private credentials. There is no silent reconnect/replay/recovery feature in this cut.

PTY exit/replacement invalidates its authority and pending transfers; retain existing exit-card and explicit close/remove behavior. A native UI's exit is not proof that its agent backend's turn succeeded. Lifecycle degradation is visible and managed backend resources remain the daemon's responsibility; no hidden replacement instance or automatic task acceptance.

## Implementation acceptance, not evidence already passed

Existing interaction criteria 07–13, 16–32 remain authoritative. In particular, exercise these distinct consumer-visible paths when implementing:

- Two attachments: observing keys/paste/literal-prefix/application mouse cannot inject input; local viewing and owner input still work.
- Owner notice arrives during typing without accidentally consuming a yield action; explicit yield preserves requester reservation.
- Request → hide progress → return → grant; the first/confirmation input is never replayed and hiding never cancels.
- Cancel versus already-committed grant; force versus a changed owner/reservation; stale actions cannot release or revoke the winner.
- Idle warning → key keep → no automatic retry; idle/background predicates rechecked at grant; unknown/multiple views never become invented eligibility.
- Detach/connection loss retains authoritative state; explicit force recovers a stranded reservation without its disappeared requester's private handle.
- Complete-broker queued-native-input accounting is checked at final validation as requested; the negative control is not a prerequisite to continue elaboration or a waiver of safe grant.

Implementation proof now includes actual amux controlling-PTY rendering, input-route interception without replay, request/yield/cancel/force, conditional consent/activity/keep/withdraw and attachment-scope races. This native slice does not prove same-instance structured Codex interaction or the final native/application settlement fence.
