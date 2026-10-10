# amux design
## Ownership
Daemon: PTYs, independent emulator per terminal, process cleanup, tree, projects, snapshots and sequenced event ring.
Client: focused terminal, scroll offset, selection, picker/filter. A picker overlays the surface and never resizes background terminals.
Background terminals retain their last attached size, initially 80x24. Only opening/focusing/resizing the visible terminal changes its size.
Unix sockets live in a private user-owned runtime directory. API v1 uses bounded newline JSON requests and responses. Same-user control is trusted, not a sandbox.
PTY readers and writers are per-terminal workers. Screen reads copy bounded cells under only that terminal's emulator lock. Structured events exclude PTY bytes.
Subscriptions start from an atomic snapshot sequence or explicit sequence. Bounded event retention reports gaps. Writes have deadlines; slow clients cannot hold global state locks.

## Shell and OMP
Stock startup files are sourced through a private ZDOTDIR wrapper. Hooks are appended, not replaced. ZLE descriptor handling performs the final readiness/BUFFER check.
Raw input always goes to PTY foreground. Shell-source execution is a separate request, refused while busy by default; queueing is explicit.
Context capture contains confirmed cwd and exported environment with capture time. Busy-source child launch reports stale capture rather than cloning interpreter state.
OMP extension uses tagged 18.8.7 lifecycle callbacks and local JSON reports. Root status, child status, process lifetime, shell readiness and conversation ID are separate.
Agent finished means root agent_end/idle after retries and continuations settle; turn_end alone is not sufficient. Lost reports/crash become unknown, never successful completion.

## Projects and Git
Start unsaved with a main host shell. Only its confirmed cwd updates context (repository root if applicable); each other terminal retains its workspace.
Saved project IDs persist with configuration only. Organizational tree is independent of process ancestry; reject cycles and cross-project parenting.
Git cache key includes actual workspace, canonical Git/worktree directory and repository identity, not branch. Refresh off the PTY path.
Hooks: daemon-start once, project-open once per open, never reattach. Sequential configuration order; project cwd; inherited exported environment; per-hook timeout; captured output and failures in diagnostics. Failure does not suppress a usable shell.

## Modules and depth
runtime: owns PTY/emulator/process lifetime; hides OS cleanup and threading from clients.
integration: owns shell-safe structured submissions and lifecycle reporting; hides handshake/readiness races.
projects: owns persistent configuration, hooks and Git cache; isolates disk/Git work from terminal I/O.
client: owns local request framing; same contract for CLI, extension and TUI.
tui: owns presentation/input translation only.
No generic plugin layer, distributed service or recovery journal.
