---
type: feat
context: specs/epics/e04-acp-agent-interaction/context.yaml
story_id: e04s01
scope: e04s01-local-conversation-transport
status: pass-with-verified-post-review-P2-repair
---

# e04s01 source self-audit

Round5 meets the approved AND gate96.875/100, zero must-fix; raw A incorrect/P2 report and all earlier rounds remain unchanged. The sole nonblocking invalid cancel-time permission responder defect was reproduced, repaired and verified through actual CLI and workspace343/strict gates. This is a parent post-repair audit, not a new independent PASS or sixth round. S01 local acceptance passes; later stories, live compatibility and publication remain gated.

## Evidence and priority

Affected-source 90-day churn: tui.rs 7, daemon.rs 4, tui/view.rs 3, cli.rs/lib.rs 2 each, control.rs/tui/control.rs 1 each; new broker/editor have no prior churn. Review started with shared routing and admission, then the new protocol and human-view boundaries.

Historical pre-round2 verification: `artifact://995` — 7 ACP library cases,335 workspace tests/30suites/10ignored and strict Clippy/build/fmt. Exact prior s01 command `artifact://1002` passed7 library/20 s01/57 legacy,1 legacy ignored. Current complete verification is `artifact://1080`:11 ACP library,21 s01,340 workspace/30suites/10ignored, strict workspace Clippy/all-target check/build/fmt. OMP8/49 and Zsh syntax passed.

Actual CLI/controlling-PTY proof: `e04s01-cli-pty-smoke.json`. One peer/session, API total42 then human increment15/total57, paste local until Enter, takeover clears the captured draft and stale controlled status, observer paste/Enter creates no work, original native terminal IDs preserved, resized ACP/picker exercised, terminal modes restored under a persistent private controlling host, foreground TUI/host/daemon exit0, owned peer absent before fixture fallback. The resized capture may end before all diff-rendered body writes; complete original-size owner/observer frames and actual application effects are separately retained. No full resized-body visual claim.

## Checklist

- PASS correctness: exact target/instance/connection/session and captured generation are checked inside the same authority→ACP memory transaction that publishes immutable accepted work. A later owner lookup does not discard accepted work. Duplicate/busy/stale inputs have no second effect. No provider-quiescence claim.
- PASS identities/lifecycle: pending RPCs are registered before writer visibility; asynchronous initialize/newSession/auth/prompt do not block the FIFO consumer. Force/cancel preserve accepted work and original session/PID; only explicit interrupt requests cancellation, with confirmation taken from the original prompt result.
- PASS lock order: topology descriptors are copied before projection; no pipe/process/wait/join under authority or ACP state. FIFO admission is target→ACP state, not ACP→authority. Owned process/tree cleanup and worker joins occur outside those locks.
- PASS resources: frame cap precedes accumulation; count/byte charges cover reserved and in-flight messages; capped stderr continues draining; history eviction exposes gaps without silently evicting live work; exact lifetime call tombstones retain a final close slot. Unknown capture cleanup is incomplete, retained and charged.
- PASS process safety: captured trees use existing birth-qualified freeze/rescan. The uncaptured fallback signals only the unreaped owned setsid group, then reaps, never signalling after reap. The direct fallback test observes descendant disappearance and an unrelated live sentinel. Metadata/capacity fault tests cover an uncertain capture outcome; they do not claim an actual OS metadata outage was induced.
- PASS authorization/routing: every initially exposed mutation requires current control; original native connection provenance is passed from server context, not request data. ACP target IDs cannot act as native PTY selectors; active ACP key/paste/mouse/focus/prefix routes are consumed locally or converted into captured structured actions, never raw fallback/replay.
- PASS privacy/output safety: default snapshots/events/inspect exclude content; opt-in content is incremental/bounded; raw provider errors/auth/env are not projected. Labels/content/control sequences are inert Ratatui data. Secret-pattern scan of affected source and e04s01 evidence found no matching key material. This is a same-user local control environment, not a sandbox.
- PASS supply chain: official schema-only `agent-client-protocol-schema = =1.12.0`, default features disabled, is tagged [OK] in the approved story. No SDK runtime/executor, provider abstraction, remote daemon protocol or generic plugin system was added.
- PASS provenance/scope: approved Plan/ADR/integration contract and isolated RED c8808f3 retained; both worker commits remain in the pending feature integration merge. No main publication, global daemon/config/auth changes or new live checks. Original dirty checkout preserved apart from explicit resume pointers.
- PASS tests: real public daemon/CLI/PTY effects, exact immutable identities, race barriers, error outcomes, cap exhaustion, absence of stale side effects and independent reap/sentinel oracles. No new source-text/default/wording or mock-echo assertions. Per-function tests were not added merely to satisfy generic checklist wording; project/developer behavior-test rules take precedence.
- PASS maintainability: one canonical broker and separate identity-bound editor; shared authority/client conventions reused. Removed boxed job dispatch/vector allocation in favour of direct fixed thread handles. No dead compatibility aliases or obsolete forwarding routes added.
- PASS docs: existing README and changelog describe the actual first-slice boundary, explicit authority/lifecycle/privacy, unsupported form capability and unverified live compatibility. Build evidence retains failed attempts and corrected probe assumptions.

