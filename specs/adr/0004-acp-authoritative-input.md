# ADR 0004 — ACP sessions with amux-authoritative input admission

Status: architecture quality passed new e04 round2 (96.875 / 100), explicitly confirmed by user. Post-review clarifications below are mandatory inputs to new Plan review. User selected Plan, then Build if two independent Plan reviews pass. This is not runtime acceptance or provider/auth/main-publication approval; ADRs 0001–0003/e03s08 remain unchanged.

## Problem and approved requirement changes

The native Codex route cannot prove consumption of terminal-buffered application input. Its original complete-broker acceptance remains failing. ACP avoids that input route by giving amux the human interaction surface and one structured connection to the agent; it does not manufacture an acknowledgement for the native route.

The user selected:

1. **Amux-owned ACP view**, rather than an upstream TUI plus sidecar. Human prompts, genuine questions, permissions and API actions address the same ACP session. Native upstream terminal agents remain separate best-effort fallback.
2. **Fence amux input admission**, rather than waiting for provider-applied settlement. Accepted work can continue after transfer; accepted-by-amux and applied-by-agent are distinct. This is ACP-only: PTY settlement and its existing unknown outcomes are unchanged.

A thin non-ACP integration API is explicitly later work. No generic provider/plugin abstraction is justified now.

## Decision

### Concrete session and view, not a fake terminal

Add a concrete daemon-owned ACP broker. Each ACP instance owns one local child adapter, one connection generation and one ACP session. The daemon publishes a separate agent-view target under a project; it does not create a Zsh process, emulator or dummy PTY for that view. Existing terminal IDs, screen/resize/input/execute behavior and OMP lifecycle semantics remain unchanged.

Use explicit local operations for ACP open, inspect, authenticate, prompt, answer, permission, interrupt and close. Authentication is an explicit authorized transition using an advertised agent-type method; never synthesize credentials, auto-login, advertise auth.terminal/URL elicitation or invent client browser/terminal delegation. Keep existing Codex operations and closed request shapes intact; do not alias them to another protocol. Reuse ownership arbitration with explicit ACP target/admission metadata.

An ACP open reserves its target and initial authority before asynchronous initialization. The visible view can show starting, initializing, ready, authentication-required, failed or ended. This also permits genuine request-scoped interactions during initialization without an invisible blocking open. No hidden prompt is used to discover identity, capabilities or readiness.

**Reason for Depth — ACP broker:** one component must own bounded protocol routing, retained requests, ordered updates, interaction decisions and child lifetime so ownership transfer cannot implicitly cancel work or confuse two connections.

**Reason for Depth — ACP view:** a structured human surface is the actual admission authority; rendering a native TUI or a synthetic shell would recreate the unobservable application-input buffer being avoided.

### Stable protocol and package reuse

Adopt the official **`agent-client-protocol-schema` 1.12.0**, with default features disabled, for stable ACP **wire v1**. Reuse its typed JSON-RPC envelopes, request/response/notification routing enums and method payloads. Crate/schema versions are not wire versions; do not enable unstable v2. The pinned schema API includes:

```rust
use agent_client_protocol_schema::ProtocolVersion;
use agent_client_protocol_schema::v1::{
    InitializeRequest, NewSessionRequest, PromptRequest,
    ContentBlock, CancelNotification,
};
InitializeRequest::new(ProtocolVersion::V1);
NewSessionRequest::new(cwd);
PromptRequest::new(session_id, Vec::<ContentBlock>::new());
CancelNotification::new(session_id);
```

The schema crate publicly supplies `JsonRpcMessage<M>::wrap`, `into_inner`, `Request`, `Response`, `Notification`, `RequestId`, `AgentRequest` and `ClientRequest`; the request enums supply `method()`. Use these official serde types instead of copying an ACP schema or inventing a generic JSON-RPC framework. ACP framing is newline-delimited UTF-8 JSON-RPC, not Content-Length framing.

Direction is explicit: `ClientRequest` is client → agent; inbound permissions/forms are `AgentRequest` (agent → client), with the matching notification/response routing. Do not reverse those names because of the receiving side's role.

The higher-level SDK runtime **3.5.0 is evaluated but not selected**. Its `ByteStreams` accumulates an uncapped line; `Lines`, `Channel::duplex` and connection task queues are unbounded. Bounded OS pipes or a bounded amux transcript do not bound those internal queues. Avoid claiming otherwise, accepting that hidden limit or forking the SDK. Use a concrete amux-owned, bounded stdio transport with the official schema types and the existing per-backend worker pattern.

The SDK `AcpAgent` process helper is also not selected: tagged `ChildGuard::terminate` signals the original process group by child PID even after `wait_for_child` reaps it. Keep amux's established birth-qualified process cleanup. No upstream fork or patch is required.

