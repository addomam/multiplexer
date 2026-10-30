# amux requirement traceability

Manual, file-level ownership map backed by actual behavior tests and native UAT; not a story-comment scanner result or a code-coverage percentage. Plans: `specs/release-plan.yaml`, `specs/epics/e01-native-terminal/`. Verdicts: `specs/verifications/e01s01-verify.yaml` through `e01s06-verify.yaml`.

| Story | Implementation | Behavioral verification | Status |
| --- | --- | --- | --- |
| e01s01 — host vertical slice | `src/runtime.rs`, `src/daemon.rs`, `src/main.rs`, `src/lib.rs` | `tests/runtime.rs`, `tests/acceptance.rs`; actual stock Zsh, daemon-owned detached screens, descendant shutdown | Covered |
| e01s02 — shell/context | `src/integration.rs`, `integrations/zsh/adapter.zsh`, daemon source/input dispatch | `tests/integration.rs`, `tests/shell_races.rs`, acceptance native Zsh; partial input, foreground busy, explicit queue, epoch race, captured cwd/exports, startup changes/removals, interrupts/loss | Covered |
| e01s03 — projects/Git/hooks | `src/projects.rs`, daemon project/tree operations | Project module tests and `tests/acceptance.rs`; real worktrees/upstream/status, immediate context save, stable config, hook failures/order, concurrent parent publication/removal | Covered |
| e01s04 — OMP | `integrations/omp/amux.ts`, `lifecycle.ts`, integration agent registry and daemon wait predicates | `lifecycle.test.ts`, Rust lifecycle/lease/cursor tests; actual manual/action OMP 18.8.7 working/question/settlement/native exit | Covered |
| e01s05 — TUI/CLI | `src/tui.rs`, `src/cli.rs`, `src/client.rs`, daemon API | TUI behavioral encoding/tree/selection tests, API acceptance tests; actual native TUI, modes/resize/picker, exited/oversize errors, CLI screen/input, detach/reattach | Covered |
| e01s06 — verification/delivery | README, integration contracts, architecture, changelog, verification/state artifacts | `preflight-e01.log`, `MVP-e01.md`, measured `NFR-e01.json`, separate failed-run logs, dual independent reviews and `daily-use-checkpoint.md` | Runtime/documentation covered; user approved main landing |

All six slices have implementation and observable test/native evidence; no known unimplemented user slice is hidden as a scaffold. Shared daemon/runtime ownership is intentional, not a generic plugin boundary. Root entrypoints and Cargo files belong to the host/runtime slice.

Limitations are explicit: Linux execution blocked by Docker VM I/O errors; minimum Rust and other OMP versions unverified. User accepted local daily use and explicitly authorized main integration/GitHub push. Installed structural scripts target the skill package rather than this project; structured matrix/blind-spot inputs are absent, so deterministic gate-trace is WAIVED, not PASS. Review cap remains exhausted (5/5); final finding fixed and mechanically/runtime verified, but independent post-fix PASS unavailable. See `specs/verifications/ENVIRONMENT-e01.md`, `REVIEW-e01.md` and `daily-use-checkpoint.md`.

## E07/E08 signed delivery

| Story | Implementation | Behavioral verification | Status |
| --- | --- | --- | --- |
| e07s01–e07s06 — approved modal daily-use UX | `src/tui.rs`, `src/tui/{menu,project,view,agent,control,launch}.rs`, `src/{cli,daemon,projects}.rs` | E07 story/task ledgers, `e07-ux.json`45 private UI cases, round4 A100/B100; later startup-config filename proof separate; complete current E08 workspace gate | Locally accepted; source delivery authorized |
| e08s01 — exact-context reopening without work continuation | `src/recovery.rs`, `src/daemon/recovery.rs`, `src/{acp,codex,integration,runtime,daemon,tui}.rs`, OMP adapter | `tests/recovery.rs`26 cases, journal/coordinator31, named ACP/native integration, `e08-recovery-r7-runtime.json`9 actual scenarios and full593 workspace cases plus guarded child; R7 A100/B100, each21/21 hashes | Locally accepted; source delivery authorized |

Current deterministic gate-trace verdict: **WAIVED**, not PASS. Repository scripts and structured matrix/blind-spot/drift inputs are absent; no percentage or scanner output is invented. Refutation check: exact-native import corruption/alias side effects, hook-order cancellation, cold-attachment quota/input replay and missing saved-context behavior would block delivery without source/test/runtime evidence; current E08 ledger and nine-check receipt cover those boundaries. No new unimplemented story or missing verification was identified. E05 is parked, not presented as complete. Quantitative coverage waiver and historical limitations remain unchanged.