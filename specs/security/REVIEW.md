# amux e06 verified-delivery security disposition

Current source/security approval: `../verifications/e06-review-round11.json`, independent A100/B100/security100 with zero findings. All92 frozen paths matched at verdict; before the authorized commit, all86 production/test/vendor/Cargo-manifest paths still match that approved snapshot. Subsequent changes only record verified acceptance, status and landing authorization.

Runtime and genuine installed-provider proof: `../verifications/e06-daily-use-verify.json` and `../verifications/e06-omp-live-final.json`. Complete regressions, native input-boundary fixes and one genuine OMP question/native-answer/API-permission deny+allow run pass. Model use is12/cap12/0; no further live calls. Copied auth/raw resources are removed, and the global subscription credential is unchanged.

The user now explicitly authorizes commit and local-main merge only. No unresolved supported security finding is waived; prior failures, native unknown application settlement and all historical review caps remain preserved. No push, tag, release, global changes or new provider run is authorized.

## Historical e04 security disposition

# amux e04 current-delivery security disposition

Current candidate: `66b5293`, based on main `a46eaddeb79a9a42628d9cb39cd60945ece27fbb`. Exact runtime-source binding: `specs/verifications/e04s04-rejection-recovery.json`; all 22 hashes still match full gate `artifact://1803`. This section supersedes the historical e03 delivery scope below.

Latest independent specialist: `epics/e04/e04s04-review-supplemental1.json`, zero supported security findings, one operational must-fix (`E04S04-OPS-S1-01`). The operational defect is locally repaired and verified through the real controlling PTY and complete regression gate. Original five-round and supplemental FAIL reports remain unchanged; there is no independent post-repair PASS.

The repair returns an existing captured action envelope only for a typed definite daemon non-admission. Answer values are removed, exact pending target/call/revision/control/generation guards remain, and ambiguous transport failures stay fail-closed without replay. No new listener, credential handling, automatic retry, authority, process-signalling or provider execution is added. Current source self-audit is `../verifications/AUDIT-e04s04.md`.

The user explicitly accepts local main integration after this verified repair: `../verifications/e04-main-landing.json` and `EXCEPTIONS.md`. No supported unresolved HIGH security finding is being waived. This human delivery decision does not reset review caps, clear the historical native application fence, or authorize new model runs, global configuration, push, tag or release.

## Historical e03 delivery security review

Scope: complete integrated e03 relative to6318845 plus fixes, feat/e03-agent-interaction. Final round5 security PASS/no unwaived HIGH did not override general A97.5%/one P1 and B100%: overall FAIL, exhausted5/5. The cleanup repair has controlled regression/local-process proofs and final preflight308. The user's subsequent “looks great”/overnight commit authorization approves limited delivery after the cap, not a new independent security/Build PASS or claimed source inspection. See EXCEPTIONS.md; native limitations/full acceptance are unchanged.

- Boundary: local Unix socket; `src/client.rs` verifies a private current-user-owned directory. `src/daemon.rs` takes the runtime lock and creates a 0600 socket. No network listener. Same-user callers intentionally have execution authority; this is not a sandbox or per-project ACL.
- Shell source: API source is intentional code execution, stored in 0600 files under a 0700 integration directory (`src/integration.rs`). FIFO frames contain validated UUID/queue/generation fields, not source bytes. Native ZLE prompt, BUFFER, generation and typeahead guards reject foreground-app execution and partial-line replacement (`integrations/zsh/adapter.zsh`); input epochs/gate serialize private wake delivery. Source paths are Zsh-quoted before native top-level evaluation.
- Hooks: user-configured `/bin/sh -c` is intentional execution, not an injection vulnerability. Project configuration uses private atomic replacement (`src/projects.rs`), validation precedes mutation, hook cwd/environment/timeout/output limits are explicit.
- Resource boundaries: bounded API frames, PTY input queues, event retention, Git work, hook diagnostics and agent/execution histories. Slow control sockets are disconnected on shutdown, and their RAII ownership keeps late terminal cleanup alive until completion.
- Process signals: kernel birth identity is checked (`src/runtime.rs`); native tests cover PID mismatch and foreground/separate-session descendants. OS permission changes, uninterruptible I/O and already unobserved orphaning remain explicit limitations, not a sandbox guarantee.
- Secrets: captured exported environments and shell-context events remain local, same-user data. Do not publish unfiltered control-event logs: exports may contain credentials. Clipboard reads are denied; host clipboard writes require client selection.

Round2 cleanup completion, R3 late consumers and R4 visible Force retain their proofs and reports. Final general A found stale-snapshot signaling after failed smoke discovery/reaping. The repair removes group fallback, revalidates each descendant PID and accepts direct-child authority only while unreaped. Discovery/state failures retain errors, never grant kill authority. Controlled RED/healthy/failure proof used only fixture-owned local processes, no auth/models/global mutations; tracked survivor diagnostics do not claim current ownership. Original round5 reports remain unchanged. User-approved post-cap delivery permits commit/merge/push without claiming forbidden round6 or independent post-repair PASS.

The focus correction uses the existing authorized/native dispatch path, retaining unknown work without manufacturing human activity. Automatic emulator query replies remain separate. The reaper correction filters Darwin's non-exit waitid statuses before taking the cleanup mutex; it preserves original zombie identity through descendant discovery and serialized reap. Neither correction broadens permissions, force behavior, replay, transport exposure or credential persistence.
