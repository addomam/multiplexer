# amux conventions
- MUST keep live runtime state in memory. Only configuration and bounded agent conversation reopening metadata may persist; agent-native conversation backing stores remain separate.
- MUST distinguish project, workspace, shell context, execution, and terminal.
- MUST preserve stock Zsh and OMP interaction.
- MUST use stable IDs and explicit API errors.
- MUST keep PTY processing independent from slow clients.
- MUST test observable behavior and process cleanup.
- MUST record plans and verification evidence in specs/.
- MUST use Conventional Commits when commits are requested.
- MUST NOT add remote protocols, PTY/process restoration, automatic work continuation, tiling, or generic plugin infrastructure. Reopening an exact saved agent conversation on relaunch is allowed; input, tool calls, permissions and old authority are never replayed.

Commands: `cargo build`; `cargo test`; `cargo clippy --all-targets -- -D warnings`; `cargo run -- --help`.
