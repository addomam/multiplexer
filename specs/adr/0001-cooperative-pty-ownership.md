---
status: accepted
---
# Cooperate on per-PTY interaction ownership

Use one daemon-authoritative interaction lock per live PTY, shared by external controllers and attached native clients rather than scoped only to an agent instance. A control request notifies the current owner without invalidating its token; safe transfer follows owner relinquishment, authoritative settlement of tracked calls and requester acceptance, so an available owner can reach a suitable handoff point without an automatic takeover deadline.

## Consequences

- The daemon reserves the pending transfer for its requester and commits fresh ownership atomically; notification or peer acknowledgement alone never grants input authority.
- Cancelling a pending takeover withdraws the request, invalidates the pre-transfer owner's token and leaves the PTY unowned. The previous orchestrator may request fresh authority immediately; amux does not automatically restore it or replay intercepted requester input.
- Explicit force revokes prior authority and grants the requester control without stopping the agent or cancelling its turn. Already-dispatched calls may still apply, and outstanding-call records and warnings survive that transition.
- All attached clients use the same handoff mechanism. The owning native client receives a control-request popup; viewing remains available to other attachments.
- A PTY ownership generation is distinct from an agent instance, conversation, execution, event sequence or terminal screen generation. Agent-targeted calls additionally check their live harness/request identity.
- Inactivity/background reacquisition policy remains external. No timer automatically steals control; slow or missing notification consumers cannot delay an explicit force or cancellation.

## Considered alternatives

- Immediate revocation merely on requesting control: rejected in favor of allowing the current owner to cooperate and finish its coordination step. Relinquishment, explicit force and cancellation still invalidate prior authority.
- Agent-only ownership: rejected because stock shells and multiple native attachments also need exclusive PTY interaction.
- Automatically returning ownership on cancellation: rejected; the previous controller must acquire a fresh lock and inspect actual interactions/outcomes rather than receive revived authority or fabricated empty history.

The full contract is in [planning context](../planning-context.yaml); source-based constraints are in [architecture exploration](../tech-architecture/agent-interaction-deepening.md). This accepted product/architecture decision does not assert implementation, runtime verification or passage of the Elaborate quality gate.
