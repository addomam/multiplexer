---
story_id: e03s04
title: Externally requested conditional reacquisition
maturity: 3
type: feat
context: specs/planning-context.yaml
risk: P0
bcps: 8
status: passing
---

## 1 Identity
Fourth slice; depends on e03s03. Manifest acceptance: interaction-11, interaction-19, interaction-20, interaction-21, interaction-22, interaction-23. Plan only; no timer or runtime behavior is claimed implemented.

## 2 User story
As a human owning a PTY, I retain control unless I explicitly hand it back or an external controller's visible, consented conditional request still qualifies at atomic acceptance; any warning interaction can keep my same control without an amux retry.

## 3 Problem
An orchestrator's idle/background policy must not become a hidden amux relock timer. Clock interpolation, unknown attachment focus or stale consent could otherwise revoke human authority despite activity or an undrawn warning.

## 4 Goal
Expose public daemon-clock conditional observations, bind consent to what the original native owner actually displayed, and safely offer/accept external reacquisition without revoking human control during countdown, drain or offer.

## 5 Scope
Explicit external `human-idle` and `views-backgrounded` claims; public conditional clock; original-connection consent/activity/keep; native warning and pending-ack behavior; owner/all attachment scope; grant-time revision checks; explicit hand-back. No policy scheduling/retry is added.

## 6 Actors
External requester chooses conditions and whether to request again. Owning native attachment displays/consents or keeps. Other live attachments supply view facts but cannot consent for the owner or extend its inactivity. Daemon arbitrates observations and grant, not user intent or orchestrator policy.

## 7 Preconditions
s03 supplies actual native surface, per-connection controllers, shared input routing, captured targets and increasing view serials. s01 supplies retained dispatch accounting. Conditional claims require a known native source owner/generation; otherwise return condition-unavailable, not an inferred human owner.

## 8 Dependencies
e03s03; wire contract human activity/clock/view sections and schema; native UX conditional reacquisition; PLAN monotonic production clock and explicit-instant deterministic tests. Extend existing `tests/pty_control.rs`; no new clock abstraction or dependency. s08 remains final application-fence/real agent proof.

## 9 Module purpose callers and contracts
`src/control.rs` owns immutable condition parameters, current eligibility, consent, revisions and offer invalidation. `src/daemon.rs` captures public clock snapshots and original-connection provenance. `src/tui/control.rs` and `src/tui/view.rs` draw warning/background consent and route keep/activity; `src/tui.rs` drains received local input before consent/view transitions. External raw API clients consume their own claim, not the native owner's private capability. Preserve per-PTY atomicity and never wait for notifications to grant/cancel.

