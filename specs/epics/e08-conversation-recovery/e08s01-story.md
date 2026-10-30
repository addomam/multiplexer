---
type: story
context: {project: amux, epic: e08, plan: PLAN.md}
---

# e08s01 — Durable exact-session recovery on relaunch

## Business narrative
A user relaunching amux after a reboot, daemon restart or crash must not hunt through agent histories to find the conversations previously open. All supported conversations reopen automatically, and wait while the user explains the interruption and arranges setup.

## Requirements

- MODIFIED — Before: live conversation/launch identities are memory-only and every ACP start creates a new session. After: bounded durable metadata identifies previously open supported conversations; startup reopens their exact native/ACP saved contexts in the recorded cwd/project/name with fresh amux process, target and authority identities.
- ADDED — No recovery picker or per-session confirmation. Recovery never submits a continue/notification prompt, retries interrupted work/tools/subagents, invokes OMP --restore, answers a permission or replays input. Transcript replay is display-only.
- ADDED — ACP initialization chooses advertised session/load, or session/resume when only that is supported; authentication retries preserve the requested saved conversation. No new/latest fallback. Native OMP uses its reported ID/file; managed Codex uses its exact thread ID and durable native provider store. Uninstrumented arbitrary terminal commands are neither guessed nor replayed.
- ADDED — Per-session close/remove/native quit is forgotten. Daemon teardown preserves open conversation records; same-daemon detach does not relaunch. Missing/unsupported/corrupt/live-or-unverifiable-peer cases remain visible, retain saved intent and do not block other eligible conversations.
- ADDED — Journal is bounded, private, no-follow, writer-locked and atomically synced. It contains no auth, env, transcript, screen, prompt draft, pending call/responder, permission or old authority. Provider-native backing stores are separate and preserve data needed for true reopening.

## Acceptance

1. Two configured-profile conversations with distinct exact IDs/names/cwds retain independent arithmetic/history across normal daemon shutdown and relaunch without a picker.
2. Before any fresh user input, persisted effects and interaction choices are unchanged; restored agent processes/instance IDs/control identities are fresh and ownerless.
3. A deliberate fresh post-recovery prompt works in the correct original conversation. Old target/control handles are rejected.
4. Session-specific close is not resurrected; daemon shutdown does not erase other open records. Same-daemon reattachment does not duplicate processes.
5. Missing session/cwd/executable or unsupported load capability produces explicit bounded diagnostics without session/new/latest/picker fallback; other records still reopen.
6. Corrupt/unsafe journal and concurrent writer cannot overwrite intent or silently claim durability; exact live/unverifiable old peers are never duplicated or killed.
7. Native OMP exact-file/cwd and managed Codex backing-store lifetimes are preserved without restoring work or copying credentials to recovery storage. Native root identity cannot be replaced by a subagent/stale report.
8. Full existing Rust/OMP/Zsh/ownership/native/ACP boundaries and actual isolated CLI/TUI cleanup remain verified. No live-provider/global installation or publication is implied.

## Evidence
Permanent uncertain-boundary tests plus actual private process/CLI/TUI receipt in specs/verifications/e08-recovery.json. No PASS before execution. Pre-edit native handoff timeout artifact3266 remains historical and must be classified, not hidden.