## Repairs and limitations retained

- Fallible fixture exchange returned a panic on EOF, causing double panic during race cleanup; return UnexpectedEof and retain all original race/reap assertions.
- Original finite flood could be processed without reaching a queue limit; the larger bounded burst actually fills the unread reply pipe/queue, preserving failure/responsiveness/reap oracles.
- Process-identity failure could falsely advertise cleanup-complete/free capacity after thread join; added failing-before/passing-after resource invariant and actual pinned-group cleanup test, with conservative unknown retention.
- Actual observer surface had a stale controlled status; invalidate it on loss of local authority, without reacquisition/replay.
- Early smoke probes assumed noncumulative arithmetic, no initial stock-Zsh terminal, and post-session-leader PTY validity on Darwin. Failed records are retained; the corrected final smoke ran under a persistent private host and passed.
- LSP symbols work, but isolated-worktree references still return empty despite known compiled callers, including after a file-specific reload. Reported through tool QA; no empty result was interpreted as no callers and no existing exported symbol was renamed. Compilation/exercised call sites are recorded separately, not mislabelled as reference-tool proof.
- Installed generic churn helper is not bundled here; actual scoped git history counts were used. Generic 300-line/20-line prescriptions are not project CONVENTIONS.md rules; the approved cohesive deep broker/editor and existing large routing modules were kept rather than introducing an unrelated module split.

Fowler smells considered: protocol identity data clumps are encapsulated by Target/Binding; boundary JSON selection is explicit rather than an independent ownership model. No actionable Middle Man, Feature Envy, Message Chains or Mysterious Name found. Lifecycle strings and large protocol match blocks remain review considerations, not asserted defects. No checklist omission is being rationalized as runtime acceptance.

## Post-resume verification failures

- `artifact://1014`: 7 ACP library and 20 s01 integration cases passed; native ownership target had 32 pass/2 fail, so chained Clippy/build did not run. Do not substitute the previous full workspace PASS for this result.
- The native callback framing gate inherited Darwin nonblocking accept. An actual isolated socket probe returned WouldBlock before the callback (2µs); switching the accepted stream to blocking delivered the callback byte. The gate now clears inherited nonblocking mode, as the existing daemon/consent gate already do. Both affected original settlement cases then passed individually, with their original timeouts/assertions.
- Native force eligibility across redraw is independently investigated against the recorded actual ANSI capture; no queued-before-visible, clipping or stale-observation assertion is weakened.
- Resumed actual CLI/TUI reached API42/human57, then exited1 on valid-timeout `setsockopt` EINVAL. An actual OS socketpair probe reproduces EINVAL with a valid35s timeout only after peer close; no zero-duration assumption or source workaround. Existing daemon documentation/code intentionally expires idle persistent clients after5s. The failed foreground smoke stopped draining the PTY across tool calls; a continuously drained experiment is required to discriminate output backpressure. The exact original blocking interval was not captured.
- The failed smoke's daemon0/host1 were reaped, owned peer observed absent and FDs closed; no fallback signals. Exact root had no lsof holders and was removed after preserving evidence.

