# E07 modal UX impact

## Target and purpose
- `src/tui.rs`: native attachment lifecycle, original authority, UI event classification and daemon dispatch. Called by CLI attach/start. Parent integration owner only.
- `src/tui/agent.rs`: bounded inert canonical conversation/history and capability-bound local interaction editor. Called only by the attachment loop; returns envelopes, never acquires control or forwards PTY bytes.
- `src/tui/project.rs`: saved bookmark forms and explicit project requests. Called by attachment Menu.
- `src/tui/launch.rs`: deliberate catalog-based ACP launch form. Called by attachment Menu/start flow, returns the original launch request.
- `src/tui/view.rs`: native canvas/Menu/help rendering and common painted hit geometry. Parent only.
- `src/tui/control.rs`: ownership presentation, consent and application input gates. Parent only; preserve full-paint/input-drain guards.
- `src/daemon.rs`, `src/projects.rs`, `src/acp.rs`: authoritative runtime metadata, configuration validation, project save normalization and canonical ACP work. One backend owner; no provider/wire/authority changes.

## Callers and contracts
`cli -> tui::{run,start} -> attachment loop -> project/launch/agent/view/control`; attachment requests use the original connection/controller. Daemon JSON consumers include CLI, OMP integration and existing native/API regression suites. Rename affects only in-memory labels. New independent ACP project placement must be atomic/rollback-safe, no filesystem creation. Project preview uses the same bounded outside-lock workspace resolution as save; save rejects an intervening directory change. Attention derives from canonical pending obligations/foreground/lifecycle, not transport readiness or inferred application acknowledgement. Configuration gains a defaulted validated function mode key without changing user configuration.

## Risk: High
Shared routing admits application input and exact typed responses. Menu commands must never interpret session/search/form text, deliver intercepted input later, auto-acquire authority, acknowledge unknown settlement or weaken existing force/cancel guards. Explicit literal-key pass-through goes through the existing input gate under fresh authority. Dialog mouse actions need the same reviewed identity as keyboard actions.

## Coverage and baseline
- Existing `tests/pty_control/native.rs` ownership, queued-force, incomplete-input, resize, recovery and no-replay cases.
- Existing `tests/acp/native*`, daily-use/launch/recovery cases and canonical form/permission tests.
- Existing project registration/configuration conflict, rollback, cwd/Git-root and native editor rejection tests.
- Existing actual private TUI assessment: ACP question hidden at110x28 and entirely absent at80x14, duplicate generic rows, live close without second confirmation, JSON basic project editing.
- Baseline `cargo test --workspace`, bg_378/artifact://2944: fails existing `native::e06s03_native_filtered_terminal_replies_cannot_confirm_queued_force_enter` at native detach deadline; 37/38 in that native suite. No production edit had begun. Preserve this as baseline failure; do not claim green or weaken the boundary.
- Rust LSP was attempted for Config/Dialog/AgentView references and parent definitions, then reloaded: symbols work but semantic hover/definition/references return none despite real consumers. Reported through xd://report_issue. Use explicit source contracts while semantic indexing is unavailable.

## Recommended action
Proceed with approved isolated UI initiative. Parent owns shared event routing, view/control, all integration, final checks and private actual-terminal smoke. Independently owned ACP/project/launch/backend files can be developed concurrently, with no mid-flight builds/tests/lint/formatters/provider calls. Existing historical acceptance/review/provider limits remain unchanged. No new dependencies or generic UI/plugin framework.
