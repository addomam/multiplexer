# Changelog

## Unreleased — ACP conversation transport (Build)

- Add explicit owned ACP views, CLI open/inspect/attach and current-control, generation-bound prompt/authenticate/interrupt/close admission using pinned official ACP v1 schema types. Keep native terminal/agent routes and stock Zsh/OMP distinct.
- Retain exact work/RPC/session identities across caller loss and authority transfer. Force never interrupts; cancelled pending claims grant nobody; accepted work is not replayed or discarded by a later owner check.
- Bound frames, reserved/in-flight queues, history, stderr and owned target admission. Keep unknown delivery and process-identity cleanup truthful; unknown cleanup is retained and continues consuming its reservation.
- Render canonical same-instance history and identity-bound local drafts in the actual human TUI. Paste never submits, observers cannot send application input, and revocation clears draft authority and stale controlled status.
- Verify the first transport slice with local deterministic stdio peers, real CLI and controlling-PTY interaction. Real adapter/provider/auth/model compatibility, later interaction stories and complete ACP acceptance are separate gates; historical native-fence failures and review caps remain unchanged.
- Expose ACP ownership's approved `settlement_boundary: input-admission` field consistently in receipts and human control panels. Preserve completed force-warning eligibility across unchanged fully-visible repaints; changed facts, clipping, resize and scrolling still require a new safe presentation.
- Accept the existing persistent transport flag on ACP open. Retain unqueued native-editor drafts under their original authority without replay, keep late process-identity cleanup uncertainty charged after close, and preserve observed authentication outcomes if follow-up setup cannot enqueue. Roll back empty failed-open views without discarding concurrent admitted-close history.
- Preserve the last confirmed ACP draft/transcript across failed inspection reads, surface the failure separately and restart history only for a real target-identity change. Distinguish an unread target from a retained cache; clear superseded local rejection notices on deliberate requeue without claiming agent admission.
- Sanitize remaining bidi/line-format marks in ACP metadata and native chrome; reject them in ACP names. Exercise actual queued-response lifetime ordering rather than claiming a late reply from an already-reaped peer.
- Answer malformed or foreign-session cancel-time permissions with explicit protocol errors without dropping the original prompt or killing its ACP session; valid late permissions retain distinct Cancelled semantics.
- Enable genuine request-/session-scoped ACP forms and exact offered tool permissions on the same owned connection. Retain original bootstrap/auth/prompt scope and typed responder identity through parent completion and wire-ID reuse.
- Add native typed flat controls, editable defaults and explicit unset, two-step review/accept, separate form decline/cancel and deliberate permission allow/reject once/always. Share backend answer validation; expose local pattern/format limitations without claiming general JSON Schema validation.
- Linearize competing API/native responses and reserve explicit-interrupt replies atomically. Keep late applicable forms/permissions nonselectable, losing answers out of accepted history, and written delivery distinct from agent application.
- Align native history with 64 live/responding plus 1024 retained completed records; account for bounded server metadata separately from the 4-MiB canonical content cap. Preserve supported large declared defaults and optional omission while keeping submitted answers within 64 KiB.
- Exercise running-question reserved handoff, noncancelling force, pending cancellation/private recovery, original-attachment conditional consent and all native input/draft fences against the same live ACP peer. Preserve stale-answer rejection and independently observed fresh arithmetic effects after deliberate reacquisition.
- Verify canonical gap/category retention and inert hostile rendering through a real controlling PTY. Keep actual cleanup observations separate from fixture fallback; pin the native test host until post-TUI termios observation and deliberately type fresh answers rather than asserting incidental defaults.
- Exercise the complete local ACP broker with same-session API/native questions and allow/deny, exact reused-wire responders, owned descendants, stalled input, hostile frames and concurrent close/shutdown. Keep genuine real-adapter acceptance separate; terminal-only settlement remains best-effort and unknown blocks safe grants.
- Cancel and join owned ACP pipe workers independently of peer EOF, including a reparented different-session process retaining stdin/stdout/stderr. Keep observed retained endpoints cleanup-failed and capacity-charged; do not detach workers or signal unknown processes.
- Retain original ACP pipe endpoints through final cleanup classification, even when an idle writer or failed frame reader returns early. Probe endpoint closure independently of workers and keep unresolved targets charged; check cancellation before I/O without rewriting completed byte counts.
- Project genuine Grok `_x.ai/ask_user_question` into the existing bounded form ledger, including nullable single-choice flags, multiselect and Other/preview/notes responses. Preserve exact wire IDs and mandatory cancel replies; reject unsupported decline without consuming the responder or inventing plan chat/skip actions.
- Add opt-in real Codex/Grok OAuth acceptance through the actual daemon and controlling PTY, with a transparent private wire observer, bounded low-effort model use and explicit cleanup. Keep failed attempts and historical native-fence/review boundaries; compatibility is claimed only by the recorded exercised rows.

