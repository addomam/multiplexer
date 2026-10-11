# Shell and OMP integration

Supported source interfaces: stock **Zsh 5.9** with interactive ZLE enabled, and **OMP 18.8.7**. Control is local and same-user trusted. Runtime files and command sources are private. Source travels over the control channel, never as typed command text; final acceptance requires an empty native primary ZLE prompt.

## Zsh

The private `ZDOTDIR` contains wrappers for `.zshenv`, `.zprofile`, `.zshrc`, `.zlogin`, and `.zlogout`. Zsh itself retains responsibility for the global startup files. Each wrapper sources the original user file once, following user changes to `ZDOTDIR` between startup stages. The adapter is appended to the interactive `.zshrc`; existing shell hooks and widgets are retained through `add-zsh-hook` and `add-zle-hook-widget`. It registers its own `zle -F -w` descriptor widget and does not replace `accept-line` or an existing editing widget.

`execute` writes a private source file and sends only an execution ID, explicit queue flag, and prompt generation through a separate FIFO. The native widget checks the current primary prompt, empty `BUFFER`, pending terminal bytes, queued ZLE keys, and prompt generation immediately before installing the source command. A stale nonqueued request is rejected if the user has already submitted another command. Partial input is never replaced. Foreground apps remain busy even when their output resembles a prompt. Source is evaluated at the shell's native top level, not inside a function: `typeset`, exports, option changes and cwd have normal shell-source semantics. Ordinary keyboard and paste input still go directly to the PTY.

