# e04 ACP threat model

Scope: ACP session/view/API additions in `/Users/addomam/projects/amux-acp-build`, `feat/acp-agent-interaction`, published baseline `a46eaddeb79a9a42628d9cb39cd60945ece27fbb`. Approved Plan round3 unlocked Build; first-story production code and local runtime evidence now exist. Overall risk: HIGH / P0. Threats below remain verification obligations, not findings or a blanket safety claim. Live provider/auth/model runs and main publication remain separately gated.

## Trust boundaries

1. Local caller/native attachment → daemon authority: controller registration and original connection provenance do not themselves authorize an action; current target-bound control is required.
2. Authority → canonical ACP admission: validation and immutable action/disposition commit must share the revocation ordering point. No later owner lookup may rebind stale drafts or discard accepted work.
3. Adapter stdout → parser/FIFO/application state: adapter data is untrusted; frame/queue/schema/resource limits precede accumulation and semantic publication.
4. Broker canonical content → actual host terminal: user/agent/tool/message/label strings are display data, never terminal escape instructions.
5. Daemon → owned child/descendants: launch argv/cwd and lifetime ownership are explicit; force takeover is not process lifecycle permission.
6. Private conversation/auth runtime → public metadata/evidence: snapshots/events/errors/evidence must not expose credentials, real content or provider environment/paths.

## Threats, mitigations and proof obligations

| Risk | Category/CWE | Threat | Required mitigation and verification |
|---|---|---|---|
| P0 | Improper authorization CWE-863 / TOCTOU CWE-367 | Old control passes a precheck and commits input after revoke; current control is substituted for an old draft | Authority→ACP state atomic commit; captured generation/target/interaction tuple; explicit barrier tests both orders, no stale target marker or losing answer. Every action route is gated in s01. |
| P0 | Insecure object binding CWE-639 | Session/request/tool/turn identity is guessed by arrival time or reused after close | Distinct target/instance/connection/session/RPC/work/prompt/interaction IDs; exact scope validation; close/reopen/stale-ID and concurrent question cases. |
| P0 | Improper lifecycle control CWE-664 | Force/cancel/socket loss implicitly interrupts, discards pending calls or kills a process | Force is authority-only; ordinary pending cancel grants nobody/revokes prior control/retains accepted work; explicit-interrupt-only protocol cancellation; same PID/session/RPC completes after transfer. |
| P0 | Race condition CWE-362 | API/native answers race or a late cancelling-session permission becomes selectable | Canonical first-valid admission; cancel reserves all response capacity and late permissions/forms commit nonselectable Cancelled/Cancel before publication; no second response/optimistic losing history. |
| P0 | Uncontrolled resource consumption CWE-400 | Newline-free frame, flood, blocked stdin/stdout/stderr or large schema grows memory/blocks all owners | Pre-accumulation 1MiB frame cap, count+byte queues including in-flight/reserved slots, bounded schemas/history/process admission; explicit failed/degraded+unknown, independent writer/FIFO, continued stderr drain; adversarial real-child tests. |
| P0 | Improper process control CWE-269 / resource lifecycle CWE-664 | Failed spawn/publication/close signals a reused PID/process group or leaves descendants | Existing setsid + birth-qualified ProcessTree, child captured before publication, outside-lock idempotent cleanup/join, concurrent close/shutdown/late-start and unrelated sentinel checks before fixture fallback. |
| P1 | Sensitive information exposure CWE-200 / log insertion CWE-117 | Raw prompt/answer/auth/env/provider errors leak in global snapshots/events or reports | Metadata-only bounded safe fields; explicit local content inspection; no raw tracing; sentinel privacy assertions and redacted live evidence. No form credential workflow. |
| P1 | Command/terminal injection CWE-78 / output neutralization CWE-116 | Shell interpolation or escape/control content changes host terminal state | Absolute executable argv direct spawn; no client env map or shell; ratatui text rendering with control/escape sanitization; real PTY test verifies visible inert text and no marker effect. |
| P1 | Unexpected privilege exposure CWE-862 | Permission UI fabricates allow-once, dismiss maps to deny, URL opens automatically or client capabilities promise delegation | Exact offered IDs/kinds, deliberate accept/decline/cancel, no automatic approval; URL/fs/terminal capabilities absent and invalid requests receive error. Agent-owned tools are not claimed sandboxed. |
| P1 | Deadlock CWE-833 | Native topology, authority, broker or blocking writer form a lock cycle | Authority→ACP state only; copy descriptors/release topology before projections; no I/O/spawn/probe/wait/join under locks; stalled transport plus concurrent snapshot/ownership/close tests. |

## Security verification approach

Use the scenario IDs in `tech-architecture/e04-TEST_PLAN_LATEST.md` (especially P0 identity/admission/resource/cleanup and P1 privacy/rendering). Test actual public daemon/CLI/PTY effects, not mocks echoing intended responses or source-text assertions. Synthetic sentinels only; no live auth material/model contact. Workers cannot alter shared authority/routing/manifests, install adapters, probe auth or bypass Plan/Build/live gates. Parent integrates, audits and independently reviews code; no task passes from this threat model or an architecture/Plan score.

No current-code HIGH findings are asserted here; code security review must trace the actual implemented paths and report evidence-backed findings (confidence ≥8/10), not treat planned mitigations as already present. Existing e03 open native-fence failure and exhausted review cap remain unchanged.