The stdout reader caps bytes **before** newline accumulation and deserialization, including a newline-free or malformed frame; never use uncapped `read_line`/`lines`. Every inter-worker ingress/egress/application queue has count and byte budgets. There is no unbounded SDK transport/task queue on the selected path. One serialized application owner consumes wire notifications, exact response outcomes and closure in arrival order; bounded pending-request/interaction tables retain their original identities independently of ownership. Blocking pipe writes stay in a dedicated owned writer, never in the authority transaction or application consumer. A full queue fails/degrades explicitly with unknown affected outcomes; it never silently drops an update, blocks a callback behind another inbound response or reports success. Numeric budgets and deterministic oversized-frame/flood/stalled-consumer proofs are mandatory Plan work.

**Reason for Depth — bounded stdio transport:** the full SDK runtime cannot enforce the required frame/queue resource boundary; the small concrete worker boundary reuses the official wire model and amux lifecycle pattern without importing an asynchronous runtime or a generic protocol framework.

Package classification: official schema crate **[OK]**, maintained typed protocol model already used by SDK 3.5.0; maintained Codex/Claude adapters **[OK]** as explicit external launch targets. No extra transport/executor dependency is selected. No automatic download, `npx latest`, provider binary replacement or global configuration mutation. Verify the complete locked dependency/toolchain compatibility before Build; source API verification is not a compilation claim.

### Admission is atomic; execution is retained

All mutating human/UI/API actions carry the current control capability and exact target identity. A single bounded, memory-only authority transaction orders the control-generation check with committing the action to the ACP ledger/queue. Revocation/handoff uses the same target authority lock. There is no check-then-enqueue interval in which revoked input can newly become accepted.

The transaction performs no process, pipe, socket, filesystem I/O, waits or worker joins. Lock order is authority then ACP state; ACP update/worker code must not call back into authority while holding ACP state. Snapshot/lifecycle paths release component locks before crossing into another component. Reject capacity/validation failures before committing. The action's payload is immutable once admitted.

The cut divides actions into:

- **Not admitted:** stale capabilities, queued UI events, drafts and API submissions not committed before revocation cannot become accepted afterward.
- **Accepted by amux:** the action and its admission sequence are canonical work, including work waiting for transport. It remains inspectable and may be sent/completed after transfer. It is never silently removed, rebound to a new owner or represented as fresh input from that owner.
- **Transport/protocol outcome:** sending, response submission, agent-reported progress and prompt completion are separate observations. None is automatically an agent-application receipt.

Do not misuse the existing PTY `native-input` settlement ledger to represent this boundary. ACP work has its own receipt fields including `admission_boundary: amux`, accepted generation/sequence, dispatch state, protocol outcome and application certainty. A cooperative grant can proceed once the amux cut is sealed; it need not await the whole long-running `session/prompt` response. Otherwise takeover to answer a pending question could deadlock behind that very prompt.

Force changes authority only. It must not send `session/cancel` or `$/cancel_request`, discard the retained request/outcome mapping, terminate an adapter or replace a session. Ordinary pending cancellation invalidates prior control, grants nobody and retains work. Conditional keep/withdraw preserves human authority; timing/reacquisition decisions stay outside the broker.

ACP conditional claims must use a genuine live amux attachment focused on that ACP view, with explicit target-kind eligibility and original-connection provenance. Do not fabricate a native attachment/PTY to satisfy the existing PTY gate, or disable ACP keep/withdraw behavior. Local draft activity can update that real view's activity through its gated provenance path without creating a PTY `native-input` settlement batch.

**Reason for Depth — atomic admission transaction:** it closes the race between ownership revocation and canonical input commit without holding a registry lock across agent I/O or pretending transport settlement is application settlement.

### Identities and state transitions

Keep distinct: amux target ID, instance ID, connection generation, ACP session ID, amux prompt/work ID, JSON-RPC request ID, optional tool-call ID, interaction ID/revision, owner generation and event sequence. Validate every applicable identity at admission. A request-scoped elicitation is bound to its exact connection/request and instance; it is not guessed to belong to the most recent prompt. Session-scoped requests must match the live ACP session. One instance owns one session, not a shared multi-agent transport pool.

Stable v1 admits one foreground prompt at a time. A second prompt while active is `busy`; there is no fallback to steer or hidden queue/retry. Render user/agent text, thought, tools, plans and completion as their actual update categories. An update without prompt identity is not assigned to a prompt solely because it arrived during that turn.

One FIFO application consumer processes incoming notifications, exact JSON-RPC responses and connection closure; it never blocks waiting for another inbound response or for a long-running prompt. Register each outgoing request's identity/outcome record **before** it can reach the writer. Initialization/new-session and prompt requests remain outstanding while that same consumer continues to process genuine request-scoped forms, permissions and stream updates. Do not run a synchronous waiter as the application consumer. Retained work is independent of a caller connection, owner token or local timeout; none sends protocol cancellation or forgets the eventual response as a destructor side effect.