## 10 Requirements
- **ADDED — policy and public facts.** Only an explicit external request arms conditional intent, bound to current native owner/generation. Public reservation carries immutable `when`, eligibility eligible/ineligible/unknown, ready_for_consent, awaiting/recorded consent and daemon-captured `clock`. Native owner reads these through its own control, snapshot or events without requester ClaimHandle/OfferHandle. Ordinary reservations have no conditional object.
- **ADDED — clock semantics.** Production uses monotonic time. Before idle consent, idle remainder reaches zero to indicate readiness, not warning expiry. Idle warning uses requested warning_ms, starts only when accepted consent follows drawing, and reports actual daemon warning remainder. Zero warning remainder while awaiting settlement/acceptance remains waiting, not a grant. Background uses kind none and remaining_ms null, never invented countdown. Events publish eligibility/readiness/consent/clock-kind changes and expiry; reads refresh remainder, with no per-tick event requirement. Snapshot refresh after gaps restores the entire condition and current clock.
- **ADDED — guarded consent.** Original native connection drains pending local human events, draws that ready condition and sends `consent-to` with exact claim_id, displayed activity_revision and current view_serial. Reject stale-view/stale-observation/not-ready/condition-unavailable rather than start stale warning. Consent records intent but does not yield: human authority stays current through countdown, settlement and offer. Final acceptance atomically rechecks owner/generation, consent, activity revision, warning completion, view revision/eligibility and settlement before revoking/granting.
- **ADDED — activity and keep.** Human inactivity starts at native grant; actual owning native key/paste/application-mouse admission updates activity without an extra per-key call. Consumed local scroll/selection/search/keep reports activity on original native connection. Output/heartbeats/polling/resize/focus/automatic fallback do not. A one-shot script using borrowed control cannot claim human provenance. Before warning, human activity resets idle interval within the same request. Once visible warning starts, keep/activity terminally aborts that attempt, invalidates its offers and retains the same human token without rearm.
- **ADDED — pending acknowledgement.** Warning key is consumed and clears partial prefix/modal state. While consent acknowledgement is pending, explicitly send keep for any warning interaction before forwarding paste/application mouse or reporting local viewing activity. After recorded consent, native paste/mouse admission aborts atomically as activity and may proceed under the same token; local gestures remain local. No unreported physical-key guarantee is fabricated: daemon orders only received reports.
- **ADDED — background and withdrawal.** Request explicitly chooses owner or all live views. Selected terminal plus outer focus true is foreground; unknown focus/initial view blocks affirmative eligibility. Picker alone does not background selected PTY. Strict view serial and membership revisions invalidate incompatible offers when new/foreground/unknown views arrive. Ineligible view facts withdraw consent/offer and return same claim to watching; keep/activity aborts terminally. Departed views are not foreground but departure is not owner consent. Conditional withdraw retains human control; it is not ordinary cancel.
- **ADDED — explicit hand-back.** Yield to an ordinary reservation, otherwise release and let external requester issue a fresh now claim immediately. Release supersedes any conditional claim tied to that owner. No request means indefinite human control; neither release nor background chooses a former orchestrator automatically. Every successful grant is fresh authority, never revival.

## 11 Interfaces and data
Use existing `ownership-claim` cooperative when human-idle `{idle_ms>=0,warning_ms>0}` or views-backgrounded `{scope:owner|all}`; control `consent-to`, `keep`, `activity`, `release`; conditional claim `withdraw`; current offer `accept`. `observed` and public condition facts confer no input authority. Keep activity revision, native view serial and aggregate view revision distinct. Claim reads share the same public daemon clock, not a second private timer. Local animation never changes input gate or accepts offers.

## 12 Main flow
External controller requests an explicit idle threshold/warning against the human generation. Owner reads public readiness, draws configured warning and consents with exact displayed revisions. Human still types/owns throughout warning and unresolved drain. If no keep occurs, warning expires and settlement becomes ready; requester reads offer and explicitly accepts. Daemon atomically validates all current facts, revokes human capability and grants fresh external control.

## 13 Alternate and error flows
A warning key before consent reply explicitly keeps and clears modal/prefix state; no stale consent can resurrect the attempt. Activity wins accept race: abort with identical human token; grant wins: later old-token input is gated, never replayed. Stale views cannot overwrite newer focus. New unknown/foreground view invalidates all-scope offer; later eligible facts require fresh consent/offer reference. Owner-only scope does not invent all-view scope. Closed owner cannot consent. Withdrawing before grant retains human control, unlike ordinary cancellation. Stale offers and source-owner changes never retarget new ownership.

## 14 Nonfunctional constraints
Explicit instants drive deterministic state tests; production monotonic clock stays internal without a clock service abstraction. No automatic requester, retry/rearm, persisted policy or per-key capability allocation. No global/ownership lock across renderer/observer/harness I/O. Actual native surface proof and private-root owned-process cleanup remain required, independently of transition fixtures.

