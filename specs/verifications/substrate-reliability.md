---
type: verification
context: amux terminal compatibility and execution-substrate reliability
---

# Substrate reliability verification

Date: 2026-10-19. Platform: macOS arm64. Branch: `fix/substrate-defects`, based on `af21287`; verification preceded the user-authorized commit and daemon shutdown recorded below. The eight defects in [the bug record](../bugs/BUG-substrate-reliability.md) meet their scoped acceptance criteria. This is not an independent review PASS, a release waiver, or a claim of exhaustive compatibility.

## Scope and ownership

amux remains an execution substrate. No autonomous scheduler, task ledger, automatic answers/merges, ACP adapter, worktree provider framework, container backend or infer integration was added. Existing user changes and root artifacts were preserved. No live/default daemon, installed binary, Herdr session, main branch or remote was replaced.

- `src/runtime/compatibility.rs`: parsed ANSI Handler adapter; complete forwarding of the pinned vte Handler surface, DECOM-relative CPR, bounded final-grapheme composition and native narrow/wide cell layout. Width changes replay only the final cluster, including insert/scroll/overwrite edge cases. SGR/metadata and damage publication remain intact; ordinary ASCII avoids segmentation and cell snapshots except potential keycap starters.
- `src/runtime.rs`: adapter integration and ownership of an uninstalled PTY child until normal wait-thread handoff; setup errors kill/reap owned processes rather than leaking them.
- `src/projects.rs`: defaulted `terminal_limit`; canonical config identity, private stable nonblocking lock, exact loaded-byte comparison, private-file sync/rename/directory sync; cancellation-aware hook process groups. All hook callers migrated, with no compatibility shim.
- `src/daemon.rs`: atomic admission reservation before preparation/spawn; rollback on every failed path; retained exits count until removal; centralized bounded diagnostics; startup socket cleanup and active-hook stop token.
- `src/client.rs`: serialized autostart and an absolute-deadline read-only API-v1 snapshot handshake for existing and new daemons. Existing bad peers are never replaced merely because probing fails.
- `src/integration.rs`: failed preparation removes partial owned resources even before registration; invalid IDs cannot traverse outside the integration directory.
- Regressions cover direct native cells, real PTYs, daemon/API concurrency and independent strict showcase expectations. The explorer's oracle was not changed.

Decision anchor: the existing `af21287` terminal-showcase baseline plus the user's execution-substrate boundary recorded in `specs/state.yaml` and the [assessment](readiness-orchestration-assessment.md).

## Mechanical checks

