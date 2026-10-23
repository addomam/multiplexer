# Same-instance agent interaction: feasibility assessment

## Verdict

Requirements are user-confirmed and persisted in [planning-context.yaml](../planning-context.yaml). **No harness has passed the complete production runtime acceptance contract.** [ADR 0003](../adr/0003-codex-first-agent-target.md) selects Codex CLI 0.160.0 local app-server/native TUI as the first implementation target on the strength of the real question/permission probes. Target selection is not a runtime PASS. The separately authorized [scoped design review](agent-interaction-design-review.md) and [e03 Plan quality gate](e03-plan-review.md) passed. Explicit Build confirmation and implementation/runtime acceptance remain outstanding.

The user's OMP preference does not weaken the mandatory same-instance native TUI, structured questions, permission allow/deny, interaction ownership and human-takeover requirements. The user explicitly permits another first harness when supported interfaces fit better.

## Evidence boundaries

Two parallel read-only research agents failed with `getaddrinfo ENOTFOUND chatgpt.com` after the computer was suspended; neither produced usable research. Findings below come from subsequent direct CLI inspection and primary-source reads, not from those failed jobs.

A large recursive GitHub-tree response was incomplete in the reader artifact. It was not used as evidence of absent APIs; investigation switched to the complete scoped OMP collaboration subtree.

### OMP 18.8.7

Observed CLI results:

- `command -v omp`: `/opt/homebrew/bin/omp`.
- `omp --version`: `omp/18.8.7`.
- `realpath /opt/homebrew/bin/omp`: `/opt/homebrew/Cellar/omp/18.8.7/bin/omp`.
- `omp --help`: native interactive mode plus `text`, `json`, `rpc` and `rpc-ui` modes.
- `omp collab --help`: local host metadata listing and retrieval of view/control links.

Primary-source findings, pinned to 18.8.7:

- [Public extension API](https://github.com/can1357/oh-my-pi/blob/v18.8.7/packages/coding-agent/src/extensibility/extensions/types.ts): `sendUserMessage`, `ExtensionContext.abort`, and approval requested/resolved notifications exist. The inspected approval notification types contain identity and decision metadata, not a response resolver. This does not prove no other interface exists.
- [RPC types](https://github.com/can1357/oh-my-pi/blob/v18.8.7/packages/coding-agent/src/modes/rpc/rpc-types.ts): `extension_ui_request` and `extension_ui_response` correlate UI requests and answers by ID; rich questions require the host's `set_ask_dialog` opt-in.
- [RPC implementation](https://github.com/can1357/oh-my-pi/blob/v18.8.7/packages/coding-agent/src/modes/rpc/rpc-mode.ts): `rpc-ui` routes tool UI over the protocol. This does not establish coexistence with the ordinary native TUI on that instance.
- [Native collaboration host](https://github.com/can1357/oh-my-pi/blob/v18.8.7/packages/coding-agent/src/collab/host.ts): the authoritative native session mirrors UI requests to writable guests, with request-end notifications and guarded resolution. Host/guest traffic uses `CollabSocket` and a relay.
- [Collaboration controller](https://github.com/can1357/oh-my-pi/blob/v18.8.7/packages/coding-agent/src/collab/controller.ts): hosting follows relay configuration, starts/rotates rooms and gates guest actions during startup/session changes.
- [Local collaboration registry](https://github.com/can1357/oh-my-pi/blob/v18.8.7/packages/coding-agent/src/collab/registry.ts): its Unix-socket contracts are `snapshot` metadata and `link` retrieval. The inspected registry is discovery/capability distribution, not a local command/answer transport.
- [Collaboration settings](https://github.com/can1357/oh-my-pi/blob/v18.8.7/packages/coding-agent/src/collab/settings.ts): relay-backed auto-hosting is opt-in; no settings were changed.
- Existing `integrations/omp/amux.ts` reports lifecycle and heartbeats; it has no structured answer/control channel.

Conclusion: native same-session collaboration exists, but the inspected route requires relay-backed collaboration. No supported local-only OMP path satisfying the entire confirmed contract was established. Do not silently enable a relay, patch OMP, monkey-patch native UI internals, or substitute RPC-only operation.

### Codex CLI 0.160.0

Observed CLI results:

- `command -v codex`: `/opt/homebrew/bin/codex`.
- `codex --version`: `codex-cli 0.160.0`.
- `codex --help`: native TUI `--remote` accepts `unix://` and `unix://PATH`, in addition to WebSocket URLs.
- `codex app-server --help`: `--listen` accepts `stdio://`, `unix://`, `unix://PATH`, WebSocket and `off`; the app-server is labelled experimental.
- `codex resume --help`: a native TUI can select a session ID while using the `--remote` endpoint.

[Official app-server documentation](https://developers.openai.com/codex/app-server) describes:

- Ordinary native CLI TUI attachment to app-server via `--remote`.
- A Unix-domain transport using WebSocket framing over the local socket, without requiring a TCP listener or external relay.
- `thread/start`, `thread/resume`, `turn/start`, `turn/steer` and `turn/interrupt`.
- Structured `item/tool/requestUserInput`, command/file approval requests and permission grants.
- `serverRequest/resolved` notifications identifying resolved/cleared pending requests, including question cleanup on interruption or turn transitions.
- Version-specific schema generation using the installed CLI.

Caution: the documentation is current upstream documentation, not a full version-pinned source audit. Installed help confirms native/local transport flags, not every current protocol field. The documentation labels app-server/WebSocket surfaces experimental; do not turn an exposed CLI surface into a production-support guarantee.

Conclusion: a concrete local same-instance native/API route is documented and its transport flags are present in the installed binary. Full multi-client request arbitration, native-input gating, session selection and application-time ownership fencing remain unproved.

### Claude Code

[Official CLI documentation](https://code.claude.com/docs/en/cli-reference) distinguishes native interaction, background-session attachment, and print/stream-JSON mode. [Agent-view documentation](https://code.claude.com/docs/en/agent-view) describes native background-session attachment and replies, but states that replying from the peek panel does not resolve permission dialogs; those require attachment. [Hook documentation](https://code.claude.com/docs/en/hooks) exposes permission decisions and lifecycle hooks. [Mod event documentation](https://code.claude.com/docs/en/plugins/mods/events) adds in-process permission/prompt interception.

These are useful extension points, not proof of the complete concurrent local control/answer/fencing contract. No installed-version check, native runtime experiment or exhaustive Claude interface audit was performed. Claude Code is not selected or ruled out categorically.

## Required runtime experiment

Original spike question: can an owned local Codex app-server and ordinary Codex native TUI share one live thread while an API client resolves genuine questions/approvals, and can interaction authority be fenced at the point actions take effect without cancelling existing agent work? The subsequent safe/forced handover and orchestrator-directed reacquisition refinement below supersedes the original unconditional fence and explicit-yield-only requirements.

The experiment must use isolated configuration, workspace and a private Unix socket. It must not inspect/control existing user sessions, start an external relay/TCP service, change installed binaries, or leave processes/artifacts behind. Start with model-free transport/native-attachment checks; genuine question and approval behavior requires a real harness path, not fabricated events or an echo server.

Evidence needed before declaring feasibility passed:

1. Native TUI and API identify the same live thread/instance.
2. Genuine question and permission requests can be answered through either interface; native and API views observe authoritative resolution.
3. A native/API response race cannot resolve twice or answer a later request.
4. Per-PTY ownership gates interaction from all non-owning native attachments and external controllers while preserving observations and native protocol progress.
5. Cooperative requests retain the owner's token until yield; safe grant settles forwarded calls, reserves transfer for the requester and waits for its acceptance before committing fresh ownership.
6. Explicit force enables human input without cancelling the turn, warns that already-dispatched calls may still land and never revives revoked authority.
7. Cancelling pending takeover grants no ownership, invalidates the pre-transfer token and permits fresh acquisition without restoring it; delayed handshake messages cannot affect newer ownership.
8. Reacquisition occurs only under an eligible external-orchestrator request or explicit hand-back, honors the inactivity warning and human cancellation, and cannot revive stale generations.

A socket write remains insufficient evidence of safe drain. Forced takeover is the user's explicit acceptance of unresolved-call risk, not proof of application-time fencing.

## Orchestration gates

- Requirement confirmation: passed by explicit user selection.
- Confirmed planning context: persisted.
- Production harness/runtime acceptance: open for implementation/Verify; the complete feature does not need to exist merely to finish its specification.
- Elaborate independent quality gate: passed in the newly authorized specification-only scope; the historical implementation cap remains exhausted and unchanged.
- Plan: artifacts and independent dual quality gate passed; explicit Build confirmation is outstanding. Build/release are unstarted.
- Generic YAML cockpit validator: existing release/execution schema mismatch remains; this assessment makes no schema-migration claim.

## Runtime follow-up

The user authorized a 30-minute private spike. [Actual runtime findings](../archive/spikes/SPIKE-native-agent-interaction.md) prove same-thread native/API attachment, genuine question answers and permission allow/deny, with real model turns and tool effects. The experiment finished in 11.95 minutes; owned processes and temporary artifacts were removed.

Full feasibility remains open. A concurrent answer race resolved once to API Blue, while the native TUI printed its losing Red answer. A stopped-server probe demonstrated that an answer already written to the socket can apply after a client-local revocation point. No working ownership lock, takeover hook or authoritative fencing was implemented or claimed.

Current checkpoint: the [e03 capsule](../epics/e03-agent-interaction/PLAN.md) and [dual Plan quality gate](e03-plan-review.md) passed; await explicit Plan-to-Build confirmation. The scoped reviews did not reset the historical implementation cap. Deeper native-buffer investigation remains deferred until final complete-broker validation if unresolved. The [wire contract](../contracts/pty-control.md), [native UX](../tech-architecture/pty-control-native-ux.md), and [first-target decision](../adr/0003-codex-first-agent-target.md) remain normative. No runtime PASS or Build/release authorization is implied.

## Ownership refinement after the spike

The user selected non-cancelling force takeover: immediate native input, no implicit turn interruption or process stop, with a warning that already-dispatched calls may still land. The subsequently confirmed cooperative model instead retains the current owner's token on request and invalidates it on yield, force or cancellation. Safe grant waits for authoritative settlement and reserved-requester acceptance. Pending handoff shows owner/request-level blockers after 3 seconds and offers wait/cancel/force without an automatic takeover deadline.

The user also replaced explicit-yield-only human ownership with orchestrator-directed reacquisition. The external orchestrator may request a lock after inactivity (suggested 60 seconds plus a visible 5-second countdown), on backgrounding, on explicit hand-back, or never. Human input cancels the countdown; amux does not automatically retry. Current ownership/activity conditions must be rechecked before granting a fresh token.

These are requirements, not newly exercised runtime behavior. The earlier spike still proves only its recorded interactions; lock UI, cooperative handoff, capabilities, safe drain, force and reacquisition are unimplemented. The [wire contract](../contracts/pty-control.md) now defines concrete wire types, controller recovery, conditional-consent linearization and attachment-scoped observations. CLI porcelain is intentionally deferred without weakening the first-acceptance contract.

Raw entry-point smoke: `cargo run -- api --help` exited 0 and printed `Usage: amux api [OPTIONS] <REQUEST>`. This exercised existing help parsing only, not a capability operation or live daemon.

## Capability contract validation

The [schema](../contracts/pty-control.schema.yaml) is valid JSON Schema Draft 2020-12. An isolated `uv` runner with `jsonschema` and `pyyaml` validated all 73 [fixtures](../contracts/pty-control.examples.yaml): 44 accepted messages and 29 deliberately rejected messages matched their expected shape results; valid request/response/event fixtures also matched the root union. Valid requests remained valid after the current raw CLI's top-level null-omission transformation; meaningful view nulls are nested. The validator exited 0. No project dependency was added.

Reproduce the shape check from the repository root:

```bash
uv run --no-project --with jsonschema --with pyyaml python - <<'PY'
from pathlib import Path
import yaml
from jsonschema import Draft202012Validator

base = Path("specs/contracts")
schema = yaml.safe_load((base / "pty-control.schema.yaml").read_text())["schema"]
cases = yaml.safe_load((base / "pty-control.examples.yaml").read_text())["cases"]
Draft202012Validator.check_schema(schema)
for case in cases:
    validator = Draft202012Validator({
        "$defs": schema["$defs"],
        "$ref": "#/$defs/" + case["definition"],
    })
    assert validator.is_valid(case["message"]) == case["valid"], case["name"]
    if case["valid"]:
        assert Draft202012Validator(schema).is_valid(case["message"]), case["name"]
print(f"{len(cases)} shape expectations passed")
PY
```

This is an exercised schema/serialization check, not a simulated daemon or a fencing test. Current owner validity, capability binding, view provenance, handoff races and authoritative application settlement require real runtime proof. No production command, native UI, daemon ownership mutation or harness interaction was exercised in this contract step. Elaborate remains active and the inherited independent-review cap remains exhausted.

## Private fencing follow-up

The user agreed another 30-minute isolated spike. [Full findings and cleanup](../archive/spikes/SPIKE-pty-control-fencing.md) record 18.39 minutes of runtime experimentation with Codex 0.160.0, one genuine native/API thread, seven real questions and four normally completed turns:

- API Blue answer held at a stopped host; cooperative human grant waited for its matching authoritative resolution, then native Red answered the next question in the same turn: `SAFE_Blue_Red`.
- Explicit force granted native input while the older API answer remained unresolved; it later applied and the unchanged turn continued: `FORCE_Blue_Red`.
- Negative control: Enter buffered in a stopped native TUI applied after an intentionally incomplete API-only ledger incorrectly declared safe handoff: `BUFFER_Red_Blue`. This is a missing native-input fence, not a passing implementation.
- Cancelling before owner yield revoked old authority, granted the human nothing, retained the forwarded call and permitted immediate fresh prior-controller acquisition: `CANCEL_Blue`. An obsolete reservation receipt was rejected despite unchanged generation.

The private broker covered only a subset and was not production amux or full wire conformance. It supplies no native lock UI, idle/background reacquisition, multi-attachment, CLI/socket-lifecycle or general native quiescence proof. Native input must remain unknown until a real application boundary is established; neither a sleep nor an unrelated structured-request receipt is sufficient.

Owned native/server processes were reaped, their process groups were empty, and private socket/root/credential copy/schema/session data were removed. Source implementation, default daemon and review cap remain unchanged. The bootstrap marker turn was additional setup, not one of the four ownership-scenario turns above.

## Elaborate continuation after user deferral

The incomplete-broker native-buffer case remains a final complete-broker acceptance check, not the next research task. The user requested continuing the rest of the specification. Safe grant still accounts for native input; no unknown outcome is reclassified successful and no force warning is removed.

[ADR 0003](../adr/0003-codex-first-agent-target.md) selects the first Codex local integration target without changing existing OMP/stock-shell defaults. Its operation matrix covers observation, prompt start/steer, structured question/permission response and exact-turn cancellation; fresh native-first startup still needs implementation validation rather than a hidden bootstrap model turn. The [native UX contract](../tech-architecture/pty-control-native-ux.md) specifies bindings, owner notices, input interception, wait/cancel/force, conditional keep-control and attachment lifetime.

The standard dual Elaborate quality gate passed in the explicitly authorized specification-only scope, with all findings addressed, and the user confirmed locked decisions and progression to Plan. The e03 capsule subsequently passed its independent dual Plan quality gate; separate Build confirmation is still required. Full production behavior remains implementation/Verify acceptance. Historical mechanical/runtime passing fields in state.yaml refer to the prior substrate, not this feature; the historical exhausted review record was not reset.

Initial continuation artifact check, before scoped review: four YAML artifacts parsed without duplicate keys, 64 local Markdown references resolved, and the native-UX SVG parsed as self-contained themed XML. All 32 acceptance scenario IDs remained present; phase 2 and the historical five-round exhausted review record were preserved. These were document/state checks only; no new UI, harness runtime or independent quality review was part of that check. Subsequent expanded contract checks and actual independent verdicts are recorded in the scoped design-review evidence.