## Unreleased — e03 ownership and agent interaction (Build)

- Breaking API/CLI cutover: public `input` and `execute` require a current control capability; CLI callers must supply serialized `--control` JSON. Raw client requests never acquire ownership implicitly.
- Preserve daemon rejection codes in concrete `client::DaemonError` values and the CLI JSON envelope instead of parsing diagnostic text.
- Register native attachments once on their original persistent connection, report serial selected-terminal/focus views with initially unknown focus, and cache per-terminal control. Unowned settled targets may be acquired normally; observers and stale controls cannot forward keys, literal prefixes, mouse or separate paste, and failed input is never replayed.
- Migrate acceptance, shell-race and independent-showcase fixtures to explicit external registration/acquisition and cached controls without changing original shell readiness, partial-input, busy or latency assertions.
- Document request-success-not-grant semantics, inspect-after-uncertainty recovery, ordinary pending cancellation granting nobody, and retained work after force/cancel.
- Add native request/owner-response/progress/cancel and deliberate fresh-force panels, plus externally requested idle/background reacquisition. Draw-bound consent, local activity, keep/withdraw and stale offers preserve human control without automatic rearming.
- Integrate a concrete Codex 0.160.0 same-instance broker with exact turn/request identities, retained outcomes, request-only permission decisions and explicit interrupt separate from ownership. Native-first discovery reads the live thread; ephemeral native title helpers do not replace its identity.
- Verify separate live start/steer, genuine question, permission allow/deny and exact-turn interrupt checks. Keep complete-broker acceptance failing on unknown native/application settlement; native winning-answer races and all32 acceptance remain unproved. See `specs/verifications/e03-integration-build.md`; Plan approval and mechanical tests are not acceptance.
- Explicitly unlock completed configuration-writer guards before closing their descriptors so a fork-inherited duplicate cannot keep the writer lock alive.
- Subscribe explicitly to the native-created Codex thread after its first requested turn, recovering authoritative completion from its paginated initial turn snapshot. No hidden bootstrap turn or API mutation replay.
- Keep Codex request-only denial distinct from its native menu: supported `decline` does not become the interrupting `cancel`; permission allow remains constrained to the request.
- Verify live output through the supported paginated turn API, including authoritative proposed-plan items, instead of assuming legacy rollout files.
- Disable inherited signing only for temporary acceptance-fixture commits; never invoke the user's signing agent or modify global Git configuration.
- Raise amux's declared Rust minimum to 1.88 and update `time` to 0.3.47, `time-core` to 0.1.8 and `num-conv` to 0.2.2 to remove GHSA-r6v5-fh4h-64xc. Keep Ratatui/Crossterm and the independent showcase's Rust minimum unchanged; no upstream backport or fork.
- Record the user's explicit acceptance of unmodified Codex's missing native input fence and optimistic losing-answer history. Defer full native/structured correctness and all32 acceptance to future ACP proof, retain failed evidence and fail-closed grants, and do not claim the limitation is fixed.
- Retain application-requested focus reports in the native unknown-input ledger without resetting human inactivity. A paused consumer cannot receive old-generation focus after a falsely safe handoff; remove the obsolete unaccounted focus helper.
- Accept only true exit notifications before serializing child reaping with process cleanup. Darwin may return stopped-child status despite `WEXITED`; filtering it avoids a stop/resume shutdown deadlock while preserving zombie/birth identity and descendant cleanup.
- Retire actually removed or unpublished PTYs outside the live ownership map. Reclaim empty state and stop ended-target view-event fan-out while retaining required claims, unknown calls and their truthful late settlements.
- Serialize concurrent managed-agent close/shutdown through completion of the same teardown attempt, so daemon shutdown cannot abandon another caller's worker join, owned-process cleanup or private-root removal. Verify owned cleanup before fixture fallback.
- Retire private automatic-launch ownership after exact late shell settlement, without releasing a newer forced owner. Keep the original private source file while acknowledgement is unknown; delayed native acceptance runs it once, not a replay, and definite rejection/completion cleans it.
- Keep only outstanding shell settlement lookup after PTY removal, not live shell/context/agent state or filesystem resources. Late callbacks remain bound to their original stable UUID even if a replacement reuses it as a display name; removed native acceptance receives a negative permission reply and cannot revive authority.
- Bind native Force confirmation to a completed, fully visible warning and drained event queue. Consume queued Enter without takeover/replay, invalidate on resize/scroll and block undrawable or clipped dialogs; keep fresh observation checks and the same live process. Use only the pinned Ratatui paragraph-measurement feature, with no terminal library version replacement.
- Fail closed when opt-in smoke cleanup cannot refresh process ownership. Remove group-wide fallback signals, validate each descendant PID and signal direct children only while unreaped. Bound validation work, preserve cleanup errors and label stale survivor identities as tracked—not owned. Add a no-model regression and verify healthy/failure teardown with isolated local processes.

