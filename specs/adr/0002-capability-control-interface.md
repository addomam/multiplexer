---
status: accepted
---
# Use typed capabilities for PTY control

Choose the control/claim/offer capability Interface (alternative C) over the initially recommended named-verb Interface. Distinct runtime capabilities encode authority, pending intent and ready-to-accept transfer; prioritizing raw Interface plumbing over early CLI ergonomics makes that separation more valuable than a smaller shell-facing lifecycle vocabulary. The cooperative per-PTY protocol in [ADR 0001](0001-cooperative-pty-ownership.md) is unchanged.

## Consequences

- Only a current `ControlHandle` authorizes interaction. A `ClaimHandle` tracks pending intent and can be inspected, cancelled or explicitly forced; an `OfferHandle` accepts a reserved safe handoff. Handle kinds are enforced by the daemon, not merely by a client wrapper.
- Handles are serializable opaque references to daemon-memory state, bound to controller, live PTY, generation and applicable transfer identity. They survive ordinary one-shot socket closure, not daemon restart or PTY replacement. Input reuses cached control authority without per-key handle allocation or registration.
- The operation/method set is closed and ownership-specific. This is not a generic object registry, capability delegation framework, dynamic RPC bus or plugin mechanism.
- Primitive calls return promptly. An asynchronous caller retains the returned claim; read-only inspection can report a ready offer, but acquisition completes only after its acceptance returns current control authority. A synchronous CLI wrapper can drive the same read/wait/accept lifecycle client-side; it must not make the daemon or native event loop block on cooperation.
- Existing `amux api` supplies raw request access. Exact lock/check/wait command names, flags such as `-f`, and ergonomic exit-code conventions are deferred. Raw API exit 0 means a successful request, not necessarily ownership; a future acquisition waiter can reserve exit 0 for an actual granted `ControlHandle`.
- A local wait timeout, SIGINT or socket loss does not automatically cancel the claim, surrender granted control or prove that acquisition failed. Inspect the controller/claim's authoritative state before choosing explicit cancellation or force; do not replay an uncertain mutation.
- Explicit force grants control without turn interruption or process stop and reports unresolved prior calls. Creating a forced claim can override a stranded reservation using a current compare-and-act observation without possessing the missing requester's private handle; a stale observation cannot target an unexpectedly newer owner.
- CLI porcelain deferral does not waive the native ownership UI, same-instance structured questions/permissions or other first-acceptance requirements.

The alternatives are recorded in [Interface designs](../tech-architecture/pty-control-interface-designs.md). The normative [wire contract](../contracts/pty-control.md), [schema](../contracts/pty-control.schema.yaml) and [examples](../contracts/pty-control.examples.yaml) define concrete messages, controller recovery, conditional consent and activity/view linearization. CLI porcelain remains deferred. These are design artifacts, not implementation evidence or an Elaborate quality-gate PASS.
