# e03 implementation threat model

Scope: approved same-instance agent interaction and per-PTY capability ownership. Build and subsequently bounded real-harness checks with existing auth and cheapest compatible non-sol models are authorized; global daemon/config/auth changes, commit/merge/push/release are not. Parent remains the integration owner; implementation is isolated from the original checkout.

## Trust and assets

The private same-user Unix socket already permits process/config control. Capabilities coordinate trusted local callers; they do not sandbox malicious same-user processes. Protect live PTY identity, exclusive controller generations/reservations, unresolved call accounting, private handle references, provider identities and private Codex credentials/resources. Runtime ownership stays in memory.

## Surfaces and mitigations

| Surface | Risk | Required mitigation and proof |
|---|---|---|
| Capability JSON and handle methods | Forged kind, stale lifetime or confused role admits input | Check daemon-minted reference's actual kind/bindings; closed fields/methods; real-daemon wrong-kind, stale-generation and target-ended effects |
| Concurrent input/yield/cancel/force/accept | Check-before-queue TOCTOU or stale transfer revokes winner | Per-PTY dispatch/revocation ordering; retain authority through actual queue consumption; compare generation plus reservation; deterministic race effects |
| Native bytes and shell continuations | Buffered work applies after a falsely safe grant | Native unknown batch survives revocation; originating shell authority survives reports; settlement must be authoritative, never write acknowledgement/timer; final complete-broker fence required |
| Public snapshots/events/native labels | Private handles or terminal-control injection | Public IDs only; private recovery scoped to controller; sanitize/clip native labels; public-leak and real rendered-surface proof |
| Attachment view/activity | Borrowed credentials forge human consent/idle facts | Bind to original persistent connection; monotonic serials; initial unknown; disconnect does not release/consent; conditional grant revalidates facts |
| Codex actions | Stale thread/turn/item/request or duplicate approval reaches another live request | Complete identity and exact string/integer provider IDs; tombstones; no silent retarget, replay, session-wide permission or generic passthrough |
| Provider transport and process resources | Unbounded observer wait, leaked backend/socket/auth copy | No provider I/O under topology/control locks; existing bounded frames/queues; private owned roots; bounded explicit live checks and only-owned termination/reaping/cleanup |

## Risk and gates

High correctness/concurrency risk; no new cross-user privilege boundary. Unknown native application settlement blocks safe grant and first acceptance. Force is explicit unresolved-risk acceptance, never interruption or process stop. Keep/withdraw preserves the same human control; ordinary cancellation invalidates prior control and grants nobody. Tests must preserve stock Zsh/OMP and independent terminal-query behavior.

The implementation uses private 0700 roots and 0600 auth/schema/log files. Real harness checks use private profiles/copies, not global login or refresh. Local inference binds authenticated loopback only. Rendered retained evidence redacts private references and wrapped credential strings; raw PTY/auth/provider logs are disposable. Bounded outer-PTY readers continue through owned process teardown, then join before saved terminal modes are restored using the retained master on Darwin.

Scoped turn/pending observation, actual native mode selection and the separate genuine question/allow/deny/interrupt checks now pass. Deny uses the pinned request-only `decline` response, not native-menu `cancel` or broader permission; allow remains request-constrained. Response-winner race presentation and native/application settlement remain complete-broker gates. The final check failed at its first safe offer with zero model turns and verified cleanup; it did not pass all32 or the stopped-native-input edge. Deterministic tests and standalone harness inference cannot waive those gates.

This maintained threat model is not a vulnerability scan PASS, independent review, runtime acceptance or reset of the historical review cap.

Completed auth/profile/schema/raw-output roots and owned inference/diagnostic resources were removed. Worker source was byte-verified into private recovery archives before retiring their sessions and worktrees; no unmerged source was discarded. See the integrated Build and worker-retirement evidence. The final workspace/Clippy/check/build/native/showcase/OMP/Zsh checks passed; this remains self-verification, not an independent security review.
