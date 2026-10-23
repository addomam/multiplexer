# Spike: PTY control fencing and application boundaries

## Question

Can one isolated broker gate the ordinary native Codex PTY and structured API answers, wait for authoritative settlement before cooperative takeover, reject stale authority, and grant forced control without cancelling the running turn?

User-agreed maximum: **30 minutes**. Runtime experiment started `2026-10-21T14:58:14+00:00` and owned runtime resources were removed at `2026-10-21T15:16:37.523108+00:00`: **18.39 minutes**. This is a throwaway control-seam experiment, not production amux implementation or full wire-contract conformance.

## Result

**Partially answered.** API-answer-to-native handoff worked in one genuine two-question turn; explicit force preserved another turn while an earlier API answer landed after the grant. Cancellation left nobody in control and retained outstanding work; the previous controller acquired fresh authority immediately.

**New counterexample:** a native Enter already written into a stopped TUI's PTY applied after an API-only ledger's incorrectly labelled safe grant. Current API-call settlement accounting is insufficient for native-origin safe handoff. A fixed delay cannot establish a missing native application acknowledgement.

The [wire contract](../../contracts/pty-control.md) already requires all forwarded work to be accounted for. The deliberately incomplete negative-control broker does not meet that contract. Do not weaken safe grant to make this experiment pass.

Subsequent user direction: the broker was intentionally incomplete; defer deeper investigation of this edge and revisit it during final complete-broker validation if it remains unresolved. The experiment remains historical evidence, not proof that a complete broker fails. [Current planning context](../../planning-context.yaml) and [ADR 0003](../../adr/0003-codex-first-agent-target.md) govern the continuing work order and first target.

## Environment and isolation

- Installed `codex-cli 0.160.0`, real account-backed `gpt-6.1-sol`; generated installed-version experimental schemas.
- Owned root `/tmp/amux-fence-jg6x4rgg`, separate workspace and `CODEX_HOME`, private Unix WebSocket endpoint; existing login copied privately without printing credentials or editing its source.
- Unmodified native `codex resume --remote unix://<private-socket> <thread-id> --no-alt-screen`, on a real 120×40 controlling PTY. `ps` showed native PID/PGID/TPGID `49660` on `ttys006`.
- One live thread, `01a15efb-cf2d-7fa0-b643-6acb511ecec3`, shared by the API connection and native TUI for every scenario. A bootstrap model response `AMUX_FENCE_SAME_INSTANCE` appeared in the native surface.
- The private Python broker owned input writes to that PTY and API answer writes, minted opaque role-tagged handles, serialized authority checks and transitions, retained dispatched API calls, and correlated real `serverRequest/resolved` notifications. Its finite subset was not an amux daemon, raw wire implementation, generic relay or shipping component.
- Real model-generated `request_user_input` calls and real native dialog answers. No fake provider, fabricated resolution event, echo server, screen-scraped structured answer, upstream patch, production daemon change or binary installation.
- Automatic terminal query replies continued separately from controller input. Surface evidence below is actual emitted TUI text with ANSI controls stripped, not physical font/layout certification.

## Findings

### 1. Cooperative API-answer drain before native control

Turn `01a15efe-e550-7940-a740-86cce90f1948`, request IDs `0` and `1`.

With a genuine first question visible in the ordinary TUI, SIGSTOP paused the owned server group; `ps` observed `Ts`. The API Blue answer successfully wrote while the host could not apply it. A human claim retained the API owner's authority until explicit yield. Yield revoked it but did not grant the human control.

Probe inspection after 3,154 ms showed `draining`, owner null, and the original forwarded `dispatch-1` / request 0 / generation 1. Premature claimant input failed `wrong-handle-kind`; an old-token answer failed `stale-control`; acceptance before offer failed `not-ready`. No native input was forwarded during that wait. This was probe inspection, not a newly implemented progress popup or event timer.

After SIGCONT, the real host emitted:

```json
{"method":"serverRequest/resolved","params":{"threadId":"01a15efb-cf2d-7fa0-b643-6acb511ecec3","requestId":0}}
```

Only then did the broker offer and accept the human grant at generation 3. The host next requested the second question in the **same turn**; `thread/read` reported `active` with `waitingOnUserInput`. Native Enter answered Red. The model returned `SAFE_Blue_Red`, and the native transcript showed `SAFE SECOND?`, `answer: Red`, and that same final marker. The turn completed normally.

New old-owner answers remained rejected without another API write. Reusing the consumed offer failed without changing current ownership. This supports the specific known-question settlement seam, not a generic fence for every API operation.

### 2. Forced takeover does not imply turn cancellation

Turn `01a15f01-fc78-7f92-8872-f5f2f3685f60`, request IDs `2` and `3`.

Again, the stopped server held a forwarded Blue answer. Explicit force granted fresh human control immediately and retained `dispatch-2` as unresolved. While the server remained `Ts`, a native Down key changed the actual local dialog from Red to Blue: human input was not waiting for server cooperation. A new call under revoked API authority failed before another write.

After server resume, the already-forwarded answer resolved **after** force. The same turn then asked its follow-up question; native Enter answered Red and the actual model returned `FORCE_Blue_Red` with status `completed`. Force itself sent no interruption or stop; the earlier SIGSTOP was deliberate fault injection. This is the accepted unresolved-call risk, not evidence that force retracts old calls.

### 3. Native PTY delivery is not native application settlement

Turn `01a15f03-da51-7481-ac95-f703d9df2d4b`, request IDs `4` and `5`.

This was a negative control against a broker that tracked structured API answers but **did not account for native input watermarks**:

1. The real native TUI displayed `NATIVE BUFFER?`, with Red selected.
2. SIGSTOP paused the native process group, not the API server; `ps` observed `Ts+`.
3. Human-authorized Enter successfully wrote into the PTY. The host still reported `waitingOnUserInput`; the question had not resolved.
4. The API-only ledger saw no outstanding API calls, allowed yield/offer/accept and incorrectly labelled the API grant safe. New input using the revoked human token was correctly rejected.
5. SIGCONT resumed the native TUI. That **earlier buffered Enter** answered Red after the purported safe grant. The same turn then asked its follow-up; the new API owner answered Blue and the model returned `BUFFER_Red_Blue`.

This failure is about omitted native work, not failure of fresh-token rejection: newly submitted stale calls were rejected while already-delivered bytes still applied. `writer.write_all` or a drained daemon queue cannot certify the native application's input/event/RPC queues are quiescent. A resolution of some unrelated question would not certify those queues either.

The generated request schema exposed no operation named for a native-input fence, flush, lease or quiescence. Inspected initialize capabilities expose experimental/notification/extension negotiation; `turn/steer.expectedTurnId` guards turn identity, not an ownership epoch. `serverRequest/resolved` carries thread/request IDs, not a native input watermark, winning answer or controller generation. This is a missing **proved** seam; it is not a claim that every possible native integration is impossible.

### 4. Cancelled takeover and fresh prior-owner acquisition

Turn `01a15f07-1cc5-7842-87ff-0500e7dce40f`, request ID `6`.

With another real API Blue answer forwarded to the stopped server:

- A force receipt captured before a new reservation was rejected as stale even though the ownership generation had not changed. Reservation identity matters separately from generation.
- An in-process 200 ms waiter timed out while the claim remained `waiting-owner` and the current owner stayed valid. This does not prove process/SIGINT/one-shot transport behavior in production.
- Explicit cancellation before yield revoked prior authority, cancelled the human reservation and left owner null. The pending call remained `forwarded`; no human capability was granted.
- A different controller's safe acquisition failed `unsettled`. The prior API controller immediately acquired a fresh capability (generation 10), without an inactivity delay or reviving the old reference.
- After server resume, the retained answer still applied and the model returned `CANCEL_Blue`, normally completed. Native input count during cancellation/reacquisition was zero; outstanding automation was not fabricated away as empty interaction history.

### Single-probe timing observations

These are controlled observations, **not latency targets or benchmarks**.

| Observed interval | Milliseconds |
|---|---:|
| Forwarded API answer to authoritative resolution in safe scenario | 3156 |
| That resolution to fresh human grant | 0.137 |
| Forced human grant to older API answer resolution | 408 |
| Buffered native Enter write to its resolution | 306 |
| Incorrect API-only safe grant to buffered native resolution | 1 |

## Implications for the plan

- Keep the selected control/claim/offer capability design and cooperative timing. Reject stale dispatch and track what already left the gate; never infer success from notification delivery or a local timeout.
- Codex's resolved-request event can support known structured-question drain under the controlled path tested here. It is not a blanket applied/rejected acknowledgement for arbitrary mutations or queued native input.
- Add a native-input application/quiescence seam to first-harness feasibility. Candidate seams must account for PTY-buffered input, application event processing and delayed native-origin calls, including owner reacquisition. Without a proved seam, retain native input as unknown and block **safe** grant; explicit force remains separately available with the recorded risk.
- Do not substitute a 5/10-second sleep, whole-turn completion, killing/restarting the native TUI, simulated keystrokes or an unapproved upstream patch for that seam. A compatible native acknowledgement mechanism or another harness must be evaluated without weakening ordinary-native/same-instance requirements.
- Existing amux `runtime::Input` has bytes/ack queueing; the writer acknowledgement follows `write_all`. The [source](../../../src/runtime.rs#L324-L335) does not supply the missing application acknowledgement, and queued input needs generation validation at actual dispatch in a future implementation.

## What was not explored

- Production amux ownership operations, full schema conformance, controller credential recovery across real one-shot clients, restart or event-gap recovery.
- A correct native-origin safe handoff, general native input/application fencing, or a solution to the earlier optimistic losing-answer presentation defect.
- Native lock badge/border, takeover/owner popups, actual three-second progress UI, multiple attachments, mouse/paste routing, inactivity countdown and background reacquisition.
- New permission scenarios (the earlier private spike remains the allow/deny evidence), free-text/multi-select permutations, hook consumers, load/backpressure behavior or arbitrary harness mutations.
- Production allocation/performance design. The tiny broker serialized its small transport writes with its state lock and is not suitable for shipping or slow-writer isolation.
- OMP capability implementation, a patched/forked harness, Linux, CI, acceptance/quality gates or an independent review PASS.

## Recommendation

Remain in Elaborate. Evaluate the native input/application acknowledgement seam before accepting Codex or choosing a first harness; retain the current contract's fail-closed settlement rule. The API-to-human result narrows the unknowns but does not make symmetric safe handoff complete. Do not advance directly to Plan/build/release or silently reset the inherited exhausted review cap.

## Experiment cleanup

Private API connection closed; owned native process reaped with status `-15`; server exited `0`. A process-group scan found no remaining member of either owned group. The watchdog was cancelled after process cleanup.

Removed the owned root, Unix socket, private credential copy, generated schemas, workspace/session data and logs; existence checks confirmed root and socket absent. No throwaway implementation was added to repository source or retained as a test. No production/default daemon was restarted or signalled.
