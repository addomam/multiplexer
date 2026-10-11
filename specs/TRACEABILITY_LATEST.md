# amux requirement traceability

Manual, file-level ownership map backed by actual behavior tests and native UAT; not a story-comment scanner result or a code-coverage percentage. Plans: `specs/release-plan.yaml`, `specs/epics/e01-native-terminal/`. Verdicts: `specs/verifications/e01s01-verify.yaml` through `e01s06-verify.yaml`.

| Story | Implementation | Behavioral verification | Status |
| --- | --- | --- | --- |
| e01s01 — host vertical slice | `src/runtime.rs`, `src/daemon.rs`, `src/main.rs`, `src/lib.rs` | `tests/runtime.rs`, `tests/acceptance.rs`; actual stock Zsh, daemon-owned detached screens, descendant shutdown | Covered |
| e01s02 — shell/context | `src/integration.rs`, `integrations/zsh/adapter.zsh`, daemon source/input dispatch | `tests/integration.rs`, `tests/shell_races.rs`, acceptance native Zsh; partial input, foreground busy, explicit queue, epoch race, captured cwd/exports, startup changes/removals, interrupts/loss | Covered |
| e01s03 — projects/Git/hooks | `src/projects.rs`, daemon project/tree operations | Project module tests and `tests/acceptance.rs`; real worktrees/upstream/status, immediate context save, stable config, hook failures/order, concurrent parent publication/removal | Covered |
| e01s04 — OMP | `integrations/omp/amux.ts`, `lifecycle.ts`, integration agent registry and daemon wait predicates | `lifecycle.test.ts`, Rust lifecycle/lease/cursor tests; actual manual/action OMP 18.8.7 working/question/settlement/native exit | Covered |
| e01s05 — TUI/CLI | `src/tui.rs`, `src/cli.rs`, `src/client.rs`, daemon API | TUI behavioral encoding/tree/selection tests, API acceptance tests; actual native TUI, modes/resize/picker, exited/oversize errors, CLI screen/input, detach/reattach | Covered |
| e01s06 — verification/delivery | README, integration contracts, architecture, changelog, verification/state artifacts | `preflight-e01.log`, `MVP-e01.md`, measured `NFR-e01.json`, separate failed-run logs, dual independent reviews | Runtime/documentation covered; release gate blocked |

All six slices have implementation and observable test/native evidence; no known unimplemented user slice is hidden as a scaffold. Shared daemon/runtime ownership is intentional, not a generic plugin boundary. Root entrypoints and Cargo files belong to the host/runtime slice.

Limitations are explicit: Linux execution blocked by Docker VM I/O errors; minimum Rust and other OMP versions unverified; no human acceptance sign-off. Installed structural scripts target the skill package rather than this project. Review cap exhausted (5/5); final finding fixed and mechanically/runtime verified, but independent post-fix PASS unavailable. See `specs/verifications/ENVIRONMENT-e01.md` and `REVIEW-e01.md`.