---
type: wire-contract
context: specs/epics/e05-thin-tool-integration/context.yaml
---

# Tool integration v1 — proposed e05 contract

Status: specification, NOT shipped or runtime-accepted. User selected a language-neutral local API. Build and live-run authorization remain separate. ADR0005 and the e05 PLAN define implementation/review gates.

## Boundary

A daemon-owned integration executable owns exactly one real tool instance/session and exchanges UTF-8 JSON Lines on stdin/stdout. Tool authors implement this contract, not ACP, daemon controller credentials, terminal scraping or a second ownership machine. stderr is a bounded private diagnostic stream, never protocol or public evidence. No shell interpolation, listener, network transport or global registration is required.

Amux adds a concrete ToolV1 codec to its existing structured broker **in process**, alongside unchanged ACP v1. There is no intermediate bridge child. One target has one owned integration child, original pipe endpoints and birth-qualified cleanup observations. An adapter may embed a tool, as the OMP SDK consumer does; any additional child must remain observable within the owned process tree. Disconnection ends this instance; no reconnect, invisible replacement or auto-restart.

Human and automation use the existing amux-owned interaction view and authority. This does not preserve an upstream tool's original native TUI. Native terminal-only fallback remains best-effort; unknown native application settlement still blocks safe grant and e03s08 acceptance.

## Frames and identities

One object per line, no stdout banners. Every frame contains `v:1` and `type`. Unknown versions/types/keys, duplicate keys, malformed UTF-8, oversized frames and invalid method payloads are definite protocol failures, not ignored extensions. No partially parsed frame is published. Request/response IDs are nonempty strings, <=128 UTF-8 bytes, unique for the entire integration lifetime **within each direction**; direction is part of correlation. IDs cannot be reused after completion. Tool session ID is nonempty <=256 bytes and immutable after opening. Amux target/instance IDs remain daemon-created, distinct from tool session ID.

| Type | Closed envelope | Meaning |
|---|---|---|
| `request` | `v,type,id,method,params` | One operation in either direction; method-specific closed params below |
| `result` | `v,type,id,result` | Terminal successful reply to exactly one request in the opposite direction |
| `error` | `v,type,id,error:{code,message}` | Terminal failed reply to that exact request; not cancellation or task success |
| `event` | `v,type,method,params` | A declared notification; never input authority or a reply |

No agent-side frame carries a ControlHandle. Amux authenticates human/API callers before admission; the child receives only admitted operations. Amux call IDs, work IDs, ownership generation, tool wire IDs and interaction revisions are separate. Lost caller replies recover through amux's existing private inspect/work API, not replay of a child request. A result with unknown/wrong-direction/already-terminal ID is a protocol error; a crossed legitimate response after a withdrawal is handled by the explicit winner rules below, not reassigned to newer work.

## Amux to tool requests

| Method | Params | Successful result |
|---|---|---|
| `initialize` | `{protocol:1}` | `{protocol:1,name,version,capabilities:{prompt,questions,permissions,interrupt,withdraw}}`; all capabilities booleans, prompt required; no assumed capabilities |
| `session-open` | `{cwd}` absolute project working directory | `{session_id}`; exactly one session; no load/fork/replace |
| `prompt` | `{session_id,text}` | `{outcome:"completed"\|"cancelled"\|"failed",application:"acknowledged"\|"unknown"}`; completed is a turn result, not external task acceptance |
| `interrupt` | `{session_id,prompt_id}` | `{requested:true}`; prompt_id must be the exact still-outstanding tool prompt request, never latest/current guessed at delivery |
| `close` | `{session_id}` | `{closed:true}` only after the tool's semantic session is disposed; process/endpoints/descendants are independently observed by amux |

One foreground prompt per session; no implicit steer/follow-up queue. Unsupported operations fail `unsupported` before effect. Initialization/open errors never launch a replacement. Authentication is owned by the integration; `authentication-required` is explicit, without amux reading/installing credentials or invoking login. Working directory is supplied once; later frames cannot change project/launch configuration.

