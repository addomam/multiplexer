# Spike: same-instance native agent interaction

## Question

Can an owned local Codex app-server and ordinary native Codex TUI share one live agent thread, expose genuine questions and permissions through either native UI or API, and support the confirmed interaction-lock/takeover contract without weakening application-time fencing?

User-authorized timebox: 30 minutes. This was a throwaway feasibility experiment, not production implementation. Result: **partially answered; not ready to advance to Plan.**

Historical contract note: after this experiment, the user accepted unresolved-call risk for non-cancelling force, orchestrator-directed reacquisition and a shared per-PTY cooperative handoff whose current owner retains authority until yield, force or cancellation. The current [planning context](../../planning-context.yaml) supersedes the agent-only, immediate-on-request revocation, unconditional fencing and explicit-yield-only assumptions in this record. No additional runtime proof is implied.

## Result

**Proved:** ordinary native TUI and API coexist on one live thread over a private Unix socket; genuine model-generated questions are answerable from either surface; API allow/deny and native permission approval reach real command execution; late answers do not resolve a different pending question; explicit interruption is distinct from client detachment.

**Observed defect:** during a genuine concurrent native/API answer race, the server accepted Blue and the model returned `RACE_Blue`, but the ordinary native TUI printed `answer: Red` for its losing submission. The server resolved once; the native transcript was optimistic rather than authoritative in this race.

**Not proved:** a working amux automation lock, explicit human takeover, takeover event/hook, human ownership until yield, or atomic fencing at harness application time. No lock implementation or fake takeover event was introduced to manufacture proof. A controlled stopped-server experiment demonstrated why a socket write cannot be equated with authoritative application.

## Environment and isolation

- Installed `codex-cli 0.160.0`; macOS arm64.
- Owned temporary root, empty workspace, separate `CODEX_HOME`, private Unix socket; no TCP listener or external relay.
- Existing Codex login file copied privately with permissions preserved. No credential values were printed; the source login/configuration was not edited.
- Installed-version stable and experimental JSON schemas generated into the owned temporary root.
- Real account-backed `gpt-6.1-sol` turns; no fake provider, fabricated tool events, echo server or mocked agent.
- Native TUI launched on a real 120×40 controlling PTY with `codex resume --remote unix://<owned-socket> <thread-id> --no-alt-screen`.
- Update dialogs were dismissed; no binary update/install occurred.
- TUI observations below are actual emitted terminal surface bytes, not physical display/font certification.

## Findings and evidence

### 1. Same-instance native/API operation works

The private server accepted the standard WebSocket upgrade over its Unix socket and JSON-RPC initialization. An API-created thread processed a real prompt and returned:

```text
AMUX_SAME_INSTANCE_6F81
```

The native TUI resumed that same thread ID and rendered both the original prompt and that exact reply. Subsequent API-triggered questions appeared as actual native question dialogs, and native answers produced API resolution notifications on the same thread.

The first attempt to resume an API-created thread before its first turn failed with `no rollout found for thread id`. After a real turn persisted the thread, native resume succeeded. This is an observed lifecycle boundary, not an assertion that thread creation alone provides attachable history.

Two owned threads separated the question-only and command-permission scenarios. Each scenario's native TUI and API used the same thread; no shadow agent substituted for a native/API pair. The first thread ID was `01a1598e-0e30-7ae0-aae9-94de756584e2`; the command/shared-race thread was `01a15991-9fd4-71f1-b1e5-0bb8899476cf`.

### 2. Questions are genuinely structured and single-request scoped

A real model turn called `request_user_input`. The API received:

```json
{"method":"item/tool/requestUserInput","id":0,"params":{"threadId":"01a1598e-0e30-7ae0-aae9-94de756584e2","questions":[{"id":"color","header":"Marker color","question":"Which marker color?","isOther":true,"isSecret":false,"options":[{"label":"Red","description":"Use a red marker."},{"label":"Blue","description":"Use a blue marker."}]}],"isBlocking":true,"autoResolutionMs":null}}
```

Excerpt omits the observed turn/item identity fields for readability; those fields were present in the actual request.

API response:

```json
{"id":0,"result":{"answers":{"color":{"answers":["Blue"]}}}}
```

The server emitted `serverRequest/resolved` with `requestId: 0`; the actual model returned `CHOSEN_Blue`, which appeared in the native TUI.

A second genuine question used server request ID 1. Re-sending the old ID-0 answer did not resolve ID 1. Native Enter then selected Red; API observation resolved ID 1 and the model returned `SECOND_Red`.

Raw late responses were ignored by the harness, not acknowledged with a typed stale-response error. An amux adapter would need its own explicit pending-request validation/errors.

### 3. Real permission allow/deny works, including native approval

The model requested elevated execution for harmless, explicitly named marker files inside the owned workspace. No permission was bypassed and no session-wide approval was granted.

| Path | Actual decision | Observed effect |
|---|---|---|
| `denied-marker` | API `decline`, request 2 | File absent before and after; model returned `DENIED` |
| `api-allowed-marker` | API `accept`, request 3 | File created with exact content `API_ALLOWED` |
| `native-allowed-marker` | Native Enter on “Yes, proceed”, request 4 | File created with exact content `NATIVE_ALLOWED` |

All three requests were authoritative `item/commandExecution/requestApproval` messages. The same pending approval dialog was visible in the ordinary native TUI, and each decision produced the matching API `serverRequest/resolved` notification.

