# e04 ACP architecture impact

Scope: new local ACP session path and amux-owned interaction view. No native application-fence repair or e03 acceptance/review reset. Architecture proposed; production code is unchanged. Detailed implementation Plan is gated on architecture review/confirmation.

## Existing module purpose, callers and contracts

| Module | Purpose / callers | Contracts and intended delta |
|---|---|---|
| `src/control.rs` | Daemon-authoritative controllers, target ownership, claims, grants and native activity; used by daemon routing, native TUI handles and existing Codex authority | Preserve every PTY generation, settlement, force/cancel, conditional and late-outcome boundary. Add an explicit ACP target/admission boundary: one memory-only authority transaction orders action commit against revoke. Do not clear native unknown batches or wait for whole ACP turns before handoff. |
| `src/daemon.rs` | Local API routing, live topology, snapshots, event fan-out and cleanup; consumed by CLI/client/TUI and integration tests | Add explicit ACP operations and separate project agent-view entries. Validate exact target/control/protocol identities. No fake PTY or new raw input route. Topology metadata is bounded/content-free and transcript explicit/incremental. Do not extend the existing state-locked snapshot span across ACP state: copy descriptors then release topology before ACP projection/event drain. No authority/ACP-state → daemon-topology inversion; no locks across I/O/join. |
| `src/client.rs`, `src/cli.rs` | Local request transport and user-facing commands; consumed by actual CLI and native attachment | Preserve existing socket framing/errors and all legacy CLI behavior. Add explicit local ACP launch/access ergonomics only where necessary; no config/auth probing, implicit adapter install or live inference. |
| `src/tui.rs`, `src/tui/control.rs`, `src/tui/view.rs` | Attachment event loop, native controller and project/terminal rendering; called by attach/detach and actual PTY consumers | Add separate ACP view dispatch/picker integration. Existing shells retain raw key/paste/mouse/focus routing and ownership gates. ACP actions are typed, identity-bound and authority-gated; no raw application bytes or replay. OMP agent state/wait semantics are unchanged. |
| `src/codex.rs` | Existing same-instance native-Codex broker and typed mutation/lifecycle tracking; called by current daemon agent-* operations | Remains legacy/native path, not the new ACP implementation. Keep closed request shapes, retained outcomes and owned cleanup. Do not retarget an existing Codex operation or attach a second writer. |
| `src/runtime.rs` | Actual PTY/emulator/process lifetime and execution delivery; called by daemon native terminal operations | No ACP PTY, fabricated screen or reinterpretation of write receipts. Reuse established birth-qualified process-cleanup technique without changing native write/settlement semantics. |
| `src/projects.rs` | Project/config persistence and runtime project topology; daemon store/picker callers | Configuration remains disk-only, runtime sessions memory-only. Project snapshots may contain explicit agent views; do not persist sessions, reinterpret terminal_limit or rename a terminal to include a non-PTY. |
| `Cargo.toml`, `Cargo.lock`, `src/lib.rs` | Dependency/module boundaries and build entry; all workspace consumers | Add pinned official schema 1.12.0 with default features off and a concrete ACP module; no SDK runtime/executor. Verify transitive MSRV and serde_json preserve_order feature unification against existing consumers. No general provider trait, remote protocol or unstable-v2 feature. |

## New responsibilities and depth

- **Concrete ACP broker:** owns one child/connection/session per instance, exact identities, bounded ordered work/transcript/interaction state and retained protocol-request/outcome mapping. Ownership transfer cannot discard a response or trigger protocol cancellation.
- **ACP view:** one typed human input surface over that same canonical broker; it prevents unobservable native buffers and rejects revoked drafts rather than replaying them.
- **Atomic ACP admission transaction:** holds only the target authority and ACP state for a bounded memory commit; it removes the validation/enqueue race without locking across agent I/O. Lock order is authority → ACP state; other paths must not acquire those in reverse.
- **Bounded stdio worker boundary:** caps each frame before accumulation/deserialization and bounds every ingress/egress/application queue by count and bytes; reuses official schema envelope/routing types and existing backend worker conventions. The higher SDK runtime's unbounded queues cannot supply this boundary, so it is not selected.

