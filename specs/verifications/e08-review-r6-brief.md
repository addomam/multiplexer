---
type: review-brief
context: {project: amux, epic: e08, story: e08s01, round: 6, maximum_rounds: 10, baseline_commit: cc468a3, security_sensitive: true, depth: full_maturity}
---
# E08 independent round6 — complete-feature review

## Authorization, independence and scope

User explicitly said: “Raise the review cap to 10 and keep going.” Only the E08 implementation-review maximum changes5→10. Five consumed pairs remain consumed; this is round6/10, not round1 or a reset. See e08-review-cap-extension.json. Other historical review/provider caps and delivery restrictions remain unchanged. Old documents describing exhaustion5/5 are historical, superseded only for this E08 cap by the explicit authorization.

Review independently and blind. Do not contact/read the other current R6 reviewer. Read all complete historical reports, but neither current peer's output. Parent must read BOTH complete reports before any remediation. No source edits while either reviewer runs. Only metadata outside the frozen21-file set may change.

Repository /Users/addomam/projects/amux-acp, isolated branch feat/agent-conversation-recovery, baseline cc468a3. Preexisting dirty E07/startup changes are intentionally preserved. Review complete E08 and introduced regressions, including unchanged consumers; do not reopen settled requirements or demand a redesign of best-effort native interaction.

Outcome: on relaunch automatically reopen every supported previously open exact saved agent conversation in saved cwd/project/name, then WAIT for fresh user instruction. No recovery picker, latest/new fallback, automatic continue/notification prompt, interrupted turn/tool/subagent retry, permission answer, queued input, old authority/control/handle/responder/RPC replay or OMP --restore. Context, not processes or permission to work, is restored.

Native application-buffer settlement remains historically unknown/fail-closed, not a new acceptance guarantee. Force never implicitly interrupts/kills work and is not ordinary safe-grant proof. E05 remains parked. No new live models/auth reads/global daemon/config/install/commit/merge/push/tag/release.

## Required reading and independent source certification

Read CONVENTIONS.md; specs/epics/e08-conversation-recovery/{PLAN.md,epic.yaml,e08s01-story.md,e08s01-tasks.yaml}; specs/product/SCOPE_E08.yaml; specs/security/epics/e08/THREAT_MODEL.md; specs/verifications/{AUDIT-e08-e08s01.md,e08-review-response.md,e08-recovery.json,e08-recovery-final-runtime.json,e08-review-cap-extension.json,e08-review-r6-source.json}.

All story/task acceptance intentionally remains failing pending current independent clearance. Read ALL TEN COMPLETE historical report bodies: e08-review-r1-A.json/-B.json through e08-review-r4-A.json/-B.json, plus lowercase e08-review-r5-a.json/-b.json. Read embedded full report strings rather than clipped JSON previews.

The R6 source manifest contains21 SHA-256 entries. Independently recompute every digest using actual read-only checksum tooling, report matched/required/mismatched/unverified; parent digests are not independent measurements. Parent remeasured all21 and debug/installed binaries immediately before R6: unchanged. Debug0a3ba8b49849d9f20a1b298cc5abf3039f57430bbfc7eeef3eaaaac03af30062; installed remains1ad4b01497b0fcbe7b82a4579dcc551b18eab55278b853736ea91c6372c4e73f. Independent executable hashing is optional, source certification mandatory.

Read relevant complete implementation/test sections across all21 paths in the manifest, not only a diff or historical repairs. It now includes src/tui.rs in addition to journal/coordinator/daemon/ACP/Codex/integration/runtime, OMP extension, registration/dependencies, fixture and integration/native tests, README and CHANGELOG. Trace unchanged CLI/client readiness, project/settings and input consumers as needed. Read-only baseline diff is allowed; do not alter Git.

## Whole-feature contracts to challenge