Retain accepted work through disconnect as confirmed, rejected or unknown according to actual evidence. Process the ordered closure marker once, mark still-unresolved work unknown, and tolerate late local write/error notifications without relabeling them as agent outcomes or confirmed cancellation. A reconnect/new adapter is a new instance/connection, not an invisible retry or revival of old controls. Event gaps require authoritative inspect/resync; bounded history must expose truncation rather than invent continuity. Never evict outstanding requests to satisfy a retention cap: reject new admission instead.

### Genuine interactions and shared disposition

Advertise stable form elicitation only after implementing the supported restricted schema. Preserve required/optional values, primitive/enum choices, multi-selection where in the pinned schema, and explicit accept/decline/cancel. Show agent identity, message and editable values before submission. Handle both session-scoped and request-scoped forms. Forms must not collect credentials. Do not advertise URL elicitation; an unadvertised mode gets the specified Invalid params error, not an invented question.

Permission UI/API select an exact offered option ID and display its actual allow/reject and once/always scope. Do not synthesize an unavailable allow-once option, treat dismiss as deny or automatically approve.

A canonical pending-interaction record owns the one protocol response identity. The first valid **amux admission** commits the selected disposition; competing/stale submissions are rejected without adding losing user answers to conversation history. This is an amux policy, not a claim that ACP supplies a first-winner protocol. `selected` means canonical local admission, `submitted` means the response was written to the transport, `failed` requires actual failure evidence, and `unknown` states that the later result is unconfirmed. None means agent application without independent evidence. Views render this same record, not an optimistic answer.

The human editor binds drafts/actions to target, connection, session and observed owner generation; interaction drafts also bind ID/revision. On revocation or identity change, quarantine/clear without auto-submit or replay. Observer mode never forwards raw key, paste, mouse or focus input to the agent. Ownership shortcuts remain distinct from application actions.

### Explicit interruption, close and failure

`session/cancel` is an explicit authorized action against the current amux prompt. Its admission also commits protocol-required `RequestPermissionOutcome::Cancelled` responses for every still-pending permission in that session; these are cancellation obligations, not user-selected offered options or automatic allow/deny. They use the retained response identities, participate in the canonical interaction race and bypass no ownership check: the authorized cancel action admits them. A permission already canonically selected is not given a second response. Pending form interactions receive truthful cancel/disposition handling, never an invented user decline. The cancel notification and permission responses are not confirmed turn cancellation: it remains pending until the original prompt response supplies a stop reason. Continue processing late updates; socket closure and local timeout are unknown, not confirmation.

Only the original prompt response's `stopReason: cancelled` confirms interruption. Other stop reasons are their actual outcomes, not cancellation proof. A JSON-RPC error records that error and releases the one foreground slot without implying agent application; it must not leave the session permanently busy.

The cancel commit reserves its required response capacity before acceptance; do not send `session/cancel` and then discover there is no room for its mandatory permission responses. While that admitted cancel remains pending, new permission requests for the cancelling session receive the same protocol-required Cancelled disposition under that retained cancel work. No new human answer or reacquisition policy is inferred. Session-scoped forms dismissed by that explicit interruption use the form `cancel` action, never `decline`; unrelated request-scoped initialization/auth interactions are not silently answered.

Late permissions and affected session-scoped forms are never published as human-selectable while that session is cancelling. Reserve the response slot and commit Cancelled / form Cancel atomically with accepting the inbound interaction under the retained cancel work; later human submissions are stale/withdrawn, not first-winner candidates. If capacity is unavailable, fail/degrade explicitly with unknown affected outcomes rather than offering a choice or leaving a half-retained responder.

Close/shutdown/failed launch are explicit resource lifecycles, not force takeover. Track children/descendants by the existing owned birth identity; bound stdout frames, stderr, retained transcript and pending work. Overflow is an explicit failed/degraded state with truthful unknown outcomes, not dropped updates followed by a success claim. Join cleanup without authority/topology locks. Failed startup, late publication and concurrent close/shutdown must all clean only owned resources and preserve unrelated sentinels.

User clarified lifecycle scope after Plan round2: keep the existing explicit close and daemon shutdown routes; defer broader lifecycle handling until hands-on UX feedback. There is no project-close product operation to invent here. Shutdown traverses ACP views/reservations in addition to native terminals; closing/removing a shell must not end a separate ACP view.

Continue draining/discarding stderr after its retention cap, or explicitly fail and reap the child. Never leave a live adapter blocked on a full stderr pipe because amux stopped reading.

