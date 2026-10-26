---
checkpoint: e06-acp-daily-use
release: v0.1.0
channel: macOS-arm64-preview
status: released-installed-and-verified
---
# Current daily-use checkpoint

## Decision

E06 is complete: all7 tasks and3 stories pass. Its [capsule](../epics/archive/e06-acp-daily-use/PLAN.md) is archived; E05 stays parked with its17 unimplemented tasks still failing. The user explicitly authorized finishing the release, pushing signed main and installing the verified binary. Do not stop the live daemon or alter global config/auth. No new live-model run is authorized:12 actual requests/cap12/0 remaining.

The current checkout is `/Users/addomam/projects/amux-acp`. Signed implementation `cdc2897fc93476212f153d848383495d1499f7f1` and signed landing record `83865ebc1062de87609e866ea9a49cb25cd84a85` replace the tree-identical unsigned commits. Preserve `backup/e06-unsigned-442d8a9` and the historical failed signing/review/runtime evidence.

Published [v0.1.0 preview](https://github.com/addomam/multiplexer/releases/tag/v0.1.0), release ID423898128, on2026-10-26. Main/tag commit`19df8e11a3cafcc281de29b73624016c875eba8f`; annotated tag`73a34aa4dfdd2589ad1843f153619551b7eb535c` has a valid SSH signature locally and on GitHub. The protected-branch guard required the permitted feature-branch/PR merge route, preserving signed history without squash/rebase/force or guard bypass.

Downloaded GitHub assets match the committed archive/binary checksums. Installed the exact published binary at `~/.cargo/bin/amux` on the existing PATH; `amux --version` returns `amux 0.1.0`. Packaged and installed private-daemon/profile ACP smokes independently apply requested settings and produce same-session `17+25=42` on the original PID, with one agent view, normal peer/daemon/socket cleanup and unrelated sentinel survival. Strict ad-hoc signature verification passes; this is **not Apple Developer ID signing/notarization**.

Owned private staging was removed only after public-asset verification, normal process cleanup and `lsof` exit1/no holders. No live/global daemon was contacted/restarted, no config/auth/PATH changes and0 new provider requests. Preserve the unsigned backup; retire only the fully integrated release branch.

## Verification

- Release preflight `bg_360`, raw `artifact://2733`:451 workspace Rust tests pass across30 suites,12 ignored; strict workspace Clippy/check/fmt pass; workspace optimized build and explicit optimized ACP fixture build pass;13 optimized profile/native cases pass in2.12 seconds;8 OMP tests/49 assertions and Zsh syntax pass. Wall189.29 seconds;0 provider requests. The13 cases overlap the workspace suite; do not sum them.
- [E06 complete verification](e06-daily-use-verify.json), [full regressions](e06-post-round10-regressions.json) and [actual native/CLI smoke](e06-native-cli-smoke-post-round10.json) retain exact commands, observations, initial failures and repaired results.
- [Fresh source/security review11](e06-review-round11.json):A100/B100/security100, zero findings; all92 frozen hashes matched at that verdict. Historical caps are unchanged. Release-only records/archival are not another runtime or source-review PASS.
- [Genuine OMP ACP acceptance](e06-omp-live-final.json):one complete1/1 run,0 ignored,10.06 seconds; native built-in question, original pending permission preserved across API Force, exact deny/allow effects, unchanged tuple/PID/work/RPC and normal turn/owned cleanup. Four cheap subscription requests in that run; aggregate12/cap12/0. No premium/API-key fallback. Private copied-auth/raw resources removed, global subscription credential unchanged; no secure-erasure claim.
- The [release receipt](release-v0.1.0.json) records actual package/download hashes, published assets, signed tag verification, installed runtime effects and owned cleanup. The immutable tag retains prepublication build proof; this follow-up main record closes publication/installation.

## Operating contract

`amux` starts/attaches the native surface; Ctrl-Q detaches the client only. Explicit ACP profiles launch installed OMP/Codex/Grok adapters; executable availability is not an auth/model guarantee. Requested settings and agent-reported/unreported settings remain distinct. Use the same original ACP conversation for structured and native interaction.

Force never implicitly interrupts or kills. Ordinary pending cancellation invalidates old control, grants nobody and retains outstanding calls. Conditional keep/withdraw preserves human control; reacquisition policy remains external. Disconnection/sleep fences authority and discards intercepted drafts rather than replaying them or automatically reacquiring/relaunching. No actual hardware sleep/resume run is claimed; deterministic pause/recovery/native regressions provide the recorded evidence.

Installing a binary does not replace a running daemon. Finish valuable work before a deliberate daemon upgrade: `amux shutdown` ends all managed work and unsaved runtime state. Runtime state is memory-only; saved project configuration opens new shells, not restored interpreter/process state.

## Limits

macOS arm64 only for this preview. Rust/Cargo1.95.0 and stock Zsh5.9 were exercised; Rust1.88 minimum and Linux runtime remain unverified. Genuine OMP ACP proof used18.12.6; native OMP18.8.7 evidence is historical. Codex/Grok provider compatibility remains limited to their earlier recorded E04 rows; there are no new provider runs in this release.

Terminal-only native/application unknown settlement remains unresolved and blocks safe cooperative grants; Force-only behavior is not complete-broker acceptance. E03's historical failed all32 acceptance and review caps remain intact. E05's language-neutral non-ACP API stays parked until a concrete consumer needs it. No crash recovery, interpreter cloning, remote daemon protocol, automatic agent/task policy or seamless live-daemon upgrade.

Quantitative coverage and generic traceability are WAIVED because no repository coverage report/matrix/scripts exist;7/7 explicit task verification/evidence links are present. GitHub CI is not configured; local checks do not become a CI-green claim. This preview is not a crates.io/package-registry publication or an application deployment.

## Historical checkpoint

The untouched [E01 daily-use checkpoint](daily-use-checkpoint-e01.md) remains available as historical evidence. It is not the current release status.