- Closed bounded private journal4MiB/512 records, canonical socket namespace, no-follow/nonblocking owner-only regular files, stable writer lock/inode/bytes conflicts, atomic file+directory sync, uncertain-write freeze. Metadata excludes auth/env/transcript/screen/drafts/work/responders/permissions/authority. Native backing stores remain separate.
- Immutable boot UUID plus birth-qualified live/gone/unknown exclusion, fresh pins for root lifetime changes even same numeric PID; never signal old reported peers. Durable launch_pending precedes recovered spawn; unknown settlement blocks duplicates. Rollback needs conclusive exact-instance cleanup; uncertain resources remain charged.
- Saved association and target epochs precede closable publication; native fallible snapshot precedes publication/application admission. Explicit close, retirement and selected-root quit fence late observations/rollback without reviving deleted intent. Per-target checkpoint scope precedes candidate/native/ACP observation; unrelated targets remain independent.
- Sequence-qualified project metadata synchronizes save/config/automatic main-shell context changes outside topology guards, retained for active targets or journal groups and pruned otherwise. Exact per-conversation cwd differs from descriptive project context.
- ACP advertised load/resume only, exact opaque history/session/auth/settings routing, substituted identities rejected, fresh ownerless runtime identities and no new-session/work replay. Saved acknowledged settings replace local defaults before existing validation; normal new launches still merge defaults. Launch fingerprint remains mandatory. Ambiguous modern model categories retain exact choices without an unresolvable alias; valid legacy-mode-first behavior remains. Internal valid saved names256 bytes versus ordinary new128 unchanged.
- Native root selection rejects nested/displaced/retired reports; unavailable OMP context and sticky Codex replacement block old trusted intent, never invent a B identity or fallback to A. Exact quoted OMP --resume FILE, never --restore. Supported managed-native Codex restrictions remain; installed0.165 is not accepted by the existing0.160 native broker. ACP is separate.
- Native preparation admission/cancellation/join and shutdown ordering retain owned resources and capacity, avoid topology/authority/PTY locks across slow work, and precede request drain. Stock Zsh/OMP behavior preserved.

## R5 findings repaired in the current frozen source

R5 A96.875 must1 and B93.75 must1/should1, each20/20 source hashes, combined FAIL. Both complete reports read before remediation. Specific dispositions need independent confirmation, not parent PASS transfer:

1. Mutable project metadata no longer invalidates unattempted startup records. Exhaustive borrowed Record matching excludes only project name/context, retains project ID and EVERY launch/session/name/cwd/settings/fingerprint/peer/pending/block field. Current sequence-qualified metadata reaches staged candidates. Coordinator tests challenge all nonmetadata changes, stale rollback and deleted/retired intent. Public real native-version gate holds the worker while supported unsaved project name/context changes; two later exact histories reopen once, remain idle/ownerless, accept deliberate arithmetic, and explicit close stays excluded.
2. Initial attachment no longer competes for native recovery capacity. Ephemeral snapshot.startup_pending is true until worker recovery/fallback ordering completes; narrow RAII clears/notifies on all worker exits. Initial TUI/exact-selector selection waits without shell/controller allocation; Esc/Ctrl-Q quits safely, resize works, other events and partial paste are discarded through quiet and first-paint drains. Explicit launch requests and existing exact Resume continuity retain their paths, and explicit opens retain ordinary quotas. This is bootstrap coordination, not terminal-limit suppression or generic transport retry.
3. Failed fallback shell creation returns before scheduling daemon-start hooks. A real later project-open hook fences the shared FIFO worker, proving startup-hook absence beyond a missing marker; independent ACP history and fresh arithmetic remain usable. Ordinary no-native first-shell-before-hooks and stopping checks remain unchanged.

The R4 repair still restores native records within ordinary quota before optional blank fallback, never taking a published human terminal, exempting slots or raising limits. Standalone unchanged-limit1 and genuine reduced-quota retry cases remain.

## Executed evidence — reviewers must NOT rerun

READ ONLY: no builds/tests/lint/formatters/smoke/amux/agent/provider/helper binaries, auth reads, filesystem/config/Git mutation or branches/worktrees. Actual read-only checksum binaries ARE permitted and REQUIRED. Parent owns verification, avoiding concurrent checks.