No filesystem/terminal delegation capabilities are advertised initially. Agents may still have their own tools; absence of client delegation is not a sandbox/security guarantee. Use caller-selected absolute executable/argv and cwd, without shell interpolation or automatic install. Do not log environment/auth or raw prompt/answer content in public metadata events; raw transport tracing is excluded from committed evidence.

An unadvertised filesystem/terminal or unknown method receives an explicit JSON-RPC error with the exact request identity, not silent consumption. Unadvertised elicitation modes use the specified Invalid params response. Error-output capacity is bounded; inability to send the error is explicit transport failure/unknown work, never an implied rejection acknowledgement.

## Shared-module impact and ownership

The parent integration owner alone edits daemon/client/CLI routing, `src/control.rs`, top-level TUI dispatch, Cargo manifests and global ledgers. Once interfaces are reviewed/stable, an ACP implementation worker may own `src/acp.rs` and its protocol fixture; a view worker may own `src/tui/agent.rs` and view-focused scenarios. Neither edits shared routing. Conflicting changes are sequenced by the integration owner, not concurrent sessions. Use separate worktrees/sessions only for substantial independent slices; preserve and integrate all worker changes before retirement.

Do not extend the existing state-locked `Daemon::snapshot` / `terminal_snapshot` span across ACP state. Copy the ACP target descriptors under topology, release it, then obtain ACP/ownership projections without nested component locks. ACP admission/update paths must not publish back into daemon topology while holding authority or ACP state; drain metadata events after releasing the component lock. Preserve the native snapshot path unless a specific integration change requires otherwise.

Detailed purpose/caller/contracts and preservation boundaries are in `specs/IMPACT_LATEST.md`; story tasks and exact verification commands belong to the next Plan phase.

## Verification and approval boundaries

First use a deterministic local ACP peer with the real daemon, real stdio adapter lifetime, shared CLI/API and actual controlling-PTY TUI. Exercise input-vs-revoke and interaction races using explicit barriers, not sleeps as proof. Check process exit/private-resource cleanup before fixture fallback can hide leaks. Preserve all stock terminal/OMP/workspace/showcase regressions.

Complete acceptance additionally needs separately approved real-adapter checks: exact adapter/backend/model versions, private auth/config/HOME, bounded spend and minimal nonsecret artifacts. Codex adapter 2.2.2 launches Codex App Server; it does not attach an existing Codex TUI. Claude Agent ACP 0.94.0 similarly owns an SDK/native-agent backend. Both are maintained ACP adapters, not evidence that each provider's stock CLI speaks ACP natively. Do not spend model credits, probe auth or install/run an adapter merely to satisfy an architecture check.

Architecture and Plan each require new e04-scoped independent dual review; e03's exhausted five-round Build review and e03s08 failure remain untouched. This ADR is not runtime proof or Build authorization.

## Primary evidence

- ACP stable v1 initialization/session/turn: https://agentclientprotocol.com/protocol/v1/session-setup ; https://agentclientprotocol.com/protocol/v1/prompt-turn
- Genuine elicitation/capabilities: https://agentclientprotocol.com/protocol/v1/elicitation
- SDK tagged exports/API: https://github.com/agentclientprotocol/rust-sdk/blob/v3.5.0/src/agent-client-protocol/src/lib.rs
- Ordered dispatch/request retention: https://github.com/agentclientprotocol/rust-sdk/blob/v3.5.0/md/ordered-application-dispatch.md
- Selected official schema API/routing: https://docs.rs/agent-client-protocol-schema/1.12.0/agent_client_protocol_schema/ ; https://docs.rs/agent-client-protocol-schema/1.12.0/agent_client_protocol_schema/v1/struct.JsonRpcMessage.html ; https://docs.rs/agent-client-protocol-schema/1.12.0/agent_client_protocol_schema/v1/enum.AgentRequest.html
- Rejected SDK runtime queue bounds: https://github.com/agentclientprotocol/rust-sdk/blob/v3.5.0/src/agent-client-protocol/src/jsonrpc.rs#L7333-L7590
- Process-helper cleanup constraint: https://github.com/agentclientprotocol/rust-sdk/blob/v3.5.0/src/agent-client-protocol/src/acp_agent.rs#L248-L327
- Codex adapter source: https://github.com/agentclientprotocol/codex-acp/tree/202e66e63343ec4f5a5e35ab590251d813d129ca
- Claude adapter source: https://github.com/agentclientprotocol/claude-agent-acp/tree/7bcbfc7a5ca7ab9c7eece4f0749ccaf49c7ba7e1

Research used pinned primary sources. Context7 is not mounted in this session; no dependency/API claim is based on an unavailable Context7 call. Runtime behavior of those adapters has not been exercised for e04.