Stock Zsh 5.9 continues its blocking terminal read after a descriptor callback; `accept-line` or `zle -U` alone cannot wake it. The descriptor widget checks the buffer, samples an input epoch after older daemon input is drained, then repeats its native buffer/typeahead check. The daemon sends a private widget key (`ESC [ amux-<terminal-id> ~`) only if that epoch is unchanged. Queued wakeups serialize with raw input; immediate execution already owns the gate through native acknowledgement, so its report worker does not reacquire it. Intervening input defers queued work instead of appending a private key behind a new foreground command. The key contains no source; the final native widget checks readiness again before top-level evaluation. No existing binding is replaced. See [Zsh 5.9 input loop](https://raw.githubusercontent.com/zsh-users/zsh/zsh-5.9/Src/Zle/zle_main.c).


Nonqueued submissions wait for the adapter's acceptance/rejection acknowledgement; cached busy/partial states can reject early, but cannot grant execution. An earlier pending submission also returns busy unless queueing is explicit, preventing a competing immediate request from holding the input gate needed by a queued wake. Queued submissions return `status: queued` immediately and are processed in FIFO order only at an empty primary prompt. Queueing is never implicit. An acknowledgement timeout invalidates shell readiness and removes the unaccepted source file. Completed executions include the shell status. Explicit removal deletes that terminal's private integration files.

If Ctrl-C skips the trailing completion command, the next native `precmd` completes the accepted execution with the interrupted status (130 on the exercised Zsh). The adapter captures `$?` before its own work; user hooks and native signal handling remain intact.

A `precmd` capture records exported environment, cwd, and capture time. Readiness is reported only from actual ZLE, not from `precmd`. Only named integration transport variables are excluded; user exports and removals remain intact. The private startup `ZDOTDIR` is translated back to the user's startup directory only while still unchanged; post-startup exported changes or removal select the child's actual startup location. Freshness is `confirmed` at a live prompt, `stale` while busy, and `unknown` after integration loss or exit. Interpreter internals, local variables, aliases and functions are not cloned. Disabling RCS/ZLE or removing integration hooks disables structured execution instead of enabling a typed-source fallback.

The daemon must service `report` concurrently and release runtime/global locks before waiting in `Integration::execute`. Child launch merges the source capture first, restores `AMUX_ORIGINAL_ZDOTDIR` from its user `ZDOTDIR`, then overlays `prepare()` transport variables.

## OMP

The shell's small `omp` function invokes `command omp -e "$AMUX_OMP_EXTENSION" "$@"`. If the user already defines an `omp` function, it is retained as `_amux_user_omp`, and the wrapper delegates to it with the extension flag. That user launcher must forward its arguments normally. Pre-existing aliases remain unchanged rather than being silently discarded; aliases that bypass the function must supply `-e "$AMUX_OMP_EXTENSION"` explicitly. Manual stock launches and launch actions otherwise use the same extension; stock OMP flags and UI remain available. `command omp` deliberately bypasses the wrapper. A bypass launch is unobserved unless the extension is supplied explicitly; unknown state is not completion. The extension loads only when `AMUX_SOCKET` and `AMUX_TERMINAL` identify an amux terminal.

The picker/CLI OMP action invokes the stock binary directly with `command omp -e "$AMUX_OMP_EXTENSION"`, independent of user aliases/functions, to load the reporter once. A project recipe can use a custom launcher.

The extension uses supported, tagged callbacks only. Root and subagent identities come from `ctx.agent.kind`, `id`, and `parentId`; factory-local instance IDs distinguish process/extension lifetimes. Conversation ID comes from `ctx.sessionManager.getSessionId()`. Each agent is reported independently, and child reports never replace the root. Heartbeats run every two seconds; a missing process, shutdown, or a report lease older than twelve seconds changes visible activity to `unknown` and makes it unsettled. `alive` reports OS PID liveness independently of logical session shutdown or activity. Timers stop when the last factory is disposed, including a subagent-only factory. Root shutdown does not become successful completion.

- `session_start`, `session_switch`, `session_branch`, and `session_tree`: idle conversation, invalidate prior completed generation.
- `agent_start`: working, advance generation.
- `turn_end`: no settlement change. Tool rounds and maintenance may still follow.
- `session_stop`: working settle-hook pass, not completion; another hook can request continuation.
- `agent_end` with `willContinue`: working continuation, not settled.
- `auto_retry_start`/`auto_retry_end`: retry saga; its end alone is not a root settle.
- `auto_compaction_start`/`auto_compaction_end`: active work or continuation. Idle maintenance restores an already completed generation only if no continuation is scheduled.
- `tool_approval_requested`/`tool_approval_resolved`: needs-input while any approval remains outstanding.
- Interactive `ask` `tool_execution_start`/`tool_execution_end`: needs-input while questions remain outstanding. Headless ask does not claim an interactive question.
- Terminal `agent_end` with no continuation, retry, or outstanding input: idle with explicit completed generation. Only this is a new agent-settled observation.
- `session_shutdown`: unknown and unsettled.

Reports use API v1 `report`, `event.kind: agent`, `instance_id`, `agent_id`, `agent_kind`, `parent_id`, `conversation_id`, `pid`, `activity`, `reason`, `generation`, `completed_generation`, `retrying`, `settled`, `shutdown`, `omp_version`, and `lifecycle_event` (native event name or `heartbeat`). Cursor-based completion waits require `lifecycle_event: agent_end`; an idle heartbeat or restored idle maintenance state is not a new finish. Shell reports use `shell-context`, `shell-ready` (including partial input and prompt generation), `shell-busy`, `shell-lost`, and execution acceptance/rejection/completion events. The Rust module owns process/lease validation and prevents a mismatched generation or pending retry from satisfying settled.

## Source evidence and regression checks

The API was read from tagged source, not inferred from UI appearance:

- [ExtensionAPI, identity, approval and tool events](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.8.7/packages/coding-agent/src/extensibility/extensions/types.ts)
- [Shared lifecycle events, `agent_end.willContinue`, session-stop and retry contracts](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.8.7/packages/coding-agent/src/extensibility/shared-events.ts)
- [Final agent-end notification after retries, async wakeups and session-stop hooks](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.8.7/packages/coding-agent/src/session/agent-session.ts)
- [Retry saga closure](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.8.7/packages/coding-agent/src/session/turn-recovery.ts)
- [Native approval requested/resolved around the prompt](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.8.7/packages/coding-agent/src/extensibility/extensions/wrapper.ts)
- [Interactive ask execution](https://raw.githubusercontent.com/can1357/oh-my-pi/v18.8.7/packages/coding-agent/src/tools/ask.ts)
- [Zsh native descriptor widgets and primary/continuation contexts](https://zsh.sourceforge.io/Doc/Release/Zsh-Line-Editor.html)

Regression coverage is in `tests/integration.rs`, `tests/acceptance.rs`, internal Rust lease/generation tests, and `integrations/omp/lifecycle.test.ts`. Real-Zsh regressions use native PTYs, the actual helper binary, FIFO and JSON transport. They cover startup sourcing, export removal, partial-input protection, foreground busy rejection, explicit queues, source semantics and adapter loss without breaking native input. Lifecycle tests cover turn versus settle, retry, continuation, approvals/questions, session transitions, shutdown, nested identity separation and compaction. Actual manual/action OMP 18.8.7 launches and working/question/settled transitions were exercised on macOS; see verification records. Linux runtime validation remains outstanding.