`application:acknowledged` requires a genuine tool-side acknowledgement for that exact prompt; callback invocation/queue insertion/stdin write is insufficient. It can be reported with a genuine prompt terminal result, not fabricated earlier. `unknown` is always permitted and visible. Failed/error/EOF alone cannot imply cancellation, application or success. An interrupt reply confirms only the request to interrupt; only the exact prompt's `outcome:cancelled` confirms cancellation. Close is lifecycle cleanup, not an alternative cancellation policy. Force takeover/pending claim cancellation never emits interrupt/close.

## Tool to amux requests

| Method | Params | Successful result |
|---|---|---|
| `question` | `{scope,message,schema}` | `{action:"accept",values}` OR `{action:"decline"}` OR `{action:"cancel"}` |
| `permission` | `{session_id,tool_call_id,message,options}` | `{option_id}` selecting exactly one offered ID OR `{cancelled:true}` |

`scope` is exactly `{session_id}` or `{request_id}`. Session scope must match the opened session; request scope must name an actually outstanding amux-to-tool request (including startup if a genuine integration requires it), not a model ID, work ID or guessed active prompt. No ambiguous dual scope. Permission scope always uses the exact opened session and nonempty genuine tool call ID.

ToolV1 reuses the existing form validator with a narrower preflight: require root `type:object`; allow root title/description/properties/required/_meta; support string (minLength/maxLength, enum or titled oneOf), finite number/i64 integer (minimum/maximum), boolean, and distinct string-selection arrays (minItems/maxItems, items enum or titled anyOf). Across the form: <=64 fields and <=256 choices; defaults, if provided, must validate but omitted optional answers remain unset. Preserve titles/descriptions and exact values. Reject nested objects, arbitrary arrays, composition, pattern/format, URL mode and unknown constraints before publication. This does not claim regex/format certification the shared validator lacks; ACP behavior remains unchanged.

Each permission option has exactly `{id,name,kind}`; kind is `allow_once`, `allow_always`, `reject_once` or `reject_always`. Preserve IDs, order, labels and kinds; no added choices, prose classification, widening of grant scope or default allow. Remembered decisions are the tool's offered semantics, not an amux grant database. An adapter unable to truthfully represent a scope must not advertise/offer it. Initial OMP adapter explicitly supports **once/deny only**, not session/persistent/folder grants; the generic contract supports all four kinds. This uses the SDK's documented host option to render a smaller approval UI, not silently rewriting a selected remembered grant into another grant.

The broker captures actual tool request ID, scope, canonical interaction ID/revision and original responder. One admitted response wins. Competing/stale/duplicate actions cannot consume or answer another callback. Ordinary definite amux non-admission leaves the original interaction pending with recoverable local draft/correction; the child receives no response until a real winner/cancellation. Ambiguous delivery remains inspectable unknown, never retry. Tool-origin errors are not forged human decisions.

## Tool events

`update` params: `{session_id,category,text}` with category `message`, `thought`, `tool`, `plan` or `notice`. Text is inert data. No response/work/turn attribution is inferred from update order; no ANSI/OSC/title/clipboard/bidi execution. Use the established canonical retained categories, pagination and explicit gaps. Native projection may replace unsafe control characters but must preserve canonical safe text without claiming byte-identical host rendering.

`interaction-withdrawn` params: `{request_id,reason}` where reason is `timeout`, `cancelled` or `failed`, permitted only when capability `withdraw:true`. The ID names a live tool-to-amux interaction. Tool withdrawals race through the **same canonical state transaction** as human/API admission. If withdrawal wins, mark withdrawn and reject later answers without sending a selection; send the tool exactly one terminal `result` with `action:cancel` for a question or `cancelled:true` for permission. If an answer already won, keep that decision and ignore the withdrawal as a crossed terminal notification for that known ID; do not undo it or select again. Late result acceptance at the tool is still distinct from amux admission. Unknown IDs/repeated withdrawals for unretained IDs fail, never target a newer question. Retain tombstones under the bounded lifetime-ID policy; capacity exhaustion ends admission, never permits reuse.

