---
type: threat-model
context: specs/epics/e05-thin-tool-integration/context.yaml
---

# e05 threat model — HIGH / P0

Planning analysis, not runtime security acceptance. Trust boundaries: external local controller→daemon authority; daemon canonical state→owned stdio child; child/provider data→human terminal; private fixture/auth runtime→public evidence. Cooperating tool executable is explicitly launched, but all its frames/content remain untrusted. Existing process/endpoint proof must survive a concrete second protocol.

| Threat | Boundary / requirement | Verification obligation |
|---|---|---|
| Authority bypass / target confusion | Tool targets are not ACP or PTY aliases. Gate all typed mutations by exact current target/control/generation/call before canonical commit. Child receives no controller capability. | Wrong-kind/foreign/stale protocol/session/scope/revision and physical native input; actual downstream absence of stale effects. |
| Revocation TOCTOU / replay | One memory-only authority→broker commit orders against revoke; admitted work retained after handoff. No later-owner revalidation or hidden retry. | Both race orders under stalled transport; force/cancel/conditional semantics and lost replies with original work. |
| False genuine interaction / duplicate decision | Only actual original typed responder registered once; exact IDs/options/schema. Permission cannot be inferred from menu text or defaulted on timeout. | Concurrent human/API and withdrawal race; option scope/ID preservation, genuine question arithmetic and allow/deny real file effects. |
| Unbounded traffic/resource exhaustion | Pre-accumulation count+byte limits, escaped reply/projection budgets, bounded lifetime IDs/tombstones, reserved mandatory responses/interrupt/close. | Newline-free/oversize/malformed floods, held consumer, interaction exhaustion and native rejection correction; unrelated PTY responsiveness. |
| Process escape / premature cleanup release | One daemon-owned integration child with retained original endpoints; birth-qualified owned processes only. Unknown resource lifetime retains charge, leader exit insufficient. | Held duplicate pipe/escaped retainer, worker joins, child/descendant absence or explicit unknown before fallback; unrelated sentinel survives. |
| Terminal injection / misleading view | Protocol data inert, no title/OSC/CSI/clipboard/bidi execution. Capture exact draft identities; no optimistic acceptance or stale replay. | Actual controlling-PTY surface/effect observations, canonical categories/gaps and fresh deliberate input after revoke/reacquire. |
| Credentials/content disclosure | No auth install/read/probe without explicit approval; private HOME/roots and redacted receipts; logs content-free. Model SDK may warm connections on bootstrap. | OS network/home isolation offline; exact separate live approval and private-root cleanup; evidence/token review. |
| Dependency or extension bypass | Optional exact pinned OMP closure; disable ambient discovery/MCP/collab/session switching and queued external human input. No new Rust SDK runtime/unbounded transport. | Source/export/lock verification plus actual callback and provider acceptance; unsupported hook/capability fails, not a synthetic fallback. |

## Reviewer focus and exclusions

Trace the concrete path from each proposed mutating route through authority/canonical state to the original child responder, and through pipe/process/worker cleanup to resource release. Do not accept an abstract assertion that the existing ACP broker automatically makes ToolV1 safe: new target discriminants, codec scoping, withdrawal semantics and buffer budgets require their own proof. Shared-core/rename/migration regressions must rerun on changed source.

Do not report private controller handles existing solely inside authorized inspect/runtime as public credential leakage; do report any propagation into child frames, metadata, public logs or redacted evidence. An explicit authorized child command is not a shell injection when executed directly as argv; do report interpolation/ambient shell launch. Bounded explicitly opted-in detailed transcript inspect is not metadata leakage. Replacing unsupported terminal controls with inert canonical display is intentional, but must not erase safety-significant identity/choice semantics. A failed cleanup classified unknown and retaining charge is not a false complete-cleanup claim. Tool's genuine internal autonomous work is not a human authority bypass; an unfenced alternate external input surface is.

No global daemon/config/auth mutation, live model run or public release is authorized by this threat model. Fresh dual code review and security review after actual Build are separate from Plan approval; e03/e04 historical exhausted caps remain intact.