## 15 Implementation steps
1. Extend `src/control.rs`/`src/daemon.rs` with conditional state, public daemon-clock snapshots/events and original-connection role/revision checks; add explicit-instant transition unit tests inside `src/control.rs` and real-daemon clock/eligibility cases in `tests/pty_control.rs`, without a public test-clock API -> verify: `cargo test --lib && cargo test --test pty_control`
2. Add `src/tui/control.rs`/`src/tui/view.rs` warning/background presentation, draw-before-consent and pending-ack keep, sharing actual activity classification in `src/tui.rs`; prove with controlling-PTY attachments -> verify: `cargo test --test pty_control`
3. Implement atomic conditional acceptance, retained human authority and scope/view/settlement race revalidation, exercising both race winners and invalidated offers -> verify: `cargo test --lib && cargo test --test pty_control`
4. Finish withdraw/hand-back/no-policy behavior, multi-attachment background surface cases and existing API/native docs updates describing public clock and explicit request semantics -> verify: `cargo test --lib && cargo test --test pty_control`

## 16 Verification script
Future deterministic cases supply explicit instants and barriers: idle ready at zero, requested nondefault warning, warning zero with unresolved settlement, activity before/after consent and grant, stale activity/view acknowledgements, withdraw and both owner/all scopes. Assert public snapshots/events contain condition/clock but no private requester references; post-gap snapshot restores exact presentation. Real-daemon cases use actual original native connections and reject one-shot activity/view/consent forgery without forwarding. Actual controlling-PTY surface cases draw a nondefault warning, pause consent reply, send a key/paste/mouse/local selection, and assert explicit keep wins while human token is unchanged and no consumed key/prefix later reaches the application. After accepted consent, verify paste/mouse abort as admitted activity and local viewing remains local. Output alone must not postpone idle eligibility. Exercise two attachments plus new unknown view, stale serial, foreground change, picker opening, departed observer and departed owner; do not infer consent from absence. Verify hand-back requires no idle delay and a long explicit-instant interval with no request/after abort cannot rearm. Record visible captures, target effects and cleanup of only owned PTY/daemon trees. Real Codex policy and final native application proof remain s08, not these shell/clock fixtures.

## 17. Acceptance criteria
1. **interaction-11:** With no eligible explicit external request, human control persists through elapsed time/output/background; only explicit release/hand-back or a fully revalidated conditional acceptance changes it.
2. **interaction-19:** Requested idle threshold leads to visible configured-duration warning only after drawn/accepted consent; public daemon clock distinguishes idle/warning/none, and output does not reset human activity.
3. **interaction-20:** Warning key or activity, including the pending-consent-ack window, aborts exactly this attempt, retains identical human control and never causes an amux retry; consumed prefix/key is not replayed.
4. **interaction-21:** Owner/all scope uses actual current attachment selection/focus/membership and owner consent; unknown facts block, picker alone does not background, and background without an eligible explicit request preserves ownership.
5. **interaction-22:** Accept races revalidate owner/generation/activity/view/consent/warning/settlement at one point; stale offers cannot win a newer generation and successful grant returns fresh authority. Zero clock alone never grants.
6. **interaction-23:** Explicit ordinary yield or release followed by fresh now claim can hand back immediately without idle delay; released owner's conditional claim is superseded and no absent policy chooses a recipient.

## 18 Out of scope
No orchestrator scheduling/retry/interpretation policy, default auto-relock, generic clock abstraction, claimed observation of unreported physical input, real model calls or early native-buffer spike. s08 final complete-broker validation still must prove safe application settlement and all 32 scenarios.

## 19 Risks
P0: warning begins before draw/consent; stale revision and pending-ack tests expose it. P0: revoke-and-revive loses human authority on keep; assert same capability/generation remains throughout. P0: background unknown/new view yields a false offer; exercise membership and serial races. P0: local clock expiry changes gate; hold settlement/acceptance beyond zero. P0: keep mistaken for ordinary cancellation; assert old human control remains valid, no hidden rearm.

## 20 Evidence and handoff
All ledger entries remain failing; no runtime/clock/surface command ran during Plan. Later evidence includes public clocks/revisions, exact race outcomes, actual warning captures, input effects and resource cleanup. Handoff to s08 includes native/API conditional mechanisms, preserved unresolved accounting and explicit fence/presentation obligations. No e03 acceptance until complete broker, all 32 scenarios and explicitly approved bounded real smoke pass.
