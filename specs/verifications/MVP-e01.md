# amux MVP verification

## Mechanical verdict

Final single contiguous run: `cargo test --test shell_races concurrent_ancestor_removal_keeps_every_surviving_parent_valid -- --nocapture && cargo fmt && cargo test -- --nocapture && cargo clippy --all-targets -- -D warnings && cargo build --release && bun test integrations/omp && zsh -n integrations/zsh/adapter.zsh` exited 0. Exact stdout/stderr: [preflight-e01.log](preflight-e01.log), captured from artifact 185 without combining runs. **56 Rust tests passed**, one subprocess-only helper intentionally ignored; **8 Bun tests / 49 assertions passed**. Clippy denied warnings; release and Zsh syntax passed. Cargo test output's condensed duration is not an end-to-end timing claim.

Failing runs are separate: artifact 158 captured private wake bytes entering a foreground raw application; artifact 181 reproduced a dangling parent during concurrent ancestor removal. The competing immediate/queued source regression initially returned integration-lost after a five-second acknowledgement timeout. Targeted green runs and the final full suite cover the corrected input epoch/gate handshake, pending-submission rejection and atomic current-parent removal. No output from a failing run is included in the passing verdict.

## Native vertical slice

Agent-operated CLI and real PTY/TUI actions on macOS arm64; not a claim of human sign-off. The ten-step slice was exercised in initial and follow-up sessions; supplementary final smoke used release binaries from artifacts 166 and 185. The last revision changed atomic tree removal, covered by real-daemon concurrent-removal tests plus a rebuilt-release cold start.

| Required path | Observed result |
| --- | --- |
| Host Zsh | Stock Zsh 5.9 opened with native prompt/startup hooks; captured cwd and exports. |
| Navigate into Git repository | Initial smoke confirmed cwd changes; project context became repository root while actual cwd remained its subdirectory. Final smoke started in `src/` and again resolved root correctly. |
| Child from context | Confirmed cwd/export inheritance; child parent was main. A child captured from another child still defaulted to main. Busy source capture was explicitly stale. |
| OMP manual and picker/action launch | Actual OMP 18.8.7 started both with native UI. Final sessions had different conversation IDs; extension reports discovered both. |
| Agent activity | Actual prompt transitioned working → needs-input with Alpha/Beta question → matching root `agent_end`, generation 1, settled. CLI Enter answered Alpha; CLI screen contained DONE. Settled OMP still rejected shell-source as busy in 3.04 ms. |
| CLI screen and input | Real CLI read native OMP screens and sent text; direct API Enter answered its native question. Screen/emulator remained daemon-owned without an attached client. |
| OMP exit/readiness | Ctrl-D exited both actual OMP instances; parent Zsh returned to confirmed prompt. Earlier actual Ctrl-C source interruption recorded status 130. |
| Reparent | Actual action terminal was reparented beneath another terminal with PID unchanged; no directory or process restart. Concurrent removal regression checks valid/cycle-free ancestry and subsequent API reparenting. |
| Detach/reattach | TUI detached with exit 0; four native shell PIDs survived, and another actual TUI reattached to the existing foreground shell. |
| Shutdown | Acknowledged shutdown exited 0; tracked root PIDs were absent and socket removed. Rebuilt release cold restart preserved saved project ID but created fresh terminal ID; source execution and CLI screen worked, then shutdown again exited 0. |

Earlier full-wrapper native probes exercised application cursor keys, Unicode wide/combining characters, UTF-8 mouse coordinates, bracketed paste, focus reporting, literal prefix, alternate-screen restoration, synchronized output, scrolling, selection and rapid resize. Visible geometry alone followed resize; picker opening did not resize background terminals. Native probe bytes included application-cursor `ESC O A`, bracketed-paste delimiters, UTF-8 mouse coordinates, focus and literal Ctrl-B. Palette update/query tests run a real raw foreground application through the PTY and compare query replies with displayed colors.

Final actual TUI also stayed open after input to a retained exited terminal, showing `exited`. A controlling-PTY harness delivered a 65,549-byte bracketed-paste frame containing 65,537 text bytes; `invalid-input: input exceeds 64 KiB` appeared and the TUI remained alive. The harness initially blocked because its own output reader was not draining full native redraws; interleaved output draining corrected the harness, not the product.

## Responsiveness and bounds

Thirty **real CLI** text-input → native Zsh screen-echo samples while a different terminal continuously printed 8,192-character lines. Includes CLI process startup and screen-read polling; not command completion latency or a cross-platform guarantee. No predeclared numeric SLA exists.

| Statistic | Milliseconds |
| --- | ---: |
| Median | 30.82 |
| P95 (29th sorted sample of 30) | 46.08 |
| Maximum | 77.72 |

Noisy terminal reached exactly the 50,000-row scrollback bound. Existing semantic tests cover slow/disconnected subscribers, event gaps, races, hook failures/timeouts, export removals, partial input, Git worktrees/upstream/counts, foreground and TERM-ignoring separate-session descendants, integration loss, agent crashes, retries, interruptions, nested/root isolation and explicit completion waits. An old automation cursor in the long smoke returned explicit `stream-gap`; taking a new snapshot/current-level query and a fresh completion cursor observed subsequent settlement.

## Supported and unverified

Observed: macOS arm64, Rust/Cargo 1.95.0, Zsh 5.9, OMP 18.8.7, Bun 1.4.2, alacritty_terminal 0.26.0, portable-pty 0.9.0. Linux-specific code exists; attempted Docker validation failed before any test/container command ran, both on Rust layer extraction and a cached-image alternative, with VM filesystem I/O errors. Details: [environment record](ENVIRONMENT-e01.md). Declared Rust 1.85 minimum is unverified. Other OMP versions are unverified. No CI, dependency scanner, coverage percentage or human UAT approval is claimed.

## Review gate

[Independent review record](REVIEW-e01.md): final pair found the same concurrent-removal race; reproduced and fixed after their reports. Mechanical/runtime verification passes. **Review cap exhausted (5/5). Human decision required.** No clean independent re-review, merge, release tag or deployment is claimed.