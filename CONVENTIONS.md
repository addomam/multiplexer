# amux conventions
- MUST keep runtime state in memory and configuration on disk.
- MUST distinguish project, workspace, shell context, execution, and terminal.
- MUST preserve stock Zsh and OMP interaction.
- MUST use stable IDs and explicit API errors.
- MUST keep PTY processing independent from slow clients.
- MUST test observable behavior and process cleanup.
- MUST record plans and verification evidence in specs/.
- MUST use Conventional Commits when commits are requested.
- MUST NOT add remote protocols, restoration, tiling, or generic plugin infrastructure.

Commands: `cargo build`; `cargo test`; `cargo clippy --all-targets -- -D warnings`; `cargo run -- --help`.