## Unreleased — Execution substrate reliability

- Correct DECOM-origin-relative CPR through the parsed ANSI Handler boundary; preserve native control behavior and independent expected replies.
- Compose joined graphemes in native narrow/wide cells, including chunked UTF-8, flags/modifiers/keycaps, CJK/combining, overwrite, wrapping and variation-width replay. Final-cluster working text is limited to 4 KiB; oversized input starts a new native scalar segment without dropping text.
- Prevent shared-config lost updates with canonical file identity, stable nonblocking writer locks, exact snapshot comparison and explicit `config-busy` / `config-conflict` errors. Sync both private replacement file and containing directory before success.
- Add daemon-wide terminal admission: default 64, configurable 1–256, counting retained exits and pending opens. Refund failed opens; clean partial integration resources and uninstalled PTY children.
- Centralize latest-20 hook/launch/enqueue diagnostics, with UTF-8-safe 16 KiB strings and explicit command/source/output truncation.
- Require bounded initialized API-v1 readiness for existing/new daemons, serialize automatic starts and clean startup sockets on failure without replacing unresponsive peers.
- Cancel active hook process groups on shutdown, reap their owned workers and skip pending hooks instead of waiting for configured timeouts.
- Breaking Rust-library change: `projects::run_hooks` now requires an explicit `&AtomicBool` cancellation argument. CLI/API framing remains version 1.
- Verify 210 workspace Rust tests, the strict independent-showcase integration, 8 OMP tests, Clippy, debug/release builds, Zsh syntax and private native CLI/TUI lifecycle probes. Retain the initial queued-execution timeout observation and exact limits in `specs/verifications/substrate-reliability.md`; no independent review PASS or live-daemon upgrade is implied.

## Unreleased — Terminal capability explorer

- Independent `terminal-showcase` Rust workspace package; amux's default binary and implementation remain unchanged.
- Fullscreen capability matrix with explicit protocol/visual evidence, eight cycling demonstrations, deterministic screenshot mode and bounded row-differential redraws.
- Independent incremental CSI/OSC/DCS/APC parser and correlated terminal queries; missing replies remain Unknown rather than Unsupported.
- Color/Unicode/typography/control specimens, Kitty/Sixel/iTerm procedural image lifecycles, advanced input inspector and explicit-consent integration actions.
- POSIX terminal/signal RAII cleanup, resize handling, bracketed-paste-safe controls, headless JSON regressions and visual pass/fail export.
- Ordinary-shell amux compatibility regression exposes origin-mode CPR and joined-emoji cursor mismatches without sharing the multiplexer oracle. Strict `--probe --check` preserves failures and exits nonzero.

## Unreleased — Ratatui UI