## Repaired verification and review gate

- `artifact://1036`: original native force case passed1/1, then workspace335 across30 suites with10 ignored, strict workspace Clippy, all-target workspace check, workspace build and formatting passed.
- OMP `bun test`: 8 passed/49 assertions; Zsh syntax passed.
- `e04s01-resume-cli-pty-smoke.json`: actual built CLI + continuously drained controlling PTY; API42/human57 on one peer/session, paste local until Enter, deliberate driver idle6.2s survives, captured draft revoked, observer produces no work, canonical retained-work panel uses `settlement_boundary`, deliberate human force succeeds, native IDs unchanged, terminal modes restored, peer absent before fallback, TUI/host/daemon0, drainer joined/no error.
- Actual private-daemon idle-expiry probe reproduces valid35s timeout EINVAL after6.004s without a persistent request; daemon remains responsive. Original smoke's exact blocking interval was not recorded; do not overclaim its timing. Production deadline, client error handling and retry behavior remain unchanged.
- First failed root was removed after cleanup/no holders. Successful second root has no holders after all owned resources exited; removal follows saved evidence. Herdr worker sessions/worktrees remain preserved until verified integration commit.
- Source is ready for fresh blind A/B and security round2. Round1 infrastructure failure is retained as an incomplete attempt, not a fabricated verdict; maximum5 and old e03 counters are unchanged.

## Round2 review response

- Persistent `acp-open`: permanent real persistent-socket regression failed before repair with `invalid-request: unknown mutation field persistent`.
- Close/capture race: permanent transition regression failed before repair (`resource.failure` null rather than `process-identity`). Resource uncertainty must remain charged even after an earlier explicit close.
- Local busy rejection: actual private CLI/controlling-PTY probe reproduced draft loss while the first real prompt reply was gated; canonical history had only the first effect42. `e04s01-queue-draft-before.json` retains the frame and cleanup evidence, including a shutdown-side proxy BrokenPipe.
- Repairs return the original owned, unqueued request without copying its draft. Only the synchronous editor callback may restore that original still-matching authority; no rebinding or automatic retry.
- Retained-work outcome rendering now reads the canonical outcome object. Authentication completion is recorded before a failed follow-up session enqueue can fail the transport.
- Current complete verification and actual after-fix queue smoke passed; this permits independent review only, not story promotion.


## Round3 readiness and F.I.R.S.T quick gate

- PASS correctness/types: persistent open is a transport-only boolean; critical late cleanup failures remain charged; observed auth responses survive continuation enqueue failure; rollback is limited to empty, complete, ended reservations.
- PASS authority/privacy: rejection returns the original owned envelope; restoration checks original tuple/control/generation after grant reconciliation. Already-queued uncertainty is never restored/replayed. A scoped key-material scan found no matching patterns; current independent security review is still required.
- PASS performance/clarity: move the rejected request/draft rather than clone it; no new runtime, policy, transport layer, compatibility shim or I/O under a lock. Existing single broker/registry/editor boundaries are retained.
- PASS resources/lifecycle: new boundary regressions preserve live instances, admitted-close history and unknown cleanup under failed-open rollback. Actual queue-smoke peer/TUI/host/daemon/proxy/drainer cleanup completes before removing private roots.
- PASS documentation: changelog and concrete integration interface updated. Every round2 comment has an explicit fix/disposition; ended eviction remains explicit bounded removal, not a guaranteed final transcript projection.
- PASS F.I.R.S.T Fast/Independent/Self-Validating for `tests/acp.rs` and `src/acp.rs` boundary cases: bounded private daemon/roots, explicit frame/effect/identity/capacity assertions, no live auth/model and owned reap before fallback. Tests assert observable mutation/persistence and resource/history transitions, not copies or source text.
- PASS same quick criteria for `tests/pty_control/native.rs`: original actual geometry/deadline retained; improved failure evidence only. One earlier resize timeout remains logged with no established cause; the instrumented exact case and later full34-case target passed. This is not retroactive e03 acceptance.
- Red flags: generic per-function/line-count rules and missing bundled helper scripts are not project conventions; no padding tests or unrelated module split added. The native timeout is recorded rather than rationalized away. Complete-broker and real-adapter gates remain unproved.