If a tool has an unavoidable deadline it must withdraw before abandoning its responder. Tool-controlled default/timeout behavior is external tool behavior, not a human answer; record it as withdrawal. An adapter must not label a recommended/default answer as human acceptance or use timeout to auto-grant permission. Explicit interrupt/close can cancel outstanding interactions using their exact original responder; they do not select deny/allow by inference.

## Bounds and transport

Hard limits apply **before accumulation or canonical publication**, including in-flight/reserved data: 1 MiB encoded line; 64 KiB prompt text; 64 KiB encoded question/permission content; 64 KiB encoded form values; 256 permission options; 64 flat form fields; 64 pending interactions; 4 MiB total retained interaction data. Reuse the established broker's stricter bounds when they apply. Prove both input and escaped response/envelope/canonical projection fit before publishing an answerable interaction; a frame-size check alone is insufficient. No visible unanswerable form or optimistic accepted transcript on serialization/capacity rejection.

Reuse existing bounded nonblocking pipe readers, dedicated blocking FIFO writer, stderr drain, cancellation wakeups, count+byte accounting and reserved protocol response/cancel capacity. Do not serialize JSON and reparse it merely to translate ToolV1 into ACP. Concrete codecs produce/consume shared typed canonical operations; move owned payloads rather than avoidable copies. Codec code cannot acquire authority independently or do child I/O inside the authority/canonical admission transaction. Maintain authority→broker lock order; no reverse/topology lock edge.

Combined live ACP/Tool targets share the existing eight-target structured process budget; native PTY capacity is separate. Preserve existing work/history/observer/watermark limits and explicit gaps. Tool wire lifetime ID retention is bounded at 4096 IDs per direction, with capacity reserved for mandatory replies, explicit interrupt and close. Exhausting normal request capacity prevents new application admission, never close/cancellation completion, ID eviction or reuse. Bounded tombstones distinguish crossed terminal withdrawal from unknown IDs. Do not automatically close admitted work to recycle capacity.

EOF, partial writes, child death and stalled pipes terminate continuity without replay. Keep original endpoints/process identity/worker handles until positive cleanup or explicit cleanup-unknown classification. Unknown cleanup retains resource charge; leader exit and `kill(0)` are not proof of endpoint/descendant/worker cleanup. Only explicitly owned, birth-qualified processes are signalled; unrelated sentinels must survive. Do not add a nested bridge process whose private endpoints disappear from daemon cleanup evidence.

## Public amux surface

Proposed CLI: `amux tool open --project <id> [--name <name>] [--cwd <absolute>] -- /absolute/integration [args...]`, `amux tool inspect <view>`, `amux tool attach <view>`. No shell evaluation or hidden setup turn. Keep existing ACP commands/payloads unchanged. One integration child and one tool session; no separate bridge executable.

New closed daemon operations: `tool-open`, `tool-inspect`, `tool-prompt`, `tool-answer`, `tool-permission`, `tool-interrupt`, `tool-close`. Mirror the established ACP operation envelopes and caller/call/target/control fields, substituting **kind `tool`**, not pretending this wire protocol is ACP. Tool targets live in `agent_views`, never `terminals`; native `input`, `execute`, `agent-*`, resize/PTY and ACP-only mutations must reject them. Existing ACP operations reject tool targets and tool operations reject ACP targets. Shared ownership claim/offer/accept/force/cancel/conditional/recover operations support the new target kind with unchanged semantics. Capability mismatch is explicit.

There is no tool-side authority API: new application input enters only through admitted human/API operations. Tool-internal autonomous work is not a second human input surface. Initial consumer disables ambient extension/session commands that could inject external human input outside amux. Do not promise arbitrary extension ecosystems are fenced.
