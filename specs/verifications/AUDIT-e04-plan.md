# e04 Plan self-audit

Verdict: **READY FOR BUILD — independent Plan round3 passed 100/100, zero findings.** User explicitly selected “Plan, then Build if reviews pass”; that authorization is now active. Full reports and failed rounds1–2 are retained. This new e04 gate does not reset e03 caps/runtime failure or prove any ACP implementation/runtime acceptance.

## Principles and completeness

| Check | Result / evidence |
|---|---|
| Vertical delivery | Four 8-BCP stories each deliver an observable shared session/view, genuine interactions, running ownership/recovery or complete acceptance, not a horizontal model layer. All initial APIs are gated in s01; interaction cancellation obligations ship in s02, not deferred behind unsafe exposure. |
| Countable artifacts | Four maturity3 specs, 20 numbered sections each, 22 matching implementation-step/task commands and 23 scenario IDs matched exactly across story acceptance, task mappings and risk-scaled test plan. All stories/tasks failing. |
| Scope/deltas | MODIFIED before/after in s01/s03/s04; ADDED s02. ACP-only human view/admission cut user-confirmed, terminal semantics/failed fence preserved, generic non-ACP API deferred. |
| Existing architecture | IMPACT maps purpose/callers/contracts for authority, daemon, CLI/client/TUI, project persistence, runtime/Codex and workspace consumers. Existing native closed variants and OMP agent-settled unchanged. New target kind is explicit, not a fake terminal. |
| Dependencies/API | Official schema-only =1.12.0/default-features=false [OK], primary published manifest/source/signatures cited. Full SDK runtime/process helper rejected for bounded-memory/cleanup mismatch; no new runtime/executor/generic validator. Actual lock/MSRV/feature-unification still a Build verification. |
| Depth | One concrete broker owns identity/transport/work outcomes, one typed editor owns stale-draft boundary; authority callback is memory-only at existing ordering point. Reasons for Depth in PLAN/stories. No generic adapter framework or separate ownership state machine. |
| Races/protocol | Atomic authority→ACP commit; register RPC before writer; one nonblocking FIFO consumer; exact typed IDs/scopes; retained accepted work across handoff; noncancelling force; ordinary cancel grants none; canonical answer race and explicit late Cancelled/Cancel obligations; cancelled stop reason only interrupt confirmation. |
| Resource/lifecycle | Exact process/frame/queue/in-flight/reservation/schema/history budgets and explicit overflow/unknown. Pre-accumulation bounds, continued stderr drain and birth-qualified owned cleanup with late-publication/shutdown/sentinel proof. No locks across I/O/join/topology inversion. |
| Human path | Existing ratatui/crossterm; actual same-session conversation/forms/permission view; explicit review/edit/decline/cancel and exact offered options, captured control/generation/revision, no optimistic losing answer/raw route/replay. Pattern/format local-validation limitations explicit. |
| Security/privacy | Threat model covers CWE863/367/639/362/400/664/200/117/116/833 boundaries. Public metadata excludes content/auth/env/raw errors; untrusted output inert; client URL/fs/terminal delegation absent. Runtime audit remains required, not pre-passed by this model. |
| Verification | Deterministic real pipes + actual private daemon/API/CLI/controlling-PTY and explicit race barriers; permanent tests only meaningful uncertain boundaries. Actual smoke beyond tests; cleanup observed before fixture fallback. Live both-adapter acceptance is a separate fail-closed task, not skipped-to-pass. |
| Ownership of edits | Parent owns all shared routing/manifests/ledgers; substantial backend/view slices get separate Herdr tabs/worktrees only after contracts stabilize. Worker files do not overlap; interface changes parent-sequenced; no worker build/lint/tests/formatters mid-flight. Only safely integrated owned resources retired. |
| Conventions/preflight | Rust2024/native stack, memory-only runtime/config-on-disk, stock Zsh/OMP, stable errors/IDs, slow clients not blocking PTY; solo-git feature branch. Existing baseline: 308 workspace Rust/8 OMP tests plus strict Clippy/check/build/fmt/Zsh/help. No CI-green claim, no ACP behavior inferred. |