- bg_43/artifact3584: focused24 public recovery and31 journal/coordinator, then examples/full586 workspace tests31 suites12 ignored/strict Clippy/fmt/build PASS;149.08s total wall. Aggregate0.00s is not runtime.
- artifact3589: exact38 ACP/9 integration/8 Codex commands PASS. OMP8 tests49 assertions, Zsh syntax PASS. No live ignored case ran; guarded owned fake-backend helper may run as a fixture child.
- All seven actual checks in e08-recovery-final-runtime.json are bound to debug0a3ba8b4 measured before/after:
  1. Cold ordinary and exact-selector TUI during native preparation at limit1: no implicit blank slot/control, safe quit/resize, physical keys and fragmented paste discarded, eventual genuine offline exact OMP context and no message replay.
  2. Same with spare capacity limit2: no implicit blank slot and same physical/input/identity checks.
  3. Genuine installed OMP in its original persistent controlling-PTY shell, exact file/session/cwd/project ID/name reopened under unchanged limit1; both startup-hook snapshots contain one terminal and the second sees restored native shell.
  4. Built CLI settings42→47→52 and342→647→952 over two reopenings; genuinely new default105; no startup prompt/cancel/session-new.
  5. Two exact ACP contexts retain42/17 before fresh input, fresh ownerless identities/stale-target rejection and actual Menu entries; no picker.
  6. Genuine native exact-file OMP reopening in fresh lifetimes without work/messages/--restore.
  7. Original persistent physical attachment /new at same native PID; new ID observed, no stale-A reopening or invented unsaved-B intent.
- Successful actual cases observe normal TUI/host/daemon exits, live-host termios restoration, joined drainers, owned peers/PTYs/socket/private-root absence and unrelated sentinel survival, not fallback cleanup as proof. PID/PPID-only descendant observation, no global command arguments. Provider0. ACP uses cleared private env/absolute deterministic peers; genuine OMP18.13.3 uses private state/fake key, sandbox network denial except Unix IPC and global-auth-path denial. Extension18.8.7 is a static SDK pin, not the measured CLI version.
- Cold gate intentionally fails a private Codex version probe, followed by genuine OMP recovery. It is NOT successful new native-Codex provider acceptance.

Historical settings/quota RED→GREEN, preparation/shutdown RED and native fixture ordering failures remain in evidence. Final R5 findings were source-proven and fixed directly; no old-source runtime RED is invented for each. First integration3557 passed behavior but hit two test-only Clippy rules, fixed without suppression. Redundant private staged-Candidate.project copy assertion was deleted (public durable/history/effect gates retained), then full and all seven current-binary checks reran. Long macOS TMPDIR correctly hit managed socket length; fixture now uses private canonical/tmp, no production workaround. Literal idle-word presentation gate was deleted, never re-pinned; exact identity/content/messages/effects still checked.

## Review limits and output

No global clock shift or exact CPU pause at every historical instruction boundary. Pending-marker crash case is stopped-journal marker injection, not an actual spawn-to-pin crash. No Linux/minimum-Rust/new live-provider/installed native Codex0.165 acceptance. Quantitative coverage waiver remains; no bundled bigpowers scripts or fabricated script/coverage PASS. Intentional malicious same-UID trusted-owner rewriting is excluded, not unsafe filesystem objects or real protocol/lifecycle races.

Assess complete correctness, security, producer-to-consuming-dispatch dataflow, lifecycle/races/identity, conventions, F.I.R.S.T behavioral quality, design/Fowler/clarity and bounded performance/resource cleanup. Real CONVENTIONS.md governs; generic20-line/300-line style rules are not project requirements. Findings need exact current file:line, causal consumer-visible scenario, must-fix/should-fix/consider, priority/confidence; evidence-backed HIGH/CRITICAL security confidence>=8 blocks. No speculative or cosmetic scope.

Return FULL report, each historical issue's specific cleared/remaining disposition, explicit distinct criterion denominator and score100*(total-must-should)/total without inflated counts or coverage conflation. Each PASS requires zero must and score>=94; parent addresses all should. Include independently measured hashes matched/required/mismatched/unverified, exact actions actually performed/not performed and limits. Do not grant runtime/delivery authorization or acceptance from old PASSes. Stay blind until both reports complete. This pair consumes6/10, with all earlier five pairs retained.
