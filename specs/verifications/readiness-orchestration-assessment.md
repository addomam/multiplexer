# Foundation readiness and orchestration direction

Date: 2026-10-18. Assessed checkout: `af21287`, branch `feat/terminal-showcase`.

Reason for existence: decide whether to replace Herdr and distinguish terminal hardening from the new agent-orchestration product. This is an assessment and proposed direction, not an approved implementation specification or new review PASS.

## Current boundary — user clarification, 2026-10-19

**amux is the execution substrate and tooling surface, not the autonomous orchestrator.** This clarification supersedes the ownership recommendations and proposed build sequence below. Earlier runtime evidence remains historical evidence.

### Reliability follow-up, 2026-10-19

The eight terminal/configuration/admission/diagnostic/readiness/hook defects identified here are now fixed, verified and committed on `fix/substrate-defects`. See [fresh evidence and retained limitations](substrate-reliability.md), including an initial queued-execution timeout whose cause was not established despite passing instrumented/final runs. Historical source locations, failing outputs and assessment commands below describe the original checkout, not the updated branch. Workspace providers, generic third-party lifecycle/interaction capability contracts and ACP remain future scope; the fixes do not turn amux into an autonomous orchestrator or certify complete Herdr replacement.

### Confirmed product direction

- amux supplies project/workspace organization, execution, interaction, observation and explicit lifecycle operations.
- External orchestration owns tasks, scheduling, coordinator roles, autonomous dispatch, retries, acceptance criteria, merge policy and automatic answers.
- Spawning a process on an explicit user/API request is substrate behavior. Deciding independently that another agent should run is orchestration.
- Worktree provisioning can be built in initially. Preserve a replaceable boundary without introducing a generic plugin framework now.
- Containers, VMs and Kubernetes are future execution environments, not current implementation commitments or authorization to add remote protocols.
- Native human interaction must remain available. Equivalent semantic operations must be accessible through CLI/API for agents and automation.
- Agent activity, errors, cycle completion and questions/approval requests should be observable where the harness supports them. Unsupported or stale observations remain unknown, not guessed success.
- Hooks/events may connect external responders. Answer-selection policy and autonomous question handling stay outside amux.

### Recommended contract shape — still to elaborate

Keep workspace provisioning separate from execution placement. A Git worktree can be mounted into a container; these are composable concerns, not mutually exclusive backend choices.

Workspace tooling should expose explicit create/adopt/inspect/release operations, ownership and busy/dirty safeguards. Execution tooling should expose launch/inspect/interact/resize/stop and attachment/output where supported.

Use stable resource handles and advertised capabilities. Do not make a local host path, host PID or PTY mandatory for every future backend. Keep the initial implementation local and concrete.

CLI/API parity means the same underlying operations and safety checks, not simulated key presses for every action. Preserve raw terminal interaction; use typed prompt/answer/cancel operations only where a harness supports them.

Do not collapse process lifetime, activity, interaction need and cycle outcome into one enum. A live agent can be working, waiting for permission or reporting an error; these observations do not determine task acceptance.

Bind question/approval responses to the actual agent instance/session and request identity. Reject stale answers. Advertise unsupported structured interaction rather than silently submitting text to the wrong foreground application.

Expose snapshots and sequenced lifecycle events with freshness/source information. External consumers choose when to act and reconcile after gaps. Event delivery is not authorization to execute external policy inside amux.

Configuration launch mechanics can remain in amux; cross-agent workflow policy stays external. A coordinator or project-status view supplied by an orchestrator may integrate with amux without becoming a built-in scheduler.

### Revised readiness interpretation

Absence of a durable task ledger, autonomous coordinator, verifier or merge controller is **not an amux deficiency under this scope**. Those belong to the external orchestrator.

Terminal compatibility, reliable launch/readiness, bounded resources, config ownership, workspace lifecycle safety and dependable interaction/state APIs remain substrate concerns.

The current memory-only runtime rule remains in effect. External orchestration owns durable task history. No restoration, generic plugin framework, container/VM/Kubernetes backend or ACP implementation was added by this clarification.

