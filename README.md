# amux

A project-oriented native terminal wrapper: a Rust daemon owns Zsh terminals and emulator state; a detachable TUI shows one terminal; the CLI controls the same local API. Shell editing and OMP remain their native interfaces.

## Install and start

Tested with Rust/Cargo **1.95.0**, stock **Zsh 5.9**, **OMP 18.8.7**, and macOS arm64. Linux has implementation paths, but attempted container validation was blocked by Docker VM filesystem I/O errors before any test ran. Ratatui 0.30.0 uses the existing Crossterm 0.29 backend; the crate now declares Rust 1.86, matching Ratatui's minimum. That minimum toolchain is not independently verified. Requires stock `/bin/zsh`. OMP is optional until launched.

```sh
cargo install --path . --locked
amux
```

The first invocation starts a background daemon and an unsaved project with a host shell. Its context follows the main shell's confirmed cwd, or the Git repository root, without changing the actual cwd. Other terminals keep their own locations.

```sh
amux snapshot
amux open --source main --name build
amux attach build
amux open --source main --omp --name agent
```

`amux attach` reattaches without restarting processes or running hooks. `amux daemon` runs in the foreground for supervision. `--socket PATH` and `AMUX_SOCKET` select a daemon; its directory must be private and owned by the current user.

## Picker and native interaction

The Ratatui interface has a **three-row title/tab bar**, a **Switch** button at the top right, and an edge-to-edge native terminal below it—no footer. Default prefix: **Ctrl-B**. Click Switch or press the prefix to replace the terminal view with a **full-screen window list**. Hierarchical rows show project, Git/agent/exit status and directory; fuzzy search covers terminal name, project, actual cwd and Git branch/commit. Select either line of a row with Enter or a mouse click to open it directly; Return or Escape restores the terminal.

| Picker action | Key |
|---|---|
| Select / open | Up/Down, Enter |
| New shell from current captured context | Ctrl-N |
| New terminal running OMP | Ctrl-O |
| Promote | Ctrl-P |
| Reparent / demote: choose a parent row next | Ctrl-R / Ctrl-D |
| Close running terminal / remove exited window | Ctrl-X |
| Detach this TUI | Ctrl-Q |
| Cancel picker | Escape |
| Send literal prefix to the application | Prefix twice |

When a terminal exits, its view shows the actual exit code and **Close window** / **New terminal** buttons. **X** removes its exited record; **N** or **Enter** starts a new terminal from the retained captured context, leaving the old record available until closed. Removing the last window shows explicit **New terminal** and **Exit** actions: click New or use **N**, **Enter**, or **Ctrl-N** to start a shell; click either Exit button or use **Escape**, **Q**, or **Ctrl-Q** to return to your host shell. **Exit detaches the UI; the daemon keeps running.** Use `amux shutdown` to stop it. With windows present, ordinary Q remains fuzzy-search input and Ctrl-Q detaches. The list's mouse wheel moves its selection. Exit/empty-card buttons stack vertically on narrow displays.
Native Ratatui surface/action evidence: [UI verification](specs/verifications/UI-ratatui.md), [empty-window navigation](specs/verifications/UI-empty-switcher.md).

Mouse wheel and Shift-PageUp/PageDown scroll locally; typing returns to the live surface. Shift-drag selects text even when the foreground application requests mouse reporting; selection copies through the host terminal's OSC 52 support. Managed-application clipboard requests are not forwarded; client selection is the supported clipboard path. Focus, scrolling, selection and picker state belong to this client. Background terminals retain their last size, initially 80×24; opening a picker does not resize them. Only the visible surface follows client resize. If multiple local TUIs show the same terminal, the last attachment/resize determines its PTY geometry.

```sh
amux config '{"prefix":"ctrl-a","projects_dir":"/home/me/projects"}'
amux detach       # request all currently attached TUIs to detach; processes remain running
amux shutdown     # deliberately terminate managed processes and stop the daemon
```

Use Ctrl-Q in the picker to detach only your current TUI. External detach is asynchronous (observed by client snapshot polling). Reattachment starts after the latest detach generation.

## Input responsiveness