| Command | Observed result |
|---|---|
| Baseline `cargo test --workspace` | 177 passed, 4 ignored |
| Final `cargo test --workspace` | 210 passed, 5 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo build --workspace` | Passed |
| `cargo build --workspace --release` | Passed |
| `cargo test -p terminal-showcase --test multiplexer -- --ignored --nocapture` | 1 passed; strict endpoint compatibility required |
| `bun test integrations/omp` | 8 passed, 49 assertions |
| `zsh -n integrations/zsh/adapter.zsh` | Passed |
| `cargo fmt --all` | Completed before the final gate |

The ignored entries are four subprocess fixtures exercised by their parent regressions and the independent multiplexer integration run explicitly above. The final gate covered all 16 acceptance tests, 6 readiness tests, 5 integration tests and existing shell-race/process-cleanup paths. No Linux, minimum-Rust, coverage-percentage or latency-benchmark claim is made.

## Independent terminal behavior

The real-daemon showcase now requires every exercise to pass, unchanged origin/ZWJ expectations, both capability statuses to be objective Verified, and strict exit status 0. This replaces the former acceptance of preserved objective failures; it does not weaken the oracle.

A separate throwaway Python foreground application imported no amux code, opened `/dev/tty`, used raw mode, emitted ANSI/UTF-8 and checked actual CPR replies. At 20×12, all 13 cases passed and the application exited 0:

- Origin-relative margins and transition back to origin-disabled reporting.
- Bytewise streamed woman-technologist ZWJ, family, regional-indicator pair, emoji modifier and keycap.
- CJK with combining mark, ASCII combining, and overwrite of a native wide-cell pair.
- Joined emoji at the right edge, variation-selector widening from the final column, and explicit cursor motion terminating composition.

Origin reported `[2,4]`; joined emoji plus X reported `[7,4]`, matching the original independent expectations. Direct regressions additionally cover every UTF-8 chunk boundary, alternate/synchronized screens, capped history, provisional scroll reversal, insert-mode replay, width shrink and no-wrap refusal.

The compatibility working string is capped at 4096 UTF-8 bytes. Crossing that boundary starts a new native scalar composition segment without discarding text; joining across that oversized boundary is deliberately not preserved. Exact-limit, overflow-wide-scalar and retained-combining-text cases pass. Native cell extension storage keeps upstream variable-size semantics: this is not an output-byte quota.

## Actual CLI/API/TUI smoke

All scenarios used freshly built binaries and private runtime/config/startup directories.

| Scenario | Observed result |
|---|---|
| Two daemons sharing one config | Stale global change and stale project-save returned `config-conflict`; the first daemon's saved project and prefix survived; failed mutations rolled back |
| Partial saved-project reload | Opening the newer saved project did not rebase stale global state; a later stale save still conflicted |
| Held external cooperating config lock | `config-busy` plus a responsive snapshot in 12.15 ms; rejected prefix was not applied; save succeeded after unlock |
| Fresh third daemon | Loaded the latest prefix and the original stable saved-project ID; the older daemon's shell PID remained unchanged |
| Terminal limit 2, one existing terminal, 16 simultaneous opens | Exactly 1 admitted and 15 `terminal-limit` errors |
| Failed/retained opens | Invalid-directory attempts refunded capacity; an exited retained record still blocked admission; removal freed capacity; lowering the limit did not evict records |
| 25 long multibyte launch failures | Latest 20 retained, failures 05–24 in order; maximum command length 16,384 UTF-8 bytes, truncation marked; event sequence remained increasing |
| Active hook with 300-second timeout and sleeping descendant | Shutdown completed in 448.94 ms, daemon exit 0; hook and descendant absent; later hook never ran |
| Release CLI attached to inert listener | Exit 1 in 2007.02 ms with `daemon-readiness`; only a read-only snapshot request; peer socket preserved and no daemon log/spawn |
| Release TUI on a real controlling PTY | Native prompt and joined Unicode emitted; typed command wrote its independent marker; detach/reattach exited 0 twice and preserved the managed shell PID |

Before-change private probes recorded an existing inert listener hanging the old attach command beyond a 5-second subprocess bound, and shutdown waiting 3.994 seconds for an active `sleep 4` hook rather than cancelling it. These were not re-run merely to reconfirm accepted failures.

The release-TUI smoke observed actual emitted surface bytes and managed-shell state, not a mocked renderer. It does not certify physical compositor paint, font shaping or host glyph appearance.

## Verification observations retained

1. The first integration compile found a missing `anyhow!` macro import in the parent's error-chain change. The call was fully qualified; the final gate compiled and passed.
2. The first full suite timed out in existing `host_context_child_partial_busy_tree_detach_and_shutdown` while waiting for queued output. Failure diagnostics were extended to include snapshot and main screen; the instrumented 16-test acceptance suite and final complete workspace suite passed. A separate 24-round real-daemon accepted-command/queue/immediate-Ctrl-C probe completed every queued command, including cancellation before the command-body marker appeared. **The original timeout's cause was not established.** No test was disabled, weakened, given a longer deadline or relabelled as a product fix. This remains a reliability observation to investigate if it recurs, not proof that an intermittent race cannot exist.
3. An ad hoc smoke attempted remove immediately after close and received the documented `alive` error before asynchronous exit publication. The harness was corrected to wait explicitly for `exited` before removal; product behavior was unchanged. The README already shows close → wait exited → remove.

## Self-audit and generalization

- Correctness: reviewed native parser forwarding, width-change rollback/damage, config snapshot identity/rollback, admission reservation transfer/refund and startup/shutdown ownership. Fixed blocking config-lock acquisition during integration so an external writer cannot stall the daemon state mutex indefinitely. Fixed partial integration setup and uninstalled-child cleanup in the same failure class.
- Class sweep: daemon diagnostics have one retention insertion site; all launch/hook/enqueue producers use it. Admission has one reservation increment, one success conversion and one RAII error refund. Both existing/new startup paths use the snapshot probe. Production and regression hook callers pass cancellation; Git worker calls retain their non-cancellable contract.
- Security: reviewed same-user trust, private lock/temp-file handling, symlink/hard-link conflict identity, validated cleanup IDs, owned process groups, bounded text before event cloning, and no-replay readiness. Secret-pattern scan of touched code/tests and the new bug record found no matches. No privilege-boundary expansion or HIGH finding was identified; this is a focused self-audit, not an independent security review.
- Supply chain: `unicode-segmentation` became a direct dependency using already-locked 1.13.3; existing `unicode-width` is reused. No emulator fork or competing parser was introduced. Optional `slopcheck` was not installed (`command -v` exited 1); repository churn/review helper scripts are absent. No scanner, numeric score or helper-run result is fabricated.
- Maintainability: compatibility code is isolated from daemon policy; shared config and diagnostic conventions are centralized; obsolete conditional showcase-failure acceptance was removed. Existing large Rust modules were not split solely to satisfy generic file/function-length heuristics. Public README/changelog updates follow existing repository documentation practice; new investigation/evidence files are under `specs/`.

## Remaining limits and handoff

- macOS arm64 only. Linux runtime, minimum Rust version and physical font/glyph presentation remain unverified.
- Successful saves exercise file and containing-directory sync, but power loss, filesystem failures after rename and real crash durability were not fault-injected. Post-rename sync failure preserves the old expected snapshot and exposes reload guidance through `config-save` error text. Non-cooperating editors are not serialized; no live full-config reload exists.
- Terminal admission and diagnostic caps are explicit local bounds, not a complete aggregate byte quota. Upstream per-cell combining storage can grow; processes orphaned beyond observed ancestry, permission changes and uninterruptible kernel I/O remain existing cleanup limits.
- e01's exhausted independent-review cap is preserved. No fresh independent PASS, landing authorization, commit, merge, push, deployment or live-daemon upgrade is implied.
- Finish valuable work before deliberately replacing an old daemon: installing the new binary alone does not retrofit its terminal/config/lifetime behavior. No automatic upgrade was performed.

Next: use the verified branch for controlled product feedback, or scope the next explicit execution/workspace primitive through `survey-context`. Do not infer approval for autonomous orchestration from these fixes.

Cleanup and record integrity: all 8 ad hoc private daemons and 2 TUI clients exited and were reaped; both owned throwaway fixture roots, including probe scripts and temporary binaries, were removed. YAML contracts and 5 local Markdown references validated; all 8 scoped registry entries are fixed. The independent-review round count remains 5.

## User-authorized commit and daemon shutdown

After verification, the user authorized stopping the default daemon and committing the complete reliability work. `target/release/amux shutdown` returned API-v1 success with `shutting_down:true`; a subsequent `pgrep` for amux processes found none. No daemon was restarted or installed binary replaced.

The reliability code, tests, assessment and supporting documentation are included in one Conventional Commit on `fix/substrate-defects`. The feature branch is retained, not merged or pushed; bug-flow state is cleared and the next context handoff is `survey-context`. Pre-existing root scratch files `script` and `terminal-showcase-report.json` remain outside this commit. The Rust library helper `projects::run_hooks` now requires an explicit cancellation flag; CLI/API framing remains version 1.