## Observed static checks

`e04-plan-static.json`: 10 strict current/new YAML documents; two historical state/execution YAML documents parsed without rewriting their existing shape; four strict story frontmatters; 22 task/step command matches, 11 unique commands pass `bash -n -c`; 23 acceptance IDs fully mapped; e03s08 failing, old review 5/5, new e04 Plan round1/max5 preserved.

Installed command:
`bash /Users/addomam/.omp/plugins/node_modules/bigpowers/scripts/lib/plan-consistency-check.sh specs/epics/e04-acp-agent-interaction`

Observed: **CRITICAL=0 HIGH=0 MED=0; PASS**. An initial MED4 was the heuristic matching the phrase “ambiguous delivery” (a truthful transport outcome, not an unresolved product requirement); clearer “unconfirmed delivery” retains the same unknown semantics. Checker uses its installed package root for verify extraction and numeric task IDs; direct capsule parsing/command matching/shell syntax above supplements that limitation. No future Cargo command ran or passed during Plan.

LSP references were attempted again for current isolated `Registry::add_target`; reload still returned empty despite the source-observed daemon caller. A workspace-folder notification sent through the tool's request-only route was rejected as unknown request; no workspace registration/configuration change occurred. Original checkout has no e03 `src/control.rs`; its older sources are not an authoritative substitute. Continue current-file source/caller mapping and compiler/full-consumer proof; do not claim empty LSP results mean no callers, or rename exported symbols without usable references. Existing public signatures need not be renamed for the additive ACP path.

## Gate result and remaining runtime proof

Two fresh independent round3 Plan reviews each passed 100 with zero findings; see `e04-plan-review-round3.json`. Build can proceed under the user's conditional authorization. New provider/live/auth/global config and ACP main publication remain separately gated. Runtime tasks stay failing until actual named commands/scenarios and surface/cleanup evidence succeed.

## Plan round1 response and round2 input

Independent Plan round1 AND gate **FAIL**, A93.75 / B100, zero must-fix and two should-fix; complete reports preserved in `e04-plan-review-round1.json`. Corrected task/story text to exclude rejected losing answers, and fixed native regressions to the real `pty_control` target/native submodule without running the invalid command. Added ignored independent-showcase command directly to s04 task3, updated context file responsibilities/settled-work pointers, made ACP focused eligibility/activity explicit and corrected Accept's payload constructor. Tool display now separates local cancel intent from actual agent/tool outcomes.

Parent additionally replaced the libtest-stdout fixture design with standalone non-production `acp_fixture` Cargo example (`tests/fixtures/acp_peer.rs`), added its build prerequisite to selected-test commands, and fixed explicit/system close to end authority+broker lifetime at one target ordering point before unlocking. Parent still exclusively owns Cargo/shared code; backend owns the extra fixture source. Force/safe/recovery presentation names retained ACP work without native-settlement claims.

Current static revalidation belongs to `e04-plan-static-round3.json`; old static/review failures remain historical, not overwritten. Two fresh independent round3 reviews must cover the current packet before conditional Build activates. No production code or ACP runtime/provider result is claimed.

Plan round2 **FAIL**, 96.875/96.875 with one must-fix each: no project-close consumer exists. Full reports are preserved in `e04-plan-review-round2.json`. User explicitly selected **Use existing lifecycle routes** and deferred broader lifecycle UX until hands-on use. Current Plan now binds system end to actual daemon dispatch/stopping/teardown, traverses every live/reserved ACP view and verifies late-open cleanup. Native shell close/remove leaves ACP views alive. No unrouted project-close helper, new project-close API or substitution of shell close remains required. Scenario/task counts and verification commands are unchanged.