## Late runtime observation during round3 freeze

`e04s01-queue-draft-after.json` / `frames.deliberate_second_effect` actually shows completed total57/count2 in the original session while chrome still says `busy: pending control RPC; action not queued` and the local notice says `not yet admitted`. The old rejection status survives a subsequent successful local enqueue, and a capture-time notice outlives canonical admission. Both code reviewers received the same new runtime evidence independently; source has not changed. Clear only superseded local status on a new successful queue admission and use a stable capture notice referring to canonical outcomes, never optimistic admitted/applied wording. Re-exercise the actual surface after correction before new review.

## Round3 response and current verification

- Shared stale-status finding: a new successful deliberate local enqueue clears its superseded local rejection; stable capture notice refers to canonical work without asserting admission. No late reply can erase a newer local error.
- Inspect-error finding: actual CLI/controlling-PTY fault reproduced unsent draft and transcript loss using a transparent proxy that forwards an intentionally stale session tuple to the real daemon. `e04s01-inspect-before.json` retains actual frames, no replay and independent peer reap/terminal-mode restoration/zero exits. `e04s01-inspect-after.json` proves retention through recovery and subsequent deliberate same-session effect57/count2 without replay.
- Preserve the last confirmed projection on transient inspect error, display the error separately, and rewind incremental history only when the actual target tuple changes. Current ownership reconciliation still invalidates stale captured authority.
- Late-startup finding: remove the meaningless release after peer reap and name the real integration scenario (responsive close while reply is held). Add a deterministic production-consumer race: a complete queued session response is dequeued while the target ordering point is held, then the lifetime ends before application. This is in-flight publication proof, not a fabricated late reply from a dead peer.
- Writer-budget consideration: keep the real in-flight charge boundary; delete manual decrement/success assertions that only restated the implementation.
- Label consideration: consistently sanitize remaining bidi/line-format marks in bounded picker/header and broker metadata; reject them in ACP display names. Original target identities are unchanged.
- Current `artifact://1113` passes12 ACP library/21 s01 integration; `artifact://1121` passes341 workspace/30suites/10ignored and strict Clippy/check/build/fmt. Bun8/49 and stock Zsh syntax pass. Actual queue-status proof asserts truthful completed header/footer as well as effects; actual CLI label boundary rejects residual format marks without a new target. Source is ready for round4, not story promotion.

## Round4 self-audit gate

