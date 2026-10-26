# E06 ACP daily-use impact

Risk: **High** for existing shared ACP bootstrap, authority and attachment routing; medium for profile launch UI. No new protocol/dependency/ownership arbiter.

- src/acp.rs: daemon reserve/start/prepare and native canonical projections. Extend genuine startup settings and actual OMP forms only where compatibility proves gaps; preserve exact responders and cleanup.
- src/daemon.rs: existing CLI/API/TUI dispatch and snapshot; concrete profile resolution reuses acp-open and its exact controller gate. No locks across external I/O.
- src/projects.rs: daemon Config persistence and snapshots; optional credential-free profiles, old files deserialize, runtime never persisted.
- src/cli.rs: human ACP start entry and existing API passthrough; preserve all existing commands.
- src/tui.rs, tui/control.rs, tui/agent.rs, tui/view.rs, client.rs: native launch and reconnect with fresh provenance, explicit lost/ended state and zero mutating replay.
- Tests: existing tests/acp.rs, native.rs and acceptance.rs cover ownership, same-instance questions/permissions, native input and cleanup; new omp.rs/daily_use.rs exercise actual consumer-visible compatibility/launch/loss transitions.

Source-observed callers are in PLAN. LSP references returned no known tui::run callers before and after reload; reported tool issue, complete fan-in count unavailable, never interpreted as no dependents. Read actual affected callers before signature migration.

Proceed with bounded plan, exact protocol identities, controlling-PTY/real-daemon proof and source-bound full regressions. Historical impact frozen in IMPACT-e05.md.
