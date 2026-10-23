# e03 post-ownership parallel Build tracks

Activate ONLY after parent verifies e03s01–02 contracts. User authorized approved Build, not live application/model runs, global daemon/config/auth changes, commits/merge/push/release. Historical review caps remain unchanged. Strong reasoning models for both tracks (ownership races and provider identity); use cheaper agents only for substantial bounded mechanical/documentation slices, not task padding.

## Isolation and integration

Parent is the single integration owner of `src/daemon.rs`, `src/client.rs`, `src/cli.rs`, `src/runtime.rs`, `src/runtime/compatibility.rs`, `src/lib.rs`, `Cargo.toml`, `Cargo.lock`, existing README/CHANGELOG and all state/task/evidence ledgers. No worker writes those files. Send exact APIs/required routing instructions in the result; parent implements the shared changes sequentially.

Each track runs in its own herdr tab, fresh OMP session and feature worktree, seeded with parent's verified uncommitted ownership implementation. No commits authorized. Parent retains a seed manifest and only copies worker-owned files, preserving all unmerged work until integration and smoke succeed. Neither track may overwrite or reformat shared files. Skip build/tests/lint/formatters mid-flight; parent runs integration checks after applying both results. Workers may inspect/generate installed Codex schema offline in private temporary roots; no live backend/model check, real auth copy, or frontend launch without separate user approval.

## Native track e03s03–04

Own `src/control.rs`, `src/tui/control.rs`, `src/tui.rs`, `tests/pty_control.rs` and a new native controlling-PTY behavior test if needed. Preserve existing Authority/Registry APIs; conditional extension stays inside control. No daemon/client routing edits. Implement full native request/wait/cancel/deliberate-force UI and presentation and idle/background conditional keep/withdraw/accept races exactly as approved story specs, task ledgers, pty-control contract/schema and native UX doc. Keep one native surface, three header rows, no footer/body geometry change. Initial focus remains unknown; original persistent connection provenance, strictly increasing view serials, disconnected/unknown facts block unsafe eligibility. Keep/withdraw preserves same human control; external policy decides when to re-request. All app routes gated and no input replay. Labels sanitized/clipped and no private handles rendered. Do not weaken ordinary-cancel or retained-call behavior. Parent owns actual native surface proof and shared focus/protocol integration.

## Codex track e03s05–07

Own `src/codex.rs`, `tests/agent_interaction.rs`, `tests/codex_native_smoke.rs` and any required concrete Codex frontend launch file under `integrations/codex/`. No existing shell integration changes without parent coordination. Implement the real concrete same-instance Unix WebSocket backend, native-first identity discovery with zero hidden setup turn, typed start/steer, genuine question answer, request-only allow/deny, exact-turn explicit interrupt, truthful lifecycle/degradation, bounded outcomes/tombstones and owned-resource cleanup. Preserve string versus integer provider IDs and complete instance/session/turn/item/request identity. No generic provider abstraction/pass-through, screen-derived target, fake success, stale retarget, task-success receipt or implicit cancellation.

Propose a small concrete module API for parent daemon creation/routing/events/cleanup and generated-schema fixtures. Each mutation takes the current `Arc<control::Authority>`, registers `forward(call_id,"agent-mutation")` at actual dispatch before I/O, rechecks queued dispatch, and settles the exact call applied/rejected/unknown without mutating owner. No topology/authority locks across provider I/O. Shared control metadata remains in control; module exposes public/redacted events and inspection facts, never private auth/socket capabilities.

The approved tungstenite dependency is exactly 0.30.0, default-features=false, features=["handshake"]. Parent adds dependency after reported imports/API. Installed 0.160.0 generated schema is required authority. Opt-in live tests are ignored by default, exact-name and double env guarded with explicit model, <=1800s overall/15s daemon/30s backend/180s response, <=6 meaningful model turns, private0700 roots and only-owned process/group/socket/auth cleanup. Implement real test bodies, not skipped/mocked success, but DO NOT RUN them. Missing prerequisites in a selected live test must fail visibly.

## Final integration

Both tracks integrate before e03s08. Native-buffer/application fence is revisited only at complete-broker validation; unknown remains settlement-unavailable and blocks safe grant/first acceptance. Force-only does not pass. Workers do not mark any task/story passing, claim runtime acceptance, reset review cap, or discard their worktrees/tabs.

## Concrete Codex integration interface

Parent will integrate this private, concrete API; implementation details inside the module remain with the Codex owner:

```rust
pub(crate) struct Broker;
pub(crate) struct Prepared {
    pub instance_id: String,
    pub command: String,
    pub env: std::collections::BTreeMap<String, String>,
}
impl Broker {
    pub fn new(runtime: &std::path::Path) -> anyhow::Result<Self>;
    pub fn prepare(&self, request: &serde_json::Value) -> anyhow::Result<Prepared>;
    pub fn bind(&self, instance: &str, terminal: &str) -> anyhow::Result<()>;
    pub fn inspect(&self, request: &serde_json::Value) -> anyhow::Result<serde_json::Value>;
    pub fn mutate(&self, op: &str, request: &serde_json::Value,
        authority: std::sync::Arc<crate::control::Authority>) -> anyhow::Result<serde_json::Value>;
    pub fn launch_failed(&self, terminal: &str, message: &str);
    pub fn end_terminal(&self, terminal: &str);
    pub fn shutdown(&self);
    pub fn take_events(&self) -> Vec<(String, String, serde_json::Value)>;
}
```

The declaration above is an interface contract, not source scaffolding or implementation. `prepare` validates the closed agent-open shape and explicit model/resume selection, owns private backend resources and returns the real frontend command/environment, with no bootstrap turn. Parent first validates the controller credential, then creates the PTY with that caller's initial control (never an automatically retired private launch controller); binds instance/terminal before spawn, extends only the child environment, and accounts shell startup with that same authority. Failure and terminal-end call cleanup; backend failure remains truthful in inspect. `agent-open` returns `{terminal, instance_id, control, state}`: terminal is the ordinary terminal object; control is the complete issuance Grant; state is current agent-inspect output. Both ordinary terminal state/snapshot and agent-inspect expose authoritative capability/lifecycle facts without private credentials.

`inspect` accepts agent-inspect with exact terminal and expected instance_id. Mutations accept the full approved closed request and the already-authorized per-PTY authority. Return an inspectable retained call receipt, not completion. Module owns all exact provider identity/turn/request registries and live-instance validation before any dispatch. `bind` can attach a just-prepared instance; shutdown also cleans unbound prepare failures. No `abort` or generic provider API is needed. Event tuples contain kind, exact terminal ID and public/redacted data; parent supplies the existing global sequence cursor after draining, never under provider I/O.

`Prepared` is an RAII lease: dropping it before successful bind cleans only that prepared instance; dropping it after bind leaves the live bound instance owned by Broker. Additional private fields are allowed. Parent borrows public fields rather than partially moving a Drop value. A failed open must never call global broker shutdown and terminate unrelated instances.

`Prepared.command` is shell-escaped executable plus arguments, without `exec`/`env` prefixes. `Prepared.env` contains only frontend overrides and no secret values; parent runs `exec env KEY=quoted_value ... command` in the startup source, not overrides on the stock shell environment. This prevents private Codex environment contamination of captured shell context; frontend exit ends its PTY lifetime. Parent redacts managed frontend source in public diagnostics. `launch_failed` records native-start failure without global cleanup or implied success. `Broker::new` only prepares local in-memory/private-root state and never invokes Codex or needs installation/auth. `inspect` is in-memory observation only: no provider I/O, including no wait on a mutex held across provider I/O.

## Integrated follow-up ownership

After initial track integration, parent exclusively owns `tests/codex_native_smoke.rs` for the approved final complete-broker entrypoint. The Codex follow-up session owns only `src/codex.rs`, `tests/agent_interaction.rs` and its result report; native follow-up ownership is unchanged. Both sessions skip builds/tests/lint/formatters; parent verifies after applying their results. Native follow-up addresses observed controlling-PTY/conditional failures. Codex follow-up binds backend/version/native launch to the same absolute executable and preserves common persistent framing without loosening closed nested schemas. These are Build fixes, not authorization for a live model check or acceptance claim.

## Subsequent parent live-check authorization

After the worker implementation/fix rounds, the user separately authorized the four exact real Codex checks and relevant installed harnesses with real authentication and cheapest compatible models, preferring free/local inference and excluding sol. The earlier worker prohibition remains historical and was not permission for workers to start models. Parent owns all subsequent integration edits and live diagnosis. That live-check authorization did not authorize a new translation/provider layer, subscription purchase, global daemon/config/auth change, commit, merge, push or release.

Both tracks are integrated into the parent feature worktree. Native e03s03–04 passed their recorded 29-case controlling-PTY proof; e03s05–07 passed their separate exact live checks and deterministic protocol gates. e03s08 remains failing on unknown application settlement. Plan approval and historical worker reports are not complete-feature acceptance. The completed worker workspaces, tabs and owned processes were closed, and their worktrees retired only after byte-verified full nonignored source preservation; see `specs/verifications/e03-worker-retirement.json`. At that Build handoff, parent integration remained uncommitted.

The user subsequently requested commit, merge and push. Feature-branch commit and normal push are authorized; failed complete-broker acceptance and absent independent Build PASS still block merge. No release or historical review-cap reset is authorized. See `specs/verifications/e03-integration-build.md`.