- Ratatui 0.30.0 rendering with the existing Crossterm 0.29 input/backend and Rust minimum raised to 1.86.
- Three-row title/tab bar and top-right Switch button; native terminal fills every remaining row with no footer.
- Full-screen hierarchical fuzzy window list with two-line rows, direct click/Enter selection and retained launch/tree shortcuts.
- Actual exit-code view with Close window and New terminal buttons, keyboard alternatives, compact stacked layout and empty-list recovery.
- Native wrapper smoke verified Unicode/palette colors, application cursor, mouse coordinates, safe bracketed paste, focus, literal prefix, alternate-screen restoration, picker/rapid resize, exit codes 0/7/9, close/new/detach and shutdown. Existing 56 Rust tests, Clippy and release build pass.
- Explicit New/Exit empty-window card and top-right Exit; Enter/N starts a terminal, Escape/Q detaches to the host without stopping the daemon. Visible picker Ctrl-Q hint and fresh-window guard preserve fuzzy search before snapshot refresh.
- Native input hot path now reuses a deadline-bound connection, serializes borrowed input bytes and transfers damage-row screen updates with atomic generation publication; eight-millisecond polling keeps UI dirtiness separate from native damage.
- Actual TUI PTY measurements: typical120×40 shell median10.81ms;500×163 median30.34ms versus direct baseline428.85ms. Separate noisy output/50,000-row scrollback keeps large raw echo median30.39ms. Full-frame redraws and physical compositor paint remain explicit limits.
- Integrated66 Rust tests,8 OMP tests, Clippy, release build, Zsh syntax and actual Herdr empty-page recovery pass; one-shot EOF, persistent framing/error continuation and bounded idle expiry verified. Paused/undrained clients may disconnect after five seconds without requests; no hidden replay.
- Client-only second latency pass caches prepared native styled cells, prioritizes bounded queued-input bursts and presents immediately with brief 1 ms interaction pacing; idle polling remains 8 ms. Stock-Zsh medians now 2.01 / 3.94 / 6.77 ms at 120 / 300 / 500 columns, and a large 24-byte burst takes 3.98 ms. Cold full-refresh outliers around 130 ms remain documented.
- Existing 66 Rust tests, 8 OMP tests, Clippy, release build and Zsh syntax pass; actual Herdr selection, Unicode/colors, picker, exit/new/empty-page detach pass. Compatible with the first-pass daemon without another process-disrupting restart. See `specs/verifications/immediate-input.md`.
- Daily-use checkpoint verifies a private locked installation outside the checkout, cold auto-start, native detach/reattach, captured context, process-preserving reparenting, busy/partial refusal, descendant cleanup, hook failure recovery and config-only restart. Retired latency workspace/checkout and empty root artifact removed with approval; committed evidence and usable build retained.

## 0.1.0 — local MVP

- Rust daemon-owned PTYs and alacritty_terminal emulator, one-surface detachable TUI, and `amux` CLI over a versioned local API.
- Unsaved/saved projects, captured Zsh workspaces, independent terminal trees, asynchronous worktree-aware Git status, configurable recipes and bounded setup hooks.
- Native stock-Zsh interaction plus confirmed empty-prompt source execution, explicit FIFO queueing, exported-context snapshots and input-epoch protection.
- OMP 18.8.7 lifecycle extension with root/child isolation, explicit working/attention/settled state, conversation identity and crash/report-loss handling.
- Sequenced snapshots/events/waits with explicit stream gaps; bounded screen/scrollback/input; detach, retained exits and descendant-aware close/shutdown.
- Native keyboard, mouse, paste, Unicode, alternate/synchronized screens, scrolling, selection and resize validation on macOS arm64. 56 Rust and 8 OMP tests pass.

Linux runtime, Rust 1.86 minimum and other OMP versions remain unverified. No crash recovery or interpreter-state cloning. Independent review reached its five-round cap; final concurrent-removal finding was reproduced and fixed, but no post-fix independent PASS is claimed. The user explicitly authorized integration into main and publication to `addomam/multiplexer` after daily-use validation. No release tag, registry publication or deployment requested. See `specs/verifications/MVP-e01.md`, `REVIEW-e01.md` and `daily-use-checkpoint.md`.