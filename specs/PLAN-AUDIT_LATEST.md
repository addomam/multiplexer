# Plan audit — e03 agent interaction

**Verdict: READY — Plan artifacts, parent checks and independent dual quality gate passed.** Build still requires explicit phase confirmation; this is not runtime acceptance.

## Principles alignment

| Check | Assessment |
|---|---|
| Vertical slices | Eight prescribed slices deliver ordinary control, overrides/recovery, native UX, conditional reacquisition, one managed agent, real interactions, exact interruption and complete-broker handoff. None is only a model/controller layer. |
| Bounded scope | Current scope maps each included outcome to e03s01–08; ten baseline stories are retained separately. CLI porcelain, orchestration policy, restoration, remote backends and extra providers remain excluded. |
| Observable success | All32 interaction scenarios remain mandatory; final complete-broker acceptance cannot substitute deterministic fixtures or force-only behavior for safe native/API handoff. |
| Hard gates | Plan quality and explicit Build confirmation; feature branch/baseline before implementation; explicit real-run opt-in and bounded model cost; unknown settlement blocks safe grant and first acceptance. |
| Domain language | Existing project/workspace/execution/terminal, controller/control/claim/offer and distinct harness/thread/turn/request identities are retained. |
| Impact | Parent LSP references plus source/test mapping cover dispatch, queues, Zsh continuations, all TUI input routes, one-shot callers and lifecycle cleanup. |
| Dependency discipline | Only tungstenite0.30 is proposed, tagged OK with actual version/MSRV/features and custom-stream API evidence. No async runtime/provider framework or SUS/SLOP dependency. |
| Premature abstraction | Three new deep boundaries have explicit reasons: ownership invariants, concrete Codex lifecycle/protocol, and unified native ownership decisions. No generic adapter registry. |

## Conventions and pre-flight answers

| Item | Answer |
|---|---|
| Test | `cargo test --workspace`; named existing/future integration targets as declared by each story |
| Build | `cargo build --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Typecheck | `cargo check --workspace --all-targets` |
| Explicit compatibility | Build workspace, then select the exact independent multiplexer ignored test; never bulk-run foreground fixture tests |
| CI | No existing CI configuration found in the targeted checkout paths; no CI wiring is added to this scope |
| Workflow | Existing Rust2024 daemon/ratatui native TUI; solo-git; feature branch required for future code; Conventional Commits only if requested |
| Repository rules | Existing project context and CONVENTIONS.md apply; runtime state in memory, stock Zsh/OMP preserved, no remote/restore/tiling/plugin expansion |
| Documentation | Plans/evidence in specs; README/usage and obsolete naked-input examples must migrate with implementation, not describe an unimplemented feature as shipped |
| Format tooling | Installed plan-consistency checker is available; repository has no bundled bigpowers scripts. Countable format asset is absent from the installed package, so each new story explicitly supplies20 numbered sections, maturity3, §17 acceptance and before/after deltas. |

## Verification strategy and limits

`tests/pty_control.rs`, `tests/agent_interaction.rs` and opt-in `tests/codex_native_smoke.rs` are explicit future story deliverables, not already runnable targets or passing evidence. Existing low-level runtime fixtures retain their original boundary. New permanent tests must catch plausible consumer-visible authority, transition, identity, timing/view, failure and cleanup bugs, not wiring/source-text/mock-echo assertions. Native UI proof launches actual attachments on controlling PTYs; real harness proof uses the actual owned Codex/native path.

The user's native-buffer deferral is retained at e03s08 final complete-broker validation, without an early probe or safe-grant waiver. Fresh native-first creation also needs actual supported-interface verification, with no fabricated setup model turn. Missing credentials, model access or an application seam blocks the selected live acceptance rather than silently skipping or passing.

## Document-only self-audit

| Area | Result and applicability |
|---|---|
| Correctness | PASS: dependency/creator ordering, all32 mappings, private/public authority boundaries, cancel versus conditional keep, retained calls and final acceptance obligations checked across the capsule. |
| Security and supply chain | PASS for the plan: explicit private capabilities, original-connection provenance, sanitized native labels and private live-run cleanup; credential-token pattern scan found no matches in the capsule/scope/impact/audit/review packet. Proposed dependency metadata/API evidence is recorded. Runtime security is not claimed. |
| Performance and design | PASS for the plan: cached borrowed authority, bounded batch/watermarks, no per-key registration or locks across external I/O; three justified deep boundaries, no extra provider/retry/clock framework. |
| Provenance and clarity | PASS: new capsule PLAN/manifest/story/task artifacts include type and context; stories reference the approved ADR/wire/native decisions and distinguish future commands from evidence. |
| Verification | PASS:16 strict YAML artifacts, eight story frontmatters,20 sections/story,32 matching step/task commands with shell syntax checks, all32 mappings and90 static wire cases. Installed consistency check reports CRITICAL=0 HIGH=0 MED=0. Actual runtime/native/model behavior remains unexecuted. |
| Code-specific checks | Not applicable: no source functions, casts, method chains or runtime refactor were added. Churn helper is not bundled and no code churn is being reviewed; impact mapping prioritizes the shared dispatch/input hotspots instead. No rationale is used to waive future behavior/cleanup tests. |

## Open gates

- Ask for explicit Plan-to-Build confirmation. Both reviewers passed without findings; implementation has not started.
- After that confirmation, isolate future code on a feature branch and verify its baseline. Selected live-model runs retain their separate explicit approval and bounded/private-resource gates.
