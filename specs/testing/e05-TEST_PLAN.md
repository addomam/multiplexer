---
type: test-plan
context: specs/epics/e05-thin-tool-integration/context.yaml
---

# e05 test architecture — planned, no runtime PASS

P0 integration change to authoritative structured broker, process lifetime and human editor. Reuse existing real-daemon/private-root/controlling-PTY conventions, not a second fixture ownership framework. Stories §17 and task ledgers are the scenario/case manifest; exact names in the new `tool_integration` target, one selected case must actually execute. A missing case or zero-case filter fails. No tests/source are created by this planning packet.

## Layers and proof obligations

| Layer | Purpose | Evidence required |
|---|---|---|
| Existing ACP/control/native regression targets | Shared-core refactor preserves delivered behavior, force/cancel/conditional distinctions and native unknown safe-grant guard | Actual changed-source default/workspace results, original failures preserved; no old source PASS relabelled |
| New `tests/tool_integration.rs` and focused submodules | Uncertain consumer-visible identities, scope/winners, bounds, stalled/escaped lifetime, stale native input | §17 exact named cases; explicit barriers rather than sleeps for race ordering; real stdio peer/state/file effects |
| Independent Python reference integration | Language-neutral author path, one real accumulator and genuinely permission-gated ledger | Execute public executable, no ACP/internal broker import; original PID/session and exact causal effects on both surfaces |
| Actual CLI/API/controlling-PTY smoke | User-visible launch/attach/handoff/draft correction/inert rendering and cleanup | Run built program outside unit-test-only proof; observe native screen, actual original responder completion and absence of stale side effects |
| Pinned OMP SDK callback/adapter checks | Verify public hooks, option/custom-answer mapping, once/deny and withdrawal/teardown | Network-denied private environment for offline work; callback tests do not claim real provider compatibility |
| Opt-in actual OMP model acceptance | Real same AgentSession produces genuine questions/permissions and resumes original work on both surfaces | Separate exact approval, actual model/effort/closure/call/spend receipts; <=8 cumulative prompts, bounded duration, no silent retries/fallback; original session and cleanup |

## Named execution

Future commands (NOT run or accepted):

- `cargo test --test tool_integration e05s01_ -- --nocapture --test-threads=1`
- `cargo test --test tool_integration e05s02_ -- --nocapture --test-threads=1`
- `cargo test --test tool_integration e05s03_ -- --nocapture --test-threads=1`
- `cargo test --test tool_integration e05s04_ -- --nocapture --test-threads=1`
- Separately authorized only: `cargo test --test tool_integration e05s04_live_omp_adapter -- --ignored --exact --nocapture --test-threads=1`

Before execution list exact case names and assert every ledger expected case is present; after execution assert selected counts/nonignored dispositions. The ignored live case cannot be counted as a pass or accidentally included in a broad ignored suite. Define the live case at the top-level test module so its declared `--exact` name is valid; if module placement changes, migrate the manifest/command together before running. Adapter-local package scripts are introduced during Build and must actually typecheck/test supported pinned hooks; do not invent a command/package manifest that doesn't exist yet.

Full gate: `cargo test --workspace -- --test-threads=1`, default `cargo test -- --test-threads=1`, strict workspace/default Clippy/build, workspace check/fmt; actual existing OMP lifecycle tests, Zsh syntax and independent showcase test from repo manifests/landing receipts. Sequential suite scheduling prevents unrelated openpty exhaustion but race cases must retain internal concurrency. No production PTY fix inferred from test resource scheduling. No CI/publish setup in scope.

## Important boundaries

- Test both commit/revoke and response/withdrawal orders at controlled barriers; follow through to downstream state, not call counts/mock echoes.
- Test exact live target/protocol/session/direction-qualified ID/ownership generation/scope/revision; wrong-kind and stale input must have no target effect. Same generated values copied between wrappers are not tests.
- Force/cancel/conditional require original pending question and prompt still live; force cannot send interrupt/close and ordinary pending cancel grants nobody. Explicit interrupt confirmation comes only from exact prompt cancelled outcome.
- Input/output/frame/retained/escaped snapshot/answer budget boundaries use actual representable data, not incidental constants-only assertions. Reserved reply/interrupt/close capacity and lifetime-ID exhaustion must still permit required terminal progress. Native definite rejection must allow deliberate correction/cancel while ambiguous lost reply remains fail-closed.
- `e05s04_omp_approval_correlation` exercises pinned real SDK wrapper/runner, two overlapping genuine approval IDs, explicit isolated always-ask/no-override settings, deny-first/allow-second actual private file effects and fail-closed timeout/cancel/late continuations. Normative detailed profile: `../contracts/omp-tool-adapter.md`. No SDK provider bootstrap/auth discovery; network-denied private process. Test-only SDK timeout hook restored; not a shipped adapter setting or real-model proof.
- Hostile output includes OSC/CSI/title/clipboard/control/bidi content; prove actual host-terminal effects absent with safe canonical text/gap/category observations, not merely source sanitization.
- Own private daemon/socket/project/HOME/provider cache roots. Observe original pipe endpoints, child/descendant process births and worker joins **before** fixture fallback; normal cleanup must not use fallback as its own proof. Hold duplicate/escaped endpoints and orphaned descendants to ensure leader death alone never clears charge. Keep an unrelated live sentinel and prove it survives. Never scan/read unrelated credentials.
- Real SDK init may perform provider warming; offline callbacks must use OS network denial, private HOME and inert injected tool/model seams without claiming a model run. If SDK cannot be isolated, do not bootstrap it. No private auth copying or network-enabled SDK start without explicit live approval.
- Low-cost live harness must fail closed if a requested model/effort/call/spend cap cannot be enforced, a provider fails, genuine responder isn't exposed, or cleanup is unknown. Preserve failure row; no prompt-language pseudo-question, menu-text permission, premium fallback or universal-compatibility claim.

## Test quality and evidence

Permanent tests cover plausible observable bugs/invariants/transitions/errors. No wording/source-text/copied-schema/default/wiring/nonempty/mock-echo tests. Straightforward launch/help/forwarding checks belong in throwaway smoke, removed after actual observations. Deterministic isolated cases must be full-suite-safe; no global daemon/config/auth changes. Case fixture code is not a substitute for an independent public consumer.

Each build report records exact source identity, actual command, exact selected names/counts, runtime effects, expected negative effects absent, cleanup classification and redacted provenance. Logs must omit prompts/answers/transcript/credentials/env/auth paths; private detailed artifacts are removed after positive cleanup. Historical source and caps remain immutable. Model run proof applies only to its actual source. No e05 story is done while an acceptance row, required real-model authorization/proof or cleanup obligation is missing; native e03s08 failure remains unchanged.