- PASS correctness: failed reads cannot impersonate a new identity; actual tuple changes fetch full history, while original-grant reconciliation still invalidates drafts. No retry, fresh-authority substitution or optimistic canonical admission.
- PASS security/privacy: existing authority→broker lock order and all route gates remain unchanged. Residual nonprinting format marks are sanitized consistently; actual invalid-name admission fails without an extra target. Scoped key-pattern scan of affected source/new proof has no matches; fresh security4 is still required.
- PASS performance/clarity: one retained projection, cursor and separate local read-error string; no extra transport/thread/queue/copy or production I/O under locks. Status clearance is tied to a new deliberate local enqueue, not an unrelated late reply.
- PASS test quality: deterministic complete-frame/held-ordering-point race exercises the production consumer. The first memory-only test incorrectly expected worker completion (`artifact://1111`); current nonlive-closing/no-session assertion matches that boundary, while actual process integration still requires ended/reaped/sentinel outcomes. Removed manual-counter tautology; no source/wording tests added.
- PASS resources/evidence: actual inspect/queue PTY probes restore termios and independently reap peer/TUI/host/daemon, with no fallback signals; proxy/drainer joined and teardown BrokenPipe retained. Exact no-holder checks precede private-root removal. Label probe is explicitly CLI-only.
- PASS docs/scope: changelog and existing integration contract describe the actual read/requeue behavior; build evidence retains every failed attempt and current verification. Original dirty checkout and pending Herdr work remain preserved.
- Red flags/dispositions: no new third-party package; generic per-function/line-count rules do not override project behavior-test conventions. Native resize cause remains unestablished despite current full pass. Live compatibility, forms/permissions and complete-broker acceptance remain unproved. No skip is being relabelled acceptance.

## Final candidate self-audit

- PASS all prior correctness/security/performance/clarity checks remain applicable. Only source delta from round4 is the local inspect-error cache context: empty projection says no confirmed projection; existing cache says last confirmed projection. No copied draft, new authority path, transport policy, runtime/thread/dependency or blocking lock operation.
- PASS actual changed surface: first-read before/after captures prove the false claim and its correction; final same-view fault proof still retains draft/history and permits only deliberate same-session57/count2. Preserve identity reset on view selection rather than carrying another agent's binding/content.
- PASS final `artifact://1142`:341 workspace/30suites/10ignored and strict Clippy/check/build/fmt; Bun8/49 and Zsh syntax. Final source/capture key-pattern scan has no matches. All three new proof roots removed after reap/no-holder checks; temporary probe source removed.
- PASS evidence/gates: round4 full reports/scores and static security limits retained. Source5 manifest freezes the changed candidate. Maximum5, historical e03 failures/caps and separately required live/publication approval unchanged. No new permanent source/wording test; actual surface is the oracle.
- Red flag disposition: reviewer suggestion to avoid publishing null was not followed literally because a selected new agent cannot keep the old agent's actionable identity. The actual false retained-cache claim is fixed explicitly. Current review needs a new candidate verdict, not the prior PASS.

## Post-review P2 disposition and current gate

- Raw A5 has one should-fix and B5 none: fixed denominator32 gives96.875/100, both above94 with zero must-fix. That is the approved gate, not a new zero-should-fix rule. The preliminary precautionary human-decision hold is cleared by this exact rule; no historical report/counter is rewritten.
- Reproduced both malformed params and wrong-session permission cases: `artifact://1165`,0passed/2failed, original admitted prompt became unknown. Repair returns invalid_params for the known exact request ID through the existing reserved writer queue; valid current-session late permission remains Cancelled. Capacity failure still fails closed; no identity/authority/force/cancel semantics change.
- New permanent tests cover distinct decode and session-scope errors, original prompt cancellation, retained same-session follow-up57/count2, owned reap/workers and unrelated sentinel. Both pass after repair; the existing explicit-interrupt case also passes. No wiring/wording/default test, extra dependency, blocking operation or avoidable payload clone.
- Actual built CLI proof `e04s01-cancel-invalid-permission-cli.json` passes both cases outside tests: invalid_params, original cancelled response, same live session57/count2, native IDs preserved, owned peer absent before fallback, workers joined, sentinel alive, daemon0, no fallback signals, private root removed after exact no-holder check.
- Current `artifact://1171`:343 workspace/30suites/10ignored, including12 ACP library/23 S01/57 legacy; strict workspace Clippy/check/build/fmt. Bun8/49 and Zsh syntax pass. No UI source changed after the recorded actual PTY proofs.
- Source manifest `e04s01-post-review-source.json` distinguishes the verified repair from the reviewed round5 manifest. No independent review of this later delta, no round6, cap reset, new live auth/model run or ACP publication is claimed.