The TUI reuses its local connection and requests changed native rows instead of a whole grid for each keystroke. Actual release-TUI PTY measurements on macOS: typical120×40 shell echo median10.81ms, large500×163 median30.34ms; noisy-background raw echo at500×163 median30.39ms. See [full-wrapper measurements and observer caveats](specs/verifications/input-latency-final.md). These measure emitted terminal rendering, not physical monitor paint. Full-damage frames still cost more. Persistent clients expire after five seconds without a request: a paused or output-blocked TUI may disconnect, but its managed processes remain available for `amux attach`.

Advanced version1 API clients may opt in with `persistent:true`. For live `screen` requests, `row_updates:true` plus `since` can return changed-row deltas with complete metadata; retain the same viewport/base generation and apply `row_indices` to `cells`/`lines`. Viewport changes require a full read. Legacy CLI/extension callers continue receiving ordinary full frames/one-shot EOF.

Run daemon and TUI from the same rebuilt binary. An already-running pre-change daemon must be restarted to use this runtime/API revision; `amux shutdown` terminates its managed processes, so finish valuable work before doing that. Verification used private daemons and did not restart or replace your live daemon.

## Shell execution versus typing

```sh
amux input agent 'a message for OMP'
amux input agent --key enter
amux input agent --key ctrl-c
amux input agent --paste 'multiline text'
amux input main --bytes '[27,91,65]'

amux execute main 'export MODE=debug; cd src; print $PWD'
amux execute main 'print later' --queue
```

Input goes to the current foreground application. `execute` is shell **source**, not typed text: only an empty confirmed primary Zsh prompt can accept it. Busy applications, earlier pending submissions and partially entered lines are rejected; queueing is explicit. The adapter rechecks native prompt, buffer, generation and typeahead, with an input-epoch handshake that prevents a queued wake key from following user input into a foreground application. Ctrl-C records interrupted source completion without replacing native hooks. Exited/backpressured input and oversized paste display an error without closing the TUI.

New terminals default to children of the project's main terminal, even when captured from another terminal. They inherit captured cwd and exported environment, including export removals. While the source shell is busy, launch results report a **stale** capture. Aliases, functions, local variables, job tables and shell options are not cloned. A lost adapter disables structured execution; native raw interaction remains available. Details: [integration contracts](integrations/README.md).

Manual `omp` inside a managed shell loads the reporting extension. Existing `omp` functions are delegated to with the extension argument; existing aliases remain intact and must forward `-e "$AMUX_OMP_EXTENSION"`. The picker/CLI action uses `command omp -e "$AMUX_OMP_EXTENSION"` directly to avoid alias/function interception or duplicate extension loading. Use a project recipe for a customized launcher. Other OMP releases are not claimed compatible.

## Projects, recipes and hooks

```sh
amux project-save compiler
amux project-new scratch --config '{"hooks":[{"source":"git init; printf setup-complete","timeout_ms":10000}],"recipes":{"agent":"omp","check":"cargo check"}}'
amux project-config scratch '{"recipes":{"agent":"omp","check":"cargo check"}}'
amux open --project scratch --recipe check --name checks
amux project-open scratch
amux config '{"daemon_start_hooks":[{"source":"printf daemon-started","timeout_ms":1000}]}'
```

New directories default to `<projects_dir>/<name>`; use `project-new NAME --directory PATH` to choose a location. Initial `--config` accepts hooks and recipes and is validated before project creation. Saved projects keep stable IDs. Save resolves the latest confirmed main-shell cwd/repository root before persistence; explicit re-save updates that location. Configuration patches replace the supplied hooks array or recipes map; they do not merge individual entries.

Hooks run as noninteractive `/bin/sh -c` workers, in a single queue: daemon-start hooks first, then project-open jobs, each hook in configuration order. Daemon hooks use daemon startup cwd; project hooks use project context. Both inherit daemon exports, not mutable interpreter internals. Default timeout is 30 seconds, configurable from 1 ms to 300 seconds. Stdout/stderr and failures are visible in snapshot diagnostics/events; a failed hook leaves a usable shell and later hooks continue. Reattachment runs neither hook category. Opening an already represented project is reattachment; close and remove its terminal records before a fresh project-open, or reopen after daemon restart. Shutdown skips pending hooks and waits for the active hook's bounded timeout.