Next: elaborate the local workspace/execution/state contracts and their failure behavior before planning implementation. The earlier amux-owned orchestration-ledger proposal is not the next step.

Documentation-only clarification; no new runtime verification is claimed.

## Verdict and resumed gate

- **Local supervised terminal use:** credible opt-in daily-use candidate on the previously verified macOS/OMP combination, with explicit loss and compatibility limits.
- **Full Herdr replacement:** not established. Installed Herdr exposes managed worktree create/open/remove, richer agent commands, panes, sessions and remote surfaces. amux intentionally omits several of these; parity should be defined against the user's actual workflows, not a claim of universal replacement.
- **Unattended agent control plane:** not ready. Tasks, attempts, owned worktrees, verified results, integration authority and restart reconciliation are absent. Product feedback alone cannot supply these missing contracts.
- `survey-context` matches recorded branch and completed e01/e02 stories. `orchestrate-project --mode standard --resume` reaches recorded phase 6; no release/tag/merge/push was requested or performed. No gate is reset or automatically waived. New orchestration scope remains at problem elaboration, not approved Build.
- e01 review cap remains exhausted with no post-fix independent PASS. e02's PASS covers the standalone showcase only. Existing open origin-CPR/ZWJ bugs remain open; their accepted observations were not rerun.
- Repository timing/YAML helper scripts are absent. No claim of running them. Existing untracked `script` and `terminal-showcase-report.json` were left untouched.

Sources: [state](../state.yaml), [scope](../product/SCOPE_LATEST.yaml), [daily-use checkpoint](daily-use-checkpoint.md), [bug handoff](../bugs/BUG-showcase-endpoint-compatibility.md).

## Current primitives versus required contracts

| Domain | Implemented | Missing for the requested workflow |
|---|---|---|
| Project | Saved UUID, name/context, hooks/recipes; unsaved project | Coordinator assignment, explicit orchestration policy; preserve identity even for unnamed projects |
| Workspace/worktree | Terminal cwd plus asynchronous Git/worktree metadata | Independently owned workspace, worktree create/adopt/retire, branch/base reservation, safe dirty/busy cleanup |
| Agent | OMP 18.8.7 root/child lifecycle, lease, conversation/generation observations | Harness-independent submit/result/cancel contract and task/attempt identity |
| Execution | Shell-source UUID, native prompt guards, accepted/completed shell status | Durable task/run/attempt, acceptance criteria, evidence and retry lineage |
| Monitoring | Snapshot, bounded events, explicit gaps, terminal tree | Task/gate/integration read model and coordinator inbox |
| Integration | No first-class workflow | Candidate/base-bound verification, authorization, serialized landing, conflict and target-movement handling |
| Configuration | Global/project hooks and command recipes | Resolved/versioned agent profile and capability report; no silent policy downgrade |

Evidence: `src/projects.rs:33-113,283-465,764-999`; `src/daemon.rs:38-55,132-155,232-440,466-568`; `src/integration.rs:349-491`; `integrations/omp/amux.ts:7-81`; `src/tui.rs:399-475`.

Keep organizational terminal parent, OS process ancestry, agent parent and task dependency distinct. `worktree_id` is currently a canonical Git directory, not a managed workspace identity. `isolated:true` is not a security guarantee.

## Obvious reliability improvements

Priorities below describe adoption significance, not vulnerability severity. Static risks are not presented as newly observed runtime failures.

