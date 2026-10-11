---
type: verification-checkpoint
context: e01 local daily use; runtime baseline 5c1e2c6
---
# Daily-use stopping point

## Decision

Pause on `feat/amux-mvp` for local macOS daily use. Runtime source remains `5c1e2c6`; this cleanup changes documentation and removes approved disposable artifacts only. No merge, release, daemon replacement or global installation was performed.

## Cleanup

- User approved closing Herdr workspace `w11` (idle `amux-latency` and `reviewr` pane), removing the clean `.worktrees/input-latency` checkout and deleting the untracked zero-byte root `scripts` file. Completed all three without force.
- Retained `perf/input-latency` at `de14ddc`, its integrated changes, historical verification evidence and the working release binary/build cache. Build cache was approximately 1.5 GB; retaining it avoids a cold rebuild and does not enter Git.
- Private installation, config, startup files, test Git repository, PTY readers and all smoke processes were removed after explicit shutdown. User startup/configuration, installed binaries, main Herdr workspace and live/default amux daemon were untouched.
- README now explains daily detach/reattach, saving/reopening projects, destructive shutdown, updating and diagnostic locations. Corrected the changelog's stale Rust-minimum reference to 1.86. State and CLAUDE.md point to this stopping checkpoint; historical worktree fields are explicitly marked retired.

## Final mechanical verdict

One contiguous terminal command exited **0**:

```sh
cargo fmt --check && cargo test -- --nocapture && cargo clippy --all-targets -- -D warnings && cargo build --release && bun test integrations/omp && zsh -n integrations/zsh/adapter.zsh
```

| Check | Observed result |
|---|---|
| Rust tests | 66 passed; one intentionally ignored helper |
| Formatting / Clippy | Passed; warnings denied |
| Release build | Passed |
| OMP lifecycle tests | 8 passed; 49 assertions |
| Stock-Zsh adapter syntax | Passed |

[Exact contiguous output](daily-use-preflight.log). The tool's aggregated `0.00s` test-duration summary is inaccurate; use the actual per-suite timings in that log.

## Installed-binary daily flow

`cargo install --path . --locked --root <owned-private-directory>` succeeded. Executed the installed binary **outside the repository**, proving the packaged Zsh/OMP integration sources are embedded rather than requiring the checkout. [Installation output](daily-use-install.log), [separately labeled runtime observations](daily-use-runtime.log).

1. Cold actual 120×40 controlling-PTY TUI invocation auto-started its own daemon and native stock Zsh; rendered `DAILY_NATIVE>` and confirmed readiness. No socket proxy or fake terminal.
2. Native `cd` entered a nested Git directory. Main-shell actual cwd stayed nested while project context followed repository root. Saved the project and launched a default child from confirmed cwd/exported context; child retained `DAILY_SMOKE_EXPORT=preserved`.
3. Actual prefix/Ctrl-Q detached with status 0. Both shell processes survived; actual reattach rendered the native prompt. Promote/reparent preserved child PID and cwd. Daemon-start hook count remained unchanged.
4. Partial native input returned `partial-input` for source execution without replacing the line. Native line clearing restored readiness. A foreground job returned `busy`, not a false prompt-ready acceptance.
5. Child close terminated its shell and tracked sleeping descendant. Exit status remained available, then explicit removal discarded the record.
6. Startup hook ran first. A project's first hook deliberately exited 7 with visible `DAILY_HOOK_FAILURE`; its second hook ran afterward in project cwd. The shell remained confirmed-ready. Another detach/reattach ran neither hook category.
7. Persisted config was private (0600), retained stable project identity and contained configuration/projects rather than runtime processes/screens. Shutdown during a managed job terminated the daemon, main shell, sleeping descendant and hook-project shell; socket disappeared.
8. Started a fresh daemon from the same config: project identity survived, old terminal IDs/processes did not, daemon-start hook ran once again. Project-open created a fresh native shell at the saved repository-root context. Final shutdown exited 0; all tracked shell PIDs and socket were absent.

The first navigation probe compared Zsh's logical `/var/...` cwd with its canonical `/private/var/...` alias and timed out. Confirmed native state was correct; corrected the probe to compare directory identity. No product code was modified or failure hidden.

## Remaining boundaries

- Local macOS arm64 runtime is verified. Linux execution, declared Rust 1.86 minimum and OMP versions other than 18.8.7 remain unverified.
- Native OMP manual/action lifecycle and full UI/mode evidence remain in [MVP](MVP-e01.md), [Ratatui UI](UI-ratatui.md), [empty-window recovery](UI-empty-switcher.md) and [latest input measurements](immediate-input.md). Those broader scenarios were not rerun during this documentation-only cleanup.
- Warm native input measurements remain 2–7 ms; cold full-refresh outliers around 130 ms and unmeasured physical-display latency remain explicit. Persistent transport can disconnect a client stalled for five seconds; processes remain available to reattach.
- No runtime/crash/process restoration. Shutdown/reboot loses live shell state and screen history. Saved projects reopen as new shells; descendant-cleanup OS limits remain documented in README.
- Independent review cap remains exhausted (5/5); there is no post-fix independent PASS. Do not interpret local daily-use approval as merge/release approval or reset the cap. No new coverage/security-release gate claim.

## Resume / verify

For daily use, follow [README daily operation](../../README.md#daily-operation). To reinstall this checkout deliberately: `cargo install --path . --locked --force`, then `amux` or `amux attach`. A compatible first-pass daemon does not need another restart for the client-only optimization.

For future development, read `CONVENTIONS.md`, `specs/state.yaml` and architecture before editing. Keep the current feature branch and resolve the existing human review/release decision before landing.

`verify:` rerun the contiguous mechanical command above; reproduce the private installed-binary flow only against a fresh owned socket/config, never by shutting down the user's live daemon.
