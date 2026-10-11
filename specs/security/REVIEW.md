# amux local-control security review

Scope: greenfield feature branch `feat/amux-mvp`, implementation added after the initial design commit. Static input-to-sink review plus observed permission/behavior tests; no dependency-vulnerability scanner or Linux execution claimed.

- Boundary: local Unix socket; `src/client.rs` verifies a private current-user-owned directory. `src/daemon.rs` takes the runtime lock and creates a 0600 socket. No network listener. Same-user callers intentionally have execution authority; this is not a sandbox or per-project ACL.
- Shell source: API source is intentional code execution, stored in 0600 files under a 0700 integration directory (`src/integration.rs`). FIFO frames contain validated UUID/queue/generation fields, not source bytes. Native ZLE prompt, BUFFER, generation and typeahead guards reject foreground-app execution and partial-line replacement (`integrations/zsh/adapter.zsh`); input epochs/gate serialize private wake delivery. Source paths are Zsh-quoted before native top-level evaluation.
- Hooks: user-configured `/bin/sh -c` is intentional execution, not an injection vulnerability. Project configuration uses private atomic replacement (`src/projects.rs`), validation precedes mutation, hook cwd/environment/timeout/output limits are explicit.
- Resource boundaries: bounded API frames, PTY input queues, event retention, Git work, hook diagnostics and agent/execution histories. Slow control sockets are disconnected on shutdown, and their RAII ownership keeps late terminal cleanup alive until completion.
- Process signals: kernel birth identity is checked (`src/runtime.rs`); native tests cover PID mismatch and foreground/separate-session descendants. OS permission changes, uninterruptible I/O and already unobserved orphaning remain explicit limitations, not a sandbox guarantee.
- Secrets: captured exported environments and shell-context events remain local, same-user data. Do not publish unfiltered control-event logs: exports may contain credentials. Clipboard reads are denied; host clipboard writes require client selection.

No confidence >=8 HIGH security finding established by this static boundary review. This is not an independent security certification. Fresh independent code-review gate is tracked separately in `specs/state.yaml`.