Do not introduce generic provider/session-plugin abstractions, a non-ACP adapter SDK, restoration, a task orchestrator or retry policy. Exact API/snapshot shapes and locked dependency compatibility must be settled by the next Plan, not guessed during implementation.

## Transitive consumers and preservation checks

- Project/picker rendering must distinguish ACP views from actual terminals; terminal screen/resize/input/execute remain PTY-only and fail explicitly for the wrong target kind.
- Ownership handle snapshots and event consumers must label the target/boundary; ACP work receipts are not PTY native-input settlement.
- Metadata events must not gain prompts, answers, raw transcript, credentials, environment or private auth paths. Explicit authorized interaction inspection is not a public metadata log.
- Stable v1 prompt completion, session cancellation and pending questions overlap: retain the original request/outcome mapping through handoff and cancellation, and never block the FIFO consumer on prompt settlement. Authorized cancel commits protocol-required Cancelled permission responses with reserved capacity; selected dispositions and transport outcomes remain distinct from confirmed cancellation.
- Permission/form answers commit one canonical local decision. Losing/revoked responses must not be appended to shared conversation history or shown as accepted by the agent.
- Failed initialization and late child/session publication must race safely with close/shutdown, without admitting a second adapter or killing an unrelated reused process group.
- Independent `terminal-showcase` remains independent; stock Zsh, OMP, current CLI/API and the delivered Codex route keep their existing regression boundaries.

## Evidence and limitations

Read-only source investigation covered daemon open/dispatch/snapshot/cleanup, ownership authority/settlement, Codex broker/request shapes, TUI event/input paths, project persistence and existing real-daemon/controlling-PTY fixtures. The baseline passed: `artifact://707` default preflight; `artifact://731` actual CLI help, 308 workspace Rust tests, strict Clippy/check/build/fmt, eight OMP tests and Zsh syntax. No ACP code exists or ACP runtime behavior is claimed yet.

LSP was attempted for `Daemon::agent_open` and its actual source-observed dispatch caller in the isolated clone; Rust Analyzer reported ready but returned no references/definition. A tool issue was reported. Workspace registration must be repaired/rechecked before exported-symbol changes; do not treat empty results as evidence of no callers. No exported runtime symbol was changed for this architecture work.

## Risk and verification strategy

Risk: **P0** for authority/admission races, stale identity, first-valid response, noncancelling force and process cleanup; **P1** for schema/form correctness, transcript ordering/privacy, explicit cancel uncertainty and TUI draft fencing. Metadata/picker documentation is lower risk but cannot substitute for runtime proof.

Use deterministic local protocol peers and the actual daemon/API/CLI/TUI, explicit barriers at race boundaries, real owned child/descendant cleanup before fixture fallback and unrelated sentinels. Keep new task ledgers failing until their exact runnable verification succeeds. Add permanent tests for observable boundaries/races, not source text, wiring or copied schema defaults.

Resource proof must exercise oversized newline-free/malformed frames, floods and stalled consumers across ingress and egress, not merely observe a capped transcript. The selected schema-only path has no hidden SDK queue; failure reports unknown affected work without fabricating success. The original SDK-runtime selection failed independent architecture round1 and is preserved in `verifications/e04-architecture-review-round1.json`.

Real Codex/Claude adapter acceptance remains separately approved work with exact dependency closure/model/auth isolation, bounded spend and redacted artifacts. Deterministic peers prove amux behavior, not provider compatibility. Existing `e03s08`, complete-broker failure and exhausted e03 review history remain unchanged.

## Integration ownership

Parent assistant exclusively owns shared daemon/control/client/CLI/top-level-TUI routing, manifests and global state. After reviewed interfaces stabilize, backend (`src/acp.rs` plus its fixture) and view (`src/tui/agent.rs`) may be substantial independent slices. Workers do not edit shared files or run build/lint/tests/formatters mid-flight. Parent integrates and runs verification afterward, then retires only integrated, preserved owned worktrees/sessions/processes. No merge/push/release of ACP is currently authorized.
