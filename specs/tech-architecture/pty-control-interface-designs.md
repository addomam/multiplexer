# PTY interaction ownership: Interface alternatives

Status: alternative C, typed control/claim/offer capabilities, is selected in [ADR 0002](../adr/0002-capability-control-interface.md). Three alternatives were compared; A and B remain comparison records, not competing active Interfaces. The [normative wire contract](../contracts/pty-control.md) now supersedes this document's illustrative messages. None of the capability operations is implemented. [Planning context](../planning-context.yaml) remains authoritative for required behavior.

All JSON, type signatures, IDs and outcomes below are illustrative proposed behavior, not captured runtime output. No new operation was executed. This document does not advance the Elaborate quality gate or supply a review PASS.

## Common requirements and existing Seam

The PTY interaction ownership Module owns authority, handoff reservations and outstanding-call accounting. The concrete harness Adapter supplies supported structured interactions and authoritative settlement; an Interface cannot manufacture that capability. Native routing remains responsible for distinguishing viewing, agent-directed input and internal terminal protocol progress.

The current transport is [newline JSON API v1](../../src/client.rs#L34-L70). A normal response uses `version`, `ok` and `result`; an error uses `error.code` and `error.message`. The client currently flattens errors to text, so pending/offered/countdown states must be successful results, not error-carried metadata. [Existing events](../../src/daemon.rs#L545-L568) provide sequence cursors and gap/snapshot recovery. Hyphenated operation names and omitted absent optional fields follow [CLI conventions](../../src/cli.rs#L239-L257).

Shared invariants for all alternatives:

- A controller is a daemon-memory identity reused across one-shot calls, not a socket slot, user label or durable registration. Every native attachment has a distinct controller identity. Display names do not recover ownership.
- Current PTY authority binds its controller, live PTY and ownership generation. Agent actions additionally validate harness/session/request identity. An event sequence, input epoch or screen generation is not authority.
- A cooperative request does not revoke. Yield, explicit force or cancellation does. Safe transfer requires settlement and reserved-requester acceptance; only the daemon commits ownership.
- Cancelling before grant leaves no new owner and no reservation. The previous controller may acquire fresh authority immediately; its already-forwarded calls remain tracked as its continuing work. A different controller must not interpret an unowned-but-unsettled PTY as safe: it needs settlement or explicit force. No old token is revived and no empty interaction history is fabricated.
- Pending operations return promptly; progress, owner popup, force and cancel remain usable. There is no mandatory takeover deadline or authority expiry. Transport timeouts do not cancel server-side work or imply rejection.
- Public ownership events/snapshots carry identity and progress, not usable tokens. The current controller can inspect its own authority after an uncertain response rather than replay a mutation.
- A reacquisition countdown is distinct from a cooperative takeover: human activity aborts the former while keeping human ownership. Cancelling the latter invalidates pre-transfer authority. amux does not rearm either request autonomously.
- Native input reuses cached authority and borrowed bytes. No per-key registration, state lookup, new handshake or heap-heavy generic command graph is required. Local viewing and internal terminal replies continue without becoming ownership bypasses.

## A. Closed transition algebra

### Proposed Interface

Conceptual signatures, not compiled code:

```text
observe(target, controller) -> OwnershipState
apply(target, controller, Transition) -> TransitionOutcome
events(after) -> EventBatch

Transition = Request(expected_generation, when)
           | Yield(authority, transfer)
           | Accept(transfer)
           | CancelTransfer(transfer)
           | Force(transfer)
           | AbortReacquisition(authority, attempt)
```

`ownership-apply` carries a closed tagged transition. `Request(now)` grants when acquisition is eligible and otherwise requests cooperation; it never revokes an existing owner merely because it was called. Mutation payloads remain separate native/agent operations with current authority, not extra transition variants.

### Caller usage

An orchestrator uses `observe` to resolve the current PTY/generation, then requests authority:

```json
{"version":1,"op":"ownership-apply","terminal":"pty-a","controller":"orch-a","transition":{"kind":"request","expected_generation":7,"when":{"kind":"now"}}}
```

For a native handoff, the requester submits `request`, the owner sends `yield`, and the requester sends `accept` on the offered transfer. The current transfer ID binds all of them to one negotiation:

```json
{"version":1,"op":"ownership-apply","terminal":"pty-a","controller":"native-b","transition":{"kind":"accept","transfer":"handoff-1"}}
```

Requester cancellation is `cancel-transfer`; fresh acquisition is a new `request`. Explicit `force` is a distinct variant and returns unresolved-call warnings. An inactivity request uses `when: human-idle`; human countdown interruption uses the distinct `abort-reacquisition` variant, never `cancel-transfer`.

### What the Implementation hides

One transition arbiter handles role/phase checks, invalidation, dispatch accounting, reservation and authoritative settlement. The caller does not perform full-snapshot compare-and-swap on every input; narrow ownership/transfer identity is sufficient. The enum is intentionally limited to ownership rather than a generic RPC or event-sourcing language.

### Trade-offs

The outer Interface is compact and exhaustive typed matching is natural. Raw JSON/CLI users still learn the same semantic transitions, however; a small operation count is not evidence of low conceptual complexity. Shared outcomes can also become a broad union that each caller must narrow correctly. This shape is attractive for a closed reducer inside the Module, less discoverable as the everyday wire Interface.

## B. Explicit domain verbs — initial recommendation, not selected

### Proposed Interface

Use named operations with shared receipts rather than a generic transition envelope or handle-method dispatch. Representative signatures:

```text
controller-register(kind, label) -> ControllerId
control-state(terminal, controller?) -> ControlState
control-acquire(controller, terminal, expected_generation) -> Granted
control-request(controller, terminal, expected_generation) -> Requested
control-yield(authority, transfer) -> Draining | Offered
control-accept(controller, transfer) -> Pending | Granted
control-cancel(controller, transfer) -> Cancelled
control-force(controller, observed_control) -> GrantedWithWarning
control-reacquire(controller, terminal, expected_generation, when) -> Attempt
control-keep(authority, attempt) -> HumanControlRetained
attachment-observe(controller, view) -> ObservationRecorded
```

These are proposal names, not new CLI subcommands already available. `control-keep` is the countdown-abort operation, not cancellation of a takeover. Shared conceptual receipts are `ControllerId`, opaque `AuthorityToken`, `TransferId`, `ReacquisitionId` and `ControlState`. A transfer/attempt ID binds its target, roles and starting ownership internally; callers need not construct a second authority object with independently editable revision/generation/lifetime fields.

A `ControlState` exposes target, generation, owner, pending transfer/attempt, phase, request-level blockers, attachment-scoped facts and `seq`. It includes an opaque `ControlRef` comparing the live PTY, ownership generation and current reservation identity; it is not mutation authority and does not change merely because unrelated events/output occur. Current authority is returned only to its controller by acquisition or owner inspection. An existing input request adds its cached authority token; server admission and dispatch registration validate the actual binding. The token alone cannot authorize a stale agent target.

### Caller usage

The orchestrator registers once, retains its controller identity across one-shot calls, and acquires current authority. A native client also registers once per attachment, not per key or transport frame.

```json
{"version":1,"op":"control-acquire","terminal":"pty-a","controller":"orch-a","expected_generation":7}
{"version":1,"op":"input","terminal":"pty-a","authority":"authority-orch-8","bytes":[97]}
```

A second native attachment requests control. The owning native client gets a popup, or the orchestrator receives the event. The current token remains valid until it yields:

```json
{"version":1,"op":"control-request","terminal":"pty-a","controller":"native-b","expected_generation":8}
{"version":1,"op":"control-yield","authority":"authority-orch-8","transfer":"handoff-1"}
{"version":1,"op":"control-accept","controller":"native-b","transfer":"handoff-1"}
```

Yield returns promptly even if settlement is still pending. Accept returns `pending` if that same transfer is not ready; it does not block or register a promise to acquire later. Once ready, acceptance commits fresh authority atomically. The pending reservation prevents an opportunistic acquisition between yield and acceptance.

The cancellation path is deliberately explicit:

```json
{"version":1,"op":"control-cancel","controller":"native-b","transfer":"handoff-1"}
{"version":1,"op":"control-acquire","terminal":"pty-a","controller":"orch-a","expected_generation":9}
```

The second command is a new acquisition by the prior controller, not restoration. Generation numbers are illustrative; callers use the actual returned/observed generation, not arithmetic. If another owner or reservation has intervened, acquisition rejects the stale intent rather than retrying automatically.

Force uses `control-force` with the requesting controller and its observed `ControlRef`, returning unresolved calls with an explicit warning. It can override an unresponsive owner or stranded reservation without possession of that other requester's private receipt. The daemon rejects a stale reference rather than overriding an unexpectedly newer owner/reservation. Force never sends Ctrl-C, stops a process or disguises its result as a safe drain; late messages for `handoff-1` cannot alter another transfer or a newer owner.

An orchestrator may separately request conditional reacquisition:

```json
{"version":1,"op":"control-reacquire","terminal":"pty-a","controller":"orch-a","expected_generation":10,"when":{"kind":"human-idle","idle_ms":60000,"warning_ms":5000}}
{"version":1,"op":"control-keep","authority":"authority-native-10","attempt":"reacquisition-1"}
```

Owning native input can abort the countdown atomically as part of normal input admission; it does not require an extra keep request per key. A key consumed solely to dismiss the warning uses `control-keep` once and is not injected into the application. The daemon emits the abort and does not rearm it. Owner conditional yield and final acceptance must recheck activity and ownership; expiry alone is never a universal permission to skip handoff/settlement.

### What the Implementation hides

The same single Module owns controller/authority binding, exact transfer identity, local dispatch ordering, pending-call records and atomic ownership commits. Shared receipts keep each verb narrow; they do not create a public generic registry or duplicate authority model. The external Interface follows current `op` conventions while a closed transition reducer may remain private Implementation detail.

Normal outcomes such as requested, draining, offered, countdown and pending are successful `result` values. Actual errors retain code/message: examples include `authority-required`, `authority-revoked`, `control-reserved`, `stale-ownership`, `stale-transfer` and `settlement-required`. Typed local handling must preserve the existing wire error code rather than parse arbitrary messages or require new metadata.

### Trade-offs

Named verbs have the best discoverability for shell help, operational debugging and external controllers. Their distinct names make dangerous semantic differences visible: cancel takeover versus keep human control, and force versus cancel turn. There is more public vocabulary, but not more underlying ownership state than the other designs. Shared opaque receipts prevent that vocabulary from turning into redundant caller-side identity assembly.

Fixed authority bytes add to each input frame, but registration and state resolution remain off the hot input path. Borrowed typed serialization can preserve the existing approach; no allocation-free transport claim is made. New ownership mechanisms require deliberate operation/condition additions rather than an open-ended command language.

## C. Control, claim and offer capabilities

### Selected model; wire spelling remains proposed

The primary caller abstraction is a serializable memory-backed capability whose kind restricts its operations:

```text
claim(controller, target, when) -> ControlHandle | ClaimHandle
ControlHandle: input, supported agent actions, yield_to(proposal)
ClaimHandle: read, cancel, force
OfferHandle: accept
```

An offered handoff returns an `OfferHandle` only when the safe proposal is ready. A pending claim is never input authority. Ordinary snapshots/events contain identity/progress rather than capability references.

### Caller usage

The orchestrator keeps a control capability for normal mutations. Another attachment creates a claim; the old owner yields to it; claim inspection returns a ready offer, whose acceptance returns a new control capability:

```json
{"version":1,"op":"ownership-claim","terminal":"pty-a","controller":"native-b","when":{"kind":"now"}}
{"version":1,"op":"control-handle","controller":"orch-a","handle":"control-orch-8","method":"yield-to","args":{"proposal":"handoff-1"}}
{"version":1,"op":"control-handle","controller":"native-b","handle":"offer-native-1","method":"accept"}
```

A claim's `cancel` invalidates pre-transfer authority and grants none; the prior orchestrator creates a fresh claim. A claim's `force` grants control with unresolved-call warnings. Human countdown interruption is an owner-only keep/abort method, not claim cancellation. One-shot CLI calls serialize references without making the socket their lifetime.

### What the Implementation hides

Server-side capability kinds constrain owner/requester actions and ready offers; copied JSON cannot override the daemon's actual role/phase records. Native typed wrappers can make an offer impossible to use as input authority. The daemon still enforces identity because JSON values are copyable, not truly linear resources with automatic destructor release.

### Trade-offs

This can make a typed SDK harder to misuse when handle kinds genuinely encode readiness and role. Raw CLI users instead manage controller, control, claim and offer references, and the Implementation carries an additional object-kind lifecycle. If those handles merely wrap verb names, this adds ceremony without Depth. Capability creation is transition-time work, not per-key work; no automatic expiry, persistent socket or restoration is implied.

### Selected client and plumbing contract

The user explicitly prioritized this capability model and raw Interface plumbing over immediate CLI ergonomics. The objective benefit is enforcing authority, pending intent and ready transfer as different server-validated roles; the cost is handle lifecycle bookkeeping. That cost earns Depth only if the daemon rejects wrong-kind and stale capabilities rather than merely wrapping the same untyped methods.

- **Asynchronous acquisition:** submit the claim and return its serializable handle promptly. Subsequent inspection reports pending, ready-offer, granted or terminal state. A claim receipt, an unowned PTY or a ready offer is not a `ControlHandle`.
- **Read versus acquire:** inspection stays non-mutating. A client operation that checks and acquires may explicitly accept a ready offer; it reports acquisition only after receiving or recovering current control authority.
- **Synchronous waiting:** a client-side wrapper waits for progress, reads its claim and accepts the offered handoff. Primitive daemon calls remain bounded/nonblocking so the native client's pending controls stay usable. This waiting is not automatic mutation retry.
- **Timeout and interruption:** killing or timing out that wrapper does not cancel its server-side claim or release authority it may already have acquired. A lost accept response is an unknown client outcome; inspect private claim/controller state before deciding to cancel or force.
- **Force:** a claim can explicitly force immediate control, with unresolved-call warnings and no turn/process cancellation. Forced-claim creation can also override a stranded reservation using a current compare-and-act ownership/reservation observation, without requiring its missing requester's private capability. Stale intent is rejected rather than retargeted to a newer owner.
- **Raw entry point:** `amux api` already accepts request JSON ([CLI declaration](../../src/cli.rs#L30-L33), [request construction](../../src/cli.rs#L158-L163)). Its exit 0 denotes a successful request, which may return a pending claim. A future acquisition waiter may reserve exit 0 for actual granted control. Exact lock/check/wait names and `-f` spelling are deferred.
- **No new framework:** handle kinds and permitted methods are a closed ownership-specific set. No dynamic object dispatch, capability delegation, generic registry or extra persistent worker is implied. Native input borrows its cached control capability; handle issuance stays off the per-key path.

CLI porcelain deferral does not waive same-instance structured questions/permissions, native ownership UI, safe/forced semantics or the other first-acceptance criteria.

## Comparison and synthesis

**UX:** A minimizes outer operation names but exposes a command algebra. B uses the current discoverable operation convention and makes force/cancel/keep intent explicit. C can guide a typed client through valid operations, but its reference lifecycle is less natural for raw one-shot commands.

**Complexity and extensibility:** All alternatives need the same real state machine. B avoids a public handle lifecycle; C makes claim/control/offer role separation explicit and accepts that lifecycle cost. The user chose the latter because typed plumbing is the priority and CLI ergonomics can be layered later. A closed transition reducer can remain private Implementation detail without becoming a second public Interface.

**Efficiency:** None requires registration, full-state CAS or a handoff round trip per native key. Cached authority and borrowed bytes are the intended fast path; settlement tracking is required only where the action needs it. No ownership state lock is held while waiting for the owner, harness or observer.

**Depth and correct use:** The leverage comes from one Module enforcing the invariant, not from counting method names. Exact transfer identity, separate keep/cancel semantics, unknown-outcome accounting and atomic dispatch/grant checks are mandatory in every design. No variant makes the OMP reporter a structured-control Adapter or solves the native optimistic-answer race by declaration.

**Selection:** C, control/claim/offer capabilities. The earlier B recommendation weighted immediate shell discoverability; the user's explicit plumbing-first scope changes that trade-off. Keep C's role/phase enforcement and serializable one-shot usage, without a generic capability framework or per-input object churn. [ADR 0002](../adr/0002-capability-control-interface.md) records the decision rather than silently treating the original recommendation as accepted.

## Remaining decisions and verification limits

Alternative C and its [wire contract](../contracts/pty-control.md) are defined. Conditional consent preserves human authority until atomic acceptance, so countdown abort never revives a revoked token. Explicit owner/all background scope, unknown-view handling, monotonic activity and view revisions are specified there. Live fencing, settlement, native UI and the optimistic-answer presentation race still require proof; exact CLI porcelain remains deferred.

Controller attribution is trusted-local coordination, not a same-user security sandbox. A disappeared requester can leave a reservation until explicit cancellation/force; no timer may silently transfer it. In the selected capability model, explicit forced-claim creation uses a current compare-and-act observation, not possession of the disappeared requester's handle, so the reservation remains recoverable without silently targeting a newer owner.

Raw PTY write completion and harness-confirmed structured application remain different guarantees. Safe grant must expose unavailable/unknown settlement rather than relabel it successful. Force discloses this risk; it does not supply missing mandatory question/approval support.

The Interface proposals themselves have only shape/reference validation. The [private fencing follow-up](../archive/spikes/SPIKE-pty-control-fencing.md) adds real native/API evidence for a limited broker, not full wire conformance or production ownership: API-to-native drain and non-cancelling force worked, while omitted buffered native input invalidated a purported safe grant. Native application fencing remains open. No production implementation, automatic retry, scheduler, generic plugin framework or new review PASS was introduced.
