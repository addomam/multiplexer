# amux design
## Ownership
Daemon: PTYs, independent emulator per terminal, process cleanup, tree, projects, snapshots and sequenced event ring.
Client: focused terminal, scroll offset, selection, picker/filter. A picker overlays the surface and never resizes background terminals.
Background terminals retain their last attached size, initially 80x24. Only opening/focusing/resizing the visible terminal changes its size.
Unix sockets live in a private user-owned runtime directory. API v1 uses bounded newline JSON requests and responses. Same-user control is trusted, not a sandbox.
PTY readers and writers are per-terminal workers. Screen reads copy bounded cells under only that terminal's emulator lock. Structured events exclude PTY bytes.
Subscriptions start from an atomic snapshot sequence or explicit sequence. Bounded event retention reports gaps; active waits consume unmatched events. Socket writes occur outside global state locks. Screen updates use emulator generations rather than output events. Shutdown disconnects slow request sockets and drains worker ownership, including late PTY creation.

## Shell and OMP
Stock startup files are sourced through a private ZDOTDIR wrapper. Hooks are appended, not replaced. ZLE descriptor handling performs the final readiness/BUFFER check.
Raw input always goes to PTY foreground. Shell-source execution is a separate request, refused while busy by default; queueing is explicit.
Stock Zsh 5.9 needs a terminal-read wake after a descriptor callback. The adapter samples an input epoch after older input drains, rechecks its native buffer/typeahead, and requests a dedicated private key containing no source. Queued wakes hold the input gate and defer if the epoch changed; immediate execution already owns that gate through acknowledgement. Competing immediate submissions return busy while an earlier submission is pending, preventing gate/descriptor deadlock. Final native readiness precedes top-level source evaluation.
Context capture contains confirmed cwd and exported environment with capture time. Busy-source child launch reports stale capture rather than cloning interpreter state.
OMP extension uses tagged 18.8.7 lifecycle callbacks and local JSON reports. Root status, child status, process lifetime, shell readiness and conversation ID are separate.
Agent finished means an explicit root `agent_end` with matching completed generation and no retry, continuation, question or approval. Cursor-based finish waits require that event, not an idle heartbeat; cursorless waits query the current settled level. Finished does not mean task success. Lost reports/crash become unknown, never successful completion.

## Projects and Git
Start unsaved with a main host shell. Only its confirmed cwd updates context (repository root if applicable); each other terminal retains its workspace.
Saved project IDs persist with configuration only. Save resolves the latest confirmed main-shell cwd/repository root outside global locks before persisting; an explicit re-save updates that location. Organizational tree is independent of process ancestry; reject cycles and cross-project parenting. New terminals choose their default main parent atomically at publication, independently of capture source. Concurrent project opens reserve creation so hooks run once.
Removal performs process cleanup outside the tree lock, then removes the current Entry and rewrites its children atomically; an earlier snapshot's ancestry is never reused. This prevents concurrent removals/reparenting from publishing dangling parents or cycles.
Git cache key includes actual workspace, canonical Git/worktree directory and repository identity, not branch. Refresh off the PTY path; bounded cache pins active workspaces and evicts inactive entries. Clock-only cache age changes do not create state events.
Hooks: daemon-start once, project-open once per open, never reattach. One bounded worker runs startup then project jobs, in configuration order. Cwd is the daemon initial directory or project context; environment is inherited exports. Per-hook timeout; bounded stdout/stderr in snapshot diagnostics and sequenced events; failures do not suppress a usable shell. Shutdown skips pending jobs and joins the active hook through its timeout.

## Modules and depth
runtime: owns PTY/emulator/process lifetime; hides OS cleanup and threading from clients.
Cleanup validates kernel birth identities before signals, freezes/rescans descendant producers, sends TERM/CONT with a grace period, then freezes/rescans and KILLs. Natural exit records descendants before reaping the original root. Freeze waits are bounded; OS permission changes/uninterruptible I/O and already unobserved orphaning remain explicit limits.
integration: owns shell-safe structured submissions and lifecycle reporting; hides handshake/readiness races.
projects: owns persistent configuration, hooks and Git cache; isolates disk/Git work from terminal I/O.
client: owns local request framing; same contract for CLI, extension and TUI.
tui: owns presentation/input translation only.
No generic plugin layer, distributed service or recovery journal.