| Priority | Evidence/status | Improvement |
|---|---|---|
| Before unconditional terminal replacement | **Known defects:** origin-mode CPR and joined-emoji cursor width; [accepted evidence](../bugs/BUG-showcase-endpoint-compatibility.md) | Fix against independent terminal expectations; preserve CJK, combining, mode transitions and native interaction |
| Before autonomous spawning | **Source-confirmed gap:** no aggregate terminal admission cap; per-terminal history/threads accumulate; close retains records (`src/daemon.rs:232-371,718-754`; `src/runtime.rs:255-358`) | Bound active workers, retained terminal records/screens and total resource policy; reject overload explicitly |
| Before autonomous spawning | **Source-confirmed gap:** launch-failure and hook-enqueue-failure diagnostics append without the 20-entry pruning used only after hook completion (`src/daemon.rs:225-228,397-401,1125-1131`) | One consistently bounded diagnostics insertion path; avoid retaining sensitive launch commands unnecessarily |
| Before multiple daemon/config writers | **Runtime reproduced:** different private sockets sharing one configuration silently overwrite unrelated updates | Establish a single configuration writer or explicit config isolation/concurrency detection; `--socket` alone does not isolate config |
| Before unattended operation | **Source-confirmed ordering risk:** socket binds before initialization; client treats connect as readiness (`src/daemon.rs:1067-1071,1138`; `src/client.rs:125-154`) | Versioned API readiness handshake rather than socket existence/connectability |
| Before bounded shutdown claims | **Source-confirmed limitation:** running hooks watch timeout, not shutdown; allowed timeout reaches 300 seconds and shutdown joins the hook worker (`src/projects.rs:132-134,508-543`; `src/daemon.rs:1104-1105,1264-1267`) | Cancel owned active hook process groups during shutdown, not just pending jobs |
| Before durable-save claims across host failure | **Static durability risk:** file sync then rename without parent-directory sync (`src/projects.rs:445-457`) | Define power-loss guarantee; sync containing directory if required |

Existing strengths: independent PTY processing, capped client connections/frames/deadlines, bounded input and event replay, native partial/busy guards, generation-aware OMP settlement, process birth-identity checks and descendant cleanup. Current code has concrete regression coverage for concurrency and slow-client paths; this assessment did not rerun that suite.

Operational limits remain material: daemon state is memory-only; graceful shutdown/reopening is not crash recovery. SIGKILL cleanup, reboot continuation, Linux runtime, Rust-minimum compatibility and newer OMP versions are not proven by this session. Same-user socket authority includes other terminals/config/shutdown. Worktrees separate checkouts, not privileges. Exported shell context can contain credentials (`README.md:166`; architecture ownership section).

## Proposed ownership boundary — not yet approved

**One central product/CLI, separate state owners.** Do not turn terminal handles into tasks or inference traces into workflow state.

- **amux terminal runtime:** PTYs, native terminal emulation, process cleanup, attach/detach, local input, transient observations.
- **Orchestration layer:** project/workspace ownership, task/run/attempt ledger, dispatch, inbox, cancellation, gates and integration authority. Recommend a separately owned durable ledger within the product; this requires explicitly revising the current memory-only scope. Durable task history does not imply restoring PTYs or shell interpreter state.
- **ACP client/adapters:** negotiate capabilities, start sessions, submit/cancel prompts, collect updates and mediate supported permissions. Terminal execution remains a valid initial backend; do not build orchestration by parsing screen text.
- **Harness:** model/tool execution loop and harness-specific behavior. Third-party support must not depend on replacing its internals.
- **infer:** inference routing, provider credentials at the gateway, request/response traces and exports. It should not own task success, worktrees, dispatch or merge state.

infer already persists exchanges and returns `x-infer-trace-id` (`../infer/src/proxy.rs:769-770`; `../infer/src/trace.rs:69-77,183-207`). It has no first-class project/task/attempt correlation fields in `TraceStart`. Propose an explicit project/task/attempt/session-to-many-inference-traces link; propagate only through adapters that actually support headers/metadata and verify end to end. Response-header availability does not mean every harness exposes it. Routing all third-party harness traffic through infer is not established by ACP.

Treat correlation as sensitive metadata. infer currently forwards non-filtered custom headers upstream (`../infer/src/proxy.rs:302-341`); do not assume internal correlation IDs remain local. Do not copy raw shell environments, prompts or trace bodies into dashboard events. infer's documented traces are sensitive, lack automatic retention and are not a verified training dataset (`../infer/README.md:144-154`).

## ACP: adopt existing contracts; add only the missing policy

The assumption that there is no configuration standard is too broad. Current primary documentation provides:

| Existing contract | What it supplies | What it does not establish |
|---|---|---|
| [Session setup](https://agentclientprotocol.com/protocol/v1/session-setup) | Session identity, cwd, supplied MCP servers; capability-gated load/resume | Universal persistence, provider routing or your task model |
| [Session config options](https://agentclientprotocol.com/protocol/v1/session-config-options) | Agent-advertised model/mode/reasoning selectors and updates | Uniform options or semantic enforcement across all harnesses |
| [Tool calls/permissions](https://agentclientprotocol.com/protocol/v1/tool-calls) | Tool reports and permission requests a client can answer | Universal enumeration/removal of built-in tools; tools need not request permission |
| [Registry format](https://github.com/agentclientprotocol/registry/blob/main/FORMAT.md) | Agent distribution, versions, launch arguments/environment and platform targets | Effective project execution policy or verification/merge workflow |

Recommend an **amux AgentProfile**, not a new competing wire protocol: versioned harness/distribution reference, desired model, instruction references, MCP set, credentials by reference, workspace/role, budgets and tool/orchestration policy. Define user/project/role/run precedence; snapshot resolved values and adapter version per attempt. Separate desired, applied, unsupported and unverified settings. Provider model aliases and upstream availability can change; a model string is not a reproducibility guarantee.

For required unsupported controls, refuse a managed-policy launch rather than silently broadening authority. Native/default launches can remain available with an explicit unmanaged-capability label. Never rewrite global upstream configuration to satisfy one run.

Third-party native mode preserves upstream behavior. Managed mode should own cross-workspace scheduling in amux; disable native subagent/orchestration features only where a harness supports doing so. Otherwise expose upstream-owned nesting and do not claim exclusive scheduling. A prompt asking the agent not to use a tool is not enforcement. Revoking supplied MCP tools does not revoke built-in shell/file/network capabilities. Strong restrictions require supported harness hooks or an OS boundary.

ACP is agent interoperability, not a guarantee of all models or identical tools. Local stdio ACP is not inherently a remote protocol, but ACP itself is explicitly deferred in current scope. No scope/convention was changed here.

### Prior Art decision

- **Adopt** ACP session/capability/configuration contracts and registry distribution metadata rather than reinventing them.
- **Compose** infer as the inference-data owner, with verified adapter-specific routing/correlation.
- **Evaluate compose before building:** [Gas City](https://github.com/gastownhall/gascity) already documents runtime providers including ACP and Herdr, work routing, mail, waits, a desired-state controller and declarative configuration. This is real overlap with the requested product, not evidence that it satisfies this user's reliability or UX requirements. Its README was inspected; it was not installed, executed or adopted.
- **Build/extend only after scope approval:** amux's project UX, owned-workspace lifecycle and experimentable orchestration policy. Keep policy versioned so autoresearch changes do not mutate an in-flight attempt's contract. Do not add a generic plugin framework to obtain this boundary.

## Coordinator, workers and integration

Recommended default per named or unnamed project: one logical coordinator, created lazily on first interaction and resumed where supported. Stable project identity must not depend on display name. The coordinator can answer, inspect and propose dispatches; a deterministic controller owns state transitions and resource limits.

Move implementation, tests, review and lengthy integration to worker attempts. Do not give the coordinator a synchronous wait loop over workers. Completion notifications enter a bounded inbox. Separate worker processes do not alone make the coordinator responsive: choose short scheduling turns, queued feedback and explicit interrupt/cancel behavior for a long model turn. One active coordinator owner/lease avoids duplicate dispatch when multiple clients attach.

Every implementation worker gets an owned worktree and explicit base/candidate identities. A worktree does not block shell commands from editing the primary checkout or refs. If protecting the default branch is a safety requirement, enforce mutation authority outside the agent, not only in a role prompt. The coordinator's home can remain the primary checkout, but implementation should not run there.

Settlement must lead to result inspection/verification, not automatic success. Bind verifier evidence to exact candidate and base commits. Serialize landing per repository; if the target moves, revalidate the integration candidate. Conflicts, rejected approval and failed checks remain inspectable states. Do not remove dirty, busy, unowned or unintegrated worktrees. Retry only with explicit attempt identity and reconciliation; never infer exactly-once side effects from a prompt acknowledgement.

A project dashboard should be a read model over this ledger and live observations: tasks/dependencies, coordinator/worker availability, worktree/branch/base, blocked questions, last verified commit, verification and merge state, and permitted trace links. Chat/feedback writes to the inbox; dashboard rendering does not become a second scheduler. Use the existing CLI/snapshot surface first; a network dashboard would require separately approved exposure/security scope.

## Smallest closed workflow and adoption order

1. **Terminal reliability gate:** fix known compatibility defects and the high-value launch/resource/config gaps above; define supported platform/harness versions and automate regression gates. No CI configuration was found. Existing review restrictions remain until explicitly addressed; this report is not a release waiver.
2. **Managed workspace/task vertical slice:** one project coordinator dispatches one explicit task to one owned-worktree worker; worker reports a typed result; a declared verifier checks the exact candidate; user approves integration; controller lands or records a conflict; worktree is safely retained/retired. Show it through the current local API/UI. Cover failure paths in the same slice, not as an unspecified later reliability layer.
3. **ACP/profile vertical slice:** run the same contract through one real third-party ACP harness, then a second to expose false portability assumptions. Compare effective capabilities, cancellation, permission, resume and infer routing. Keep native terminal launch available.
4. **Autonomy and richer presentation:** enable unattended landing only for explicitly authorized policy after reconciliation and candidate-bound checks are proven; then richer dashboard and policy experiments. A first-party harness is not prerequisite.

Acceptance examples for the proposed task slice: duplicate dispatch has one owner; cancellation never merges; stale settlement cannot finish another attempt; failed/missing verifier cannot authorize merge; target movement invalidates stale evidence; dirty/busy worktree is retained; coordinator receives feedback while a worker runs; daemon/controller restart yields explicit unknown/interrupted/reconciled work, never fabricated success or blind duplicate launch.

Next skill: `elaborate-spec` to confirm durable-ledger ownership and the first closed workflow, then `scope-work` / `slice-tasks` / `plan-work`. No new cycle, scope file, epic or implementation was silently created.

## Fresh verification evidence

`verify: cargo build --workspace` — passed (dev build, 0.17s reported by Cargo).

Actual built CLI/private-daemon scenarios, not test doubles:

| Scenario | Observed result |
|---|---|
| Confirmed shell-source execution | Accepted execution UUID; shell returned ready |
| Child context | Confirmed source; cwd inherited; native child printed `CHILD_MARKER=preserved` from exported parent value |
| Partially typed native line | Exit 1, JSON error `partial-input`; submission rejected |
| Foreground `sleep 2` | Exit 1, JSON error `busy`; submission rejected |
| Agent never observed | `activity:unknown`, `settled:false`, reason `not-observed` |
| External detach | Existing terminal UUID set retained; this exercises the daemon detach command, not visual TUI reattachment |
| Explicit shutdown | Exit 0; both tracked shell PIDs absent and socket removed |
| Fresh daemon | Saved project UUID retained; old terminal UUIDs absent; project-open succeeded with a new shell workflow |
| Two sockets, one config | Daemon A saved prefix `ctrl-a`; daemon B changed only projects_dir; disk prefix reverted to B's stale `ctrl-b` |

The shared-config reproduction used two daemons with distinct private runtime directories and the same private `XDG_CONFIG_HOME`; A's global-config write preceded B's unrelated write. Both commands succeeded. It demonstrates lost updates, not malformed JSON. Configuration path follows XDG_CONFIG_HOME independently of socket (`src/client.rs:86-93`); each store serializes its own full snapshot (`src/projects.rs:429-440`). Workaround: one writer or separate config homes until ownership is explicit.

All smoke processes and fixture directories were stopped/removed. The user's live/default daemon, configuration and Herdr sessions were untouched. A smoke assertion initially assumed errors were printed on stderr; the actual CLI correctly returned its documented JSON envelope on stdout. The assertion was corrected without changing product code.

`herdr --help`, `herdr worktree --help`, `herdr agent --help`, `herdr workspace --help`, and built `amux --help` were executed read-only. No Herdr workspace or agent was started/changed. No fresh live inference, ACP session, crash recovery, GUI/TUI visual verification, full test-suite, Clippy, Linux or new independent-review PASS is claimed.