Disk contains only global/project configuration and hooks under `$XDG_CONFIG_HOME/amux/config.json` (or `~/.config/amux/config.json`). No PTYs, tree, processes, screen, shell state or conversations are restored.

## Automation

API commands print versioned JSON envelopes and return nonzero on errors. UUIDs are stable within daemon lifetime; saved project UUIDs persist. Names are selectors, and ambiguity is an error.

```sh
amux state agent
amux screen agent
amux screen main --offset 100 --limit 40
amux name build compiler-build
amux reparent agent compiler-build
amux promote agent
amux close agent
amux wait agent exited
amux remove agent
```

An agent is **settled** only after an explicit root `agent_end` with no continuation, retry or pending question/approval, and a matching completed generation. Turn completion, initial idle, process exit and shell readiness are different states. A crash or expired reporting lease is unknown, never successful completion. Child agents cannot overwrite root status. Settled means the reported agent cycle ended, not that its task succeeded or its changes are correct.

```sh
amux snapshot                                  # retain its result.seq
amux events --after 123 --timeout-ms 30000      # replay / long poll
amux wait agent agent-working --after 123
amux wait agent needs-input --after 123
amux wait agent agent-settled --after 123
amux wait main shell-ready
amux api '{"version":1,"op":"screen","terminal":"main","offset":0,"limit":24}'
```

Take a snapshot **before** sending work; chain waits using returned sequences. With `--after`, waits observe retained transitions even if later state has changed; inspect `observed` separately from current `state`. Completion waits require a new explicit root `agent_end`; idle heartbeats and maintenance do not count. Without `--after`, a wait is a current-state query. An event gap includes a replacement snapshot: resume from that snapshot's `seq`. A wait gap is an explicit error requiring a new snapshot. Active waits consume unmatched events as they go. Structured events contain no high-volume PTY byte stream.

Shell-context events may contain exported credentials. Treat the local control API and unfiltered event logs as privileged same-user data; do not publish them.

## Validation and limits

```sh
cargo test -- --nocapture
cargo clippy --all-targets -- -D warnings
cargo build --release
bun test integrations/omp
zsh -n integrations/zsh/adapter.zsh
```

[Architecture and API ownership](specs/tech-architecture/DESIGN_PLAN_LATEST.md), [implementation plan](specs/epics/e01-native-terminal/epic.yaml), and [verification evidence](specs/verifications/) record the installed bigpowers workflow and observations.

Final integrated verification: **66 Rust tests and 8 OMP tests pass**, plus Clippy, release build and Zsh syntax; [latency/UI evidence](specs/verifications/input-latency-final.md) includes900 actual TUI keystroke samples, native input modes, noisy scrollback, real Herdr navigation/detach and clean shutdown. Original native OMP/manual/action and lifecycle evidence remains in [MVP evidence](specs/verifications/MVP-e01.md). **Review cap exhausted (5/5). Human decision required.** The final tree-removal finding was reproduced and fixed; no post-fix independent review PASS or merge to the default branch is claimed.

Bounded MVP resources: 50,000 scrollback rows per terminal; 64 KiB per raw input request with 128 queued writes; API request 1 MiB / response 64 MiB; PTY geometry 2–500 columns and 2–300 rows; event history 2,048 entries / 4 MiB; Git cache 256 workspaces with 100 file details per change category (full counts retained); 128 active agent identities per terminal; 256 managed execution records, pruning completed/rejected records first and exposing `executions_discarded`. Git overflow and pending-execution capacity produce explicit unavailable/busy results. Diagnostics retain the latest 20 hooks, with 64 KiB visible per output channel and truncation marked.

No crash recovery, interpreter cloning, tiling, remote protocol or nonterminal clients. Synchronized updates, native mouse/paste, Unicode, alternate screens, scrolling and resize were exercised on macOS; no Linux runtime claim. Host fonts and terminal clipboard facilities affect presentation. Process cleanup uses kernel birth identities and freeze/rescan of known descendants, including foreground and separate-session jobs. Shutdown disconnects slow control clients and drains in-flight creation/cleanup before exit. Processes already orphaned outside observed ancestry/session cannot be guaranteed captured. OS permission changes or uninterruptible kernel I/O can prevent termination; freeze waits are bounded and emit diagnostics to the daemon log before best-effort signals. This is a same-user control environment, not a sandbox.
