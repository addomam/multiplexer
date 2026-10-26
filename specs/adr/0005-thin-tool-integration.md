---
type: architecture-decision
context: specs/epics/e05-thin-tool-integration/context.yaml
---

# ADR0005 — language-neutral cooperating-tool integration

Status: independently quality-reviewed with the e05 Build plan, round2 at100/100; implementation not yet authorized. User selected language-neutral local API, not a TypeScript-only SDK. Normative wire contract: [ToolV1](../contracts/tool-integration-v1.md); concrete consumer profile: [OMP adapter](../contracts/omp-tool-adapter.md). [Research receipt](../verifications/e05-contract-research.json) is source/CLI-metadata evidence, not runtime acceptance. [Review gate](../verifications/e05-plan-review-round2.json).

## Problem

A cooperating tool should expose its real questions, exact permission choices and live work to human/API controllers without implementing ACP. The accepted ACP broker already has the ownership, canonical first-winner, retained-work, inert view, resource and process cleanup boundaries. An alternate tool integration must not become a weaker second implementation of those boundaries.

## Decision

Add a **concrete in-process ToolV1 codec** to the existing broker/transport. Keep one daemon-owned integration process and one tool session. Separate protocol kind from the shared structured target semantics; `acp` stays ACP and new `tool` stays ToolV1. Rename the private `src/acp.rs` module to `src/structured_agent.rs` as the single cutover, with all private callers migrated after functioning LSP references. Move ACP wire-specific code into `src/acp_protocol.rs` only where it separates parsing/serialization from shared canonical/process logic; no public ACP API or behavior changes. Tool codec is `src/tool_protocol.rs`. Two explicit concrete variants, no plugin registry, trait hierarchy, async runtime or daemon-facing SDK.

**Reason for Depth:** the shared structured broker hides identity/admission/revocation, queue accounting and endpoint/process lifetime from both codecs and all consumers. The typed human editor hides stale drafts and prevents raw input/replay. These are existing deep boundaries extended to an actual second protocol, not new abstraction for hypothetical backends. Codec selection occurs once at launch and cannot change mid-instance. Semantic operations are typed/moved, not JSON→ACP JSON→JSON serialization round trips.

Keep the existing authority→broker memory-only admission transaction and existing structured input boundary. Add the closed tool target kind and route it to that transaction. Never reinterpret a tool as PTY or declare a wire write to be provider application. Canonical work retains original identities after force; pending takeover cancellation changes ownership only. Conditional reacquisition still requires the original live attachment facts and external policy.

The new public process protocol is transport, not authority. All external human/API input enters amux before the child. For the first embedded consumer disable ambient extension discovery, remote collaboration, MCP and uncontrolled session switching so it cannot offer an unfenced alternate human input path. Do not claim arbitrary unmodified OMP installations/extensions are fenced.

## Alternatives

- **Require ACP:** good existing standard, but defeats the explicitly requested thin non-ACP author experience. ACP remains unchanged and preferred where supported.
- **TypeScript/Bun-only SDK:** real OMP embedding hooks are useful, but user explicitly selected language-neutral. A pinned optional OMP adapter consumes the public wire contract; it does not define a mandatory author language.
- **ACP-speaking bridge process spawning the real integration:** reuses daemon routing but introduces a second protocol hop/queue and private original endpoints whose cleanup uncertainty may disappear when the bridge exits. Reject: use one owned child and the proven daemon transport instead.
- **Separate tool daemon/ownership/backend:** duplicates the hardest races and canonical outcomes; reject.
- **Plain OMP RPC or terminal menu parsing:** RPC has genuine question dialogs but typed approval degrades to generic select. Prompt text is not a permission discriminator; reject.
- **OMP collaboration:** remote relay/control and registry semantics add an out-of-scope transport and writer authority; reject.

## First consumers and package decision

1. A public Python-stdlib reference calculator/ledger integration demonstrates a non-ACP, non-JS author path with real state/file effects. Its questions are needed to finish arithmetic; its permission determines whether the ledger file is written. It is a real deterministic cooperating tool, **not proof of model/provider compatibility**.
2. A standalone OMP embedding adapter pinned to **18.12.6 / source45c35e40c46166dc726ccc52f5d27f40f2449f99**. Existing lifecycle extension remains on its original contract and unchanged. The SDK's genuine UI callbacks, `setToolUIContext` and exported `modes/runtime-init` initializer connect to the same AgentSession, not another agent. Typed `approve` maps only the advertised once/deny profile to actual SDK once/deny values. Remembered/session/folder grants are explicitly not supported by this first adapter. Real questions use actual callbacks and exact option/answer mapping, not prompt-crafted prose. Disable automatic external follow-up injection and implicit prompt queues.

`@oh-my-pi/pi-coding-agent@18.12.6`: **[OK — source-verified candidate, runtime pending]**, Bun>=1.3.14, optional adapter-only dependency, exact lock/closure required during Build. Root package exports embedding APIs; wildcard `modes/*` exports runtime-init. No private patch/fork or dependency in Rust core. SDK bootstrap can initiate provider connection/paid warming: never initialize it as a purportedly offline check without OS network denial and private HOME. Missing dependency closure or unsupported callbacks fails acceptance, not a synthetic fallback. No new Rust dependency: reuse pinned official ACP schema1.12.0 and current bounded transport.

## Consequences and gates

Shared core/routing migration is P0 and integration-owner-only. Verify ACP behavior and all existing regression boundaries after each core change; preserve the post-cap source/history without claiming old results cover changed code. LSP returned false-negative references in the relocated worktree: repair navigation before symbol migration; do not use an empty result as a fan-out proof.

ToolV1 deliberately has no login/configuration, reconnect/replay, durable runtime, scheduler, automatic answer/reacquisition policy or arbitrary SDK distribution. Exact limits/errors/withdrawal winner semantics are in the wire contract. Original endpoints and cleanup-unknown resource charge remain daemon-observable; no worker/child escape behind a successful bridge exit.

Both fresh independent reviewers must evaluate contract **and** Build plan, each >=94 with no must-fix, within a new e05-only five-round cap. E03/e04 exhausted caps remain unchanged. Build requires explicit user Plan-to-Build approval after the reviewed packet; new live runs require separately approved harness/model/auth/call/spend limits. Local main integration/push/release remains unauthorized. Plan approval is not runtime acceptance and cannot close historical native-fence failure.