The first permission attempt reused a question-only thread whose earlier developer instructions prohibited file modification. The model correctly refused to execute. The actual permission experiment was isolated into a fresh thread, rather than bypassing those instructions or relabelling a refusal as an approval.

### 4. A native/API answer race exposes optimistic native display

For server request 5, the real native dialog selected Red while an API client submitted Blue. Two sender threads were released from one barrier; the API write completed first.

Observed authoritative result:

```text
serverRequest/resolved requestId=5: exactly one observed notification
model final response: RACE_Blue
```

Observed native surface excerpt:

```text
Questions 1/1 answered
  RACE?
  answer: Red
...
RACE_Blue
```

This is one directly observed race, not exhaustive concurrency certification. The server did not apply both answers, but the native transcript claimed the losing answer. Do not claim accurate shared resolution presentation merely because the final model output used the winning value.

### 5. Socket write is not the application boundary

A genuine `BOUNDARY` question was pending as request 6. The owned server process was stopped with SIGSTOP; `ps` observed state `Ts`. While the server could not consume requests, the API successfully wrote the Blue answer to the Unix WebSocket. A client-side revocation boundary timestamp was recorded after the write and before SIGCONT.

After resuming the server:

```text
serverRequest/resolved requestId=6
model final response: BOUNDARY_Blue
```

Thus the transport accepted a write before the host could apply it, and that answer applied after the recorded client-side boundary. **No actual amux lock or human takeover was implemented in this probe.** It demonstrates the in-flight boundary that any ownership design must address; it is not proof of a working or failed amux lock.

The generated experimental client-request schema exposed no interaction ownership/lock token or revocation method in the inspected control surface. `turn/steer` has `expectedTurnId`, which guards turn identity, not a change of interaction ownership within the same turn. Question/approval response shapes carry request ID and answer/decision, not an ownership generation.

A broker checking a token only before writing bytes cannot claim strict application-time fencing from this evidence. A design must establish an authoritative fence or an explicitly defined handover/drain protocol; it must not silently redefine socket enqueueing as completed application.

### 6. Detachment does not imply cancellation; interruption has its own lifecycle

With genuine request 7 pending, the owned native TUI process was terminated. The private server remained running, and `thread/read` reported:

```json
{"type":"active","activeFlags":["waitingOnUserInput"]}
```

The API answered Red and the same turn completed with `DETACH_Red`. This proves client lifetime is separable from current agent work in this setup. It does **not** prove the unimplemented human-takeover feature.

For genuine request 8, `turn/interrupt` returned an empty acknowledgement. The observed turn then completed with status `interrupted`; the server process stayed alive. `serverRequest/resolved` arrived after `turn/completed`, so adapters must tolerate that cleanup ordering rather than assuming all request cleanup precedes turn completion.

A later question used request 9. A late response to interrupted request 8 did not resolve request 9; answering request 9 produced `AFTER_CANCEL_Red`.

## Implications for design

- Codex 0.160.0 is a credible same-instance interaction candidate, not an accepted implementation choice or production-support guarantee. Its app-server surface is experimental.
- Preserve authoritative request identity, resolved notifications and explicit stale-response errors in amux; do not infer resolution from optimistic native transcript text.
- Human/native input gating belongs at an owned input/control boundary. Existing generic lifecycle reporting does not provide exclusive ownership or takeover semantics by itself.
- Resolve already-submitted in-flight actions before choosing the ownership commit/fence semantics. A client-local generation counter alone is insufficient evidence.
- A takeover notification must follow the authoritative ownership transition and remain independent of hook-consumer progress. No notification delivery or ownership hook was tested here because that feature does not exist yet.
- Keep turn completion, request cleanup, native-client lifetime, process lifetime, explicit cancellation and task acceptance separate.
- The confirmed contract is unchanged. No stock-native-UI replacement, weaker fencing, remote relay, upstream patch/fork or new compatibility shim was authorized or introduced.

## What was not explored

- A complete native/API racing matrix, permission races or multiple simultaneous question sets.
- Free-text/multi-select answer permutations, session-change/reconnect identity collisions, heartbeat/event-gap recovery or crash durability.
- A working amux lock/takeover implementation, owner-crash policy, human-yield operation, takeover hook delivery or slow-hook behavior.
- A proven host-acknowledged handover barrier or host-side ownership-generation validation.
- Fixing the native optimistic-answer race, upgrading Codex, altering OMP, or expanding remote protocol scope.
- Linux, alternative Codex versions, CI, coverage or a fresh independent review PASS.

## Recommendation

Continue within Elaborate using `deepen-architecture` to resolve the ownership/application boundary and the authoritative native-response presentation gap. Compare an authoritative host fence with an acknowledged handover protocol without silently weakening immediate-revocation expectations. Assess whether supported native interfaces suffice before proposing any upstream change.

Do not advance directly to `plan-work`, implementation or release: full interaction feasibility and the standard Elaborate quality/confirmation gates remain open. Preserve the existing independent-review cap.

## Experiment cleanup

The runtime experiment finished in 11.95 minutes, within the authorized 30-minute maximum. The private server exited 0; the last owned native client was terminated and reaped with status -15. A process-group check found no remaining member of the owned server group.

Removed the owned temporary root, private socket, generated schemas, marker files, session/configuration data and private credential copy. Filesystem checks confirmed both the temporary root and socket absent. No throwaway integration code was retained in the repository.
