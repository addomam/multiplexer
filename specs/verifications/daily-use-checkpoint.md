---
checkpoint: e06-acp-daily-use
release: v0.1.0
channel: macOS-arm64-preview
status: released-installed-and-verified
---
# Current daily-use checkpoint

## Latest local responsive Menu

Menu/project buttons now stay horizontal when full labels fit, stack on narrow windows and become compact rows when short. Ordinary shortcut legends are removed from both default and locked Menu headers/buttons. **?** in Menu or **Ctrl+/** opens wrapping, scrollable help; Escape closes only help and preserves the search/editor draft. Distinct enhanced Ctrl+? works too; legacy Ctrl+? remains Backspace. Help stays below the ownership header rather than painting over warnings.

[Responsive Menu proof](responsive-menu.json) records the final installed 110×28, 32×42, 80×6 and 20×10 surfaces, 32×12 compact project dialogs, physical mouse actions, literal editor question marks, no input replay and unchanged background native dimensions. Same shell PID 26713 produces 42 then 57; all 2,750 native character/color/style cells restore exactly, including 38 blue cells and zero menu-background artifacts. Final gates: 458 Rust tests/12 ignored, strict Clippy/fmt/release/install, OMP 8/49 and Zsh syntax. Four TUI/host/drainers and both private daemons/native processes/sockets/root finish normally; the final live sentinel survives application shutdown before owner reaping. Initial shortcut/header findings, fixture mistakes, expired bounded guard and ENOSPC are retained in evidence.

Installed SHA256: `a45a1f7d44605994efe82ae78e43e8506a38baf7f9470f8f2dc3d2e43681b2ab`. Reattach the client to load this UI patch; it adds no daemon/protocol/storage changes. The global daemon/config/auth remain untouched, and the earlier project-backend upgrade boundary below still applies. Changes remain uncommitted on `feat/cli-projects-help`; no new providers/review PASS/merge/push/release or historical cap reset. Only the explicitly approved incremental build cache was cleared; the user subsequently reported 60GB free and normal builds continued without settings changes or further cleanup.

## Previous local Menu/project workflow

**Menu → New session → `cd` → Menu → Save project** now bookmarks an existing directory (the confirmed Git root inside a repository), without creating a folder or restarting that session. Each New session gets an independent unsaved project ID; Ctrl-N still adds a terminal to the selected project. **Menu → Projects** opens/reuses saved projects, edits names/hooks/recipes and confirms bookmark removal while keeping files and native/ACP sessions. The visible Switch control is renamed Menu.

[Menu/project verification](menu-projects.json) records 456 passing Rust tests/12 ignored, strict Clippy/fmt/release/install, OMP 8/49, Zsh and 38 installed help paths/76 invocations. Actual controlling-PTY proof covers mouse/keyboard/Tab saves, independent bookmarks, configuration errors, compact resize, same native/ACP 42-to-57 continuity through Forget, private restart/open and a fresh interpreter. All 16 blue native cells survive Menu with zero selected/chrome background artifacts on the final installed client. Fixture-oracle/input failures are retained; all owned TUI/host/daemon/native/peer/drainer/socket/root resources finish normally, the unrelated sentinel survives, and no fallback kill or provider call is used.

The earlier Menu/project build SHA256 `84e49d1e3764af7a08991c4dd076ce6ccc466db6b93034908ade83f24046a179` is superseded locally by the responsive build above, not by a new published release. **New Save/New session require an updated daemon** and fail safely on an old one. The global live daemon/config/auth were not touched: finish its managed work before a separately approved upgrade. Reopening only the client does not upgrade its daemon. Historical provider/review/native-fence/E05 limits remain unchanged.

## Previous local CLI repair

The requested command/argument descriptions and examples now cover short and long help throughout the CLI. Project management is `amux projects list|add|save|open|config|remove`, without the old top-level CLI aliases. Removal forgets saved configuration only: directories, live native/ACP work, runtime identity, hooks and recipes remain. Current changes live uncommitted on `feat/cli-projects-help`, carrying the earlier UI repairs intact.

[CLI and project lifecycle proof](cli-projects-help.json) records38 installed help paths/76 calls/378 documented rows,453 passing Rust tests, strict Clippy/fmt/release, stock OMP8/49 and Zsh syntax. Actual CLI/native and deterministic ACP fixtures prove42-to57 continuity, original RPC completion, conflict rollback without a false success event, retained files and correct saved IDs after private restart. All private processes/sockets/root are cleaned normally; unrelated sentinel survives. No provider calls or global config/auth changes.

The earlier CLI-only build reported `amux 0.1.0` with SHA256 `b62d74d3859af24ad8e499619b7421b801eef918b95ec8d94cc97a7a3e2f4124`; the Menu build above supersedes that local artifact, not the published assets/tag or original receipt. List/remove require an updated daemon; no automatic live restart, commit/merge/push/new release or historical cap reset was performed.

## Previous local UI repair

The quiet ownership/Switch repaint fix was verified and installed from `fix/quiet-ownership-picker-repaint`; its uncommitted changes are retained in the current branch/build. The default controlled-here badge/message is hidden; exceptional control notices remain. Switch exits repaint the client canvas once without clearing the managed application or replaying input. [UI proof](UI-ratatui.md#post-release-quiet-ownership-and-switch-repaint--2026-10-26) records451 passing Rust tests, actual colored native/session/resize checks and installed native plus synthetic-ACP arithmetic42, with normal private cleanup and0 provider calls.

The earlier UI-only install had SHA256`39979c07f94036ca7b2f60543485ecbe3fbd4b3b4c28a7807af76526fb716328`; the CLI build above supersedes that local artifact, not the published release. Reopen existing TUI clients to load the combined patch; normal explicit reacquisition rules still apply. No new model/auth/global-config/publication authorization or E05 scope change.

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
