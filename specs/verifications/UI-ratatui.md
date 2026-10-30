# Ratatui UI verification

## Mechanical verdict

Single contiguous command `cargo fmt && cargo test -- --nocapture && cargo clippy --all-targets -- -D warnings && cargo build --release` exited0 (artifact210). Exact stdout/stderr: `UI-ratatui-preflight.log`. Existing **56 Rust tests passed**, one intentionally ignored subprocess helper; Clippy denied warnings and release build passed. OMP integration was unchanged; its previous eight-test verdict is not represented as a new run. No permanent UI tests were added: actual native surface exercises cover this presentation change.

## Actual native wrapper

Ran rebuilt release daemon with stock Zsh in a private temporary startup/config environment. A managed host Zsh launched the actual TUI as its foreground application, displaying another managed terminal. Read the resulting UI through the daemon-owned complete emulator—not source text or a mock frame. Additional standalone controlling-PTY session verified the sole-window case.

- At 100×33, three-row rounded project/active-window tab bar with rightmost 12×3 Switch button; native terminal was 100×30, occupying every remaining row/column. No footer or terminal border.
- Mouse Switch replaced the surface with full-screen WINDOWS/search header and hierarchical two-line list. Project/agent/Git metadata and cwd appeared; clicking a row opened the window directly. Native target geometry stayed unchanged while list was open.
- Native input printed `界é NATIVE_RATATUI` correctly; combining and wide characters, shell prompt and cursor were retained.
- Native `exit 7` became an actual Exit code:7 card with Close window/New terminal buttons. Clicking New opened a fresh live shell in captured cwd, while old exit7 record remained. Selecting old record from list and clicking Close removed it without exiting the TUI host.
- Raw foreground probe used alternate screen, application cursor, SGR mouse, bracketed paste, focus and a custom palette. Actual native color projected into host cells as RGB `[17,34,51]`. Probe initially inherited its native cursor row on alternate-screen entry; a smoke assumption that content must start at row0 was corrected, not the product.
- Forwarded bytes captured by actual foreground process: `1b4f411b5b3230307e615b33316de7958c65cc811b5b3230317e1b5b3c303b31303b354d1b5b3c303b31303b356d1b5b4902`. This contains Up→ESC O A, bracketed paste with inner ESC stripped but Unicode preserved, mouse `(10,5)` after subtracting three header rows, focus ESC[I, and literal Ctrl-B from prefix twice. Probe alternate-screen exit restored native shell.
- Picker-open resize from 100×33 to120×36 left foreground PTY100×30 unchanged until Escape; then foreground became120×33. Rapid visible resize70×20→160×42→40×18→120×36 produced corresponding native heights minus three, without affecting runtime lifetimes.
- Actual exit0 card at38×22 stacked Close/New vertically and retained exit code. Keyboard N started another native shell. Prefix/Ctrl-Q returned host to native prompt while child remained alive.
- Standalone actual TUI observed exit9; X removed its only terminal record, leaving a usable empty full-screen list and a live TUI. Ctrl-N created a fresh native shell. Detach exited0; daemon shutdown exited0, roots absent and socket removed.

## Ownership and limits

Only Cargo metadata/lock and client presentation/input handling changed; daemon API, emulator, shell/OMP adapters and process lifetime implementation remain unchanged. Ratatui0.30.0 uses Crossterm0.29; declared Rust minimum1.86 matches dependency metadata, but minimum toolchain remains unverified. Linux has the previously documented Docker VM I/O blocker. Original whole-MVP review cap remains exhausted; this request does not reset that cap or claim an independent review pass. Temporary probe/startup files and smoke processes were removed after proof.

## Post-release quiet ownership and Switch repaint — 2026-10-26

User report: ordinary “Controlled here” chrome is unnecessary, and clicking a session in Switch can leave the selected-row background in the terminal pane. Implemented on `fix/quiet-ownership-picker-repaint`, based on main `cc468a34cd7c055457ec286fc3464dc5ac2103a9`; no commit, merge, push, tag or release is part of this repair.

### Changes and boundaries

- `src/tui/control.rs`: ordinary locally controlled views retain the existing project/session header. Clear the routine successful-acquisition message and stale shown-warning pointer when no warning exists. Preserve explicit control panels, manual pending requests, conditional draw-bound warning accounting, uncertain/locked/reserved states and the force-risk message. Ownership/input admission and the three-row geometry are unchanged.
- `src/tui.rs`: remember whether the last successful paint showed Switch. On its transition back to a session, clear the physical/Ratatui canvas before drawing the complete surface, once per overlay exit rather than every frame. This covers row selection, Return, Escape, Enter, search and prefix navigation without input-specific patches. Native cached cells, application PTY data and process lifetimes are unchanged.
- Existing native readiness helpers now correlate authoritative attachment ownership with the actual project header instead of waiting for the removed badge. Competing-attachment election still requires the other view's locked warning; real input/effect/identity assertions remain. The ignored complete-broker helper is migrated and compiled, not run against a provider.

The user report is ground truth. A simple pre-fix same-target/default-color probe restored identical cells; it did not independently reproduce the original host-terminal artifact, and no pre-fix runtime RED is claimed. The observed code left physical recovery to incremental diff after dropping the styled picker; the repair explicitly invalidates that canvas at the presentation boundary. Exact original host-terminal trigger remains unclassified.

### Exercised verification

- Baseline `cargo test --lib tui:: -- --nocapture`:10 pass,122 filtered; `bg_361`, `artifact://2790`.
- Integrated `cargo fmt && cargo build`:pass, `bg_362`.
- `cargo test --workspace -- --test-threads=4 && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check && cargo build --release --locked`:pass, `bg_363`, `artifact://2803`,172.28s;451 Rust tests across30 suites,12 ignored. No ignored/live case was promoted to a pass.
- `bun test integrations/omp/lifecycle.test.ts`:8 pass/49 assertions, `bg_364`. `zsh -n integrations/zsh/adapter.zsh`:pass in `bg_365`.
- Actual stock-Zsh controlling-PTY client, continuous emulator drain/cursor replies, private cleared environment/HOME/config/runtime/socket,110×28: local ownership renders the ordinary project/session header without the badge/message; a second attachment retains the locked warning; deliberate Force still displays its prior-calls/work-continues risk.
- Actual colored native output includes `界🙂`,16 truecolor-background cells and arithmetic42. Mouse row, Return button, Escape, Enter, typed search, prefix/Escape and different-target/back transitions each restore all2750 native cells' text, foreground, background, bold and reverse attributes, with zero residual selected-row backgrounds. Search text is consumed locally, not replayed into the shell.
- Physical resize while Switch is open to110×30 reconciles the native PTY to110×27. All2970 rendered native cells match the new canonical screen, including legitimate scrollback reflow; fresh shell arithmetic47 succeeds. Both original stock-Zsh PIDs remain29678/37058.
- Installed optimized client independently controls a fresh native shell PID55323, renders its arithmetic42, selects a deterministic ACP peer PID55347/session`arithmetic-55347`, submits one actual prompt and observes `Total=42; commands=1`. Independent `effect.json` confirms value42/commands1/original RPC2. Returning to native restores all2750 cells and the original shell PID. Both normal native and ACP title bars stay quiet. Its captured output has three full-screen clears: startup and the two Switch exits, not a clear per paint.

Two throwaway-driver assumptions were corrected without product changes: prefix/Ctrl-Q while already in the picker opened its ACP launch dialog, so it was cancelled and detach was retried from native; search indentation/ACP reply wording and pre-resize row positions were replaced with actual row data, independent effects and the current canonical grid. No adapter was launched from the accidental dialog, no provider was contacted, and no fixture failure is claimed as a product RED.

### Local installation, privacy and cleanup

`cargo install --path . --force --locked` installs the verified optimized client at `~/.cargo/bin/amux`; version remains`amux 0.1.0`. Built and installed SHA256 both`39979c07f94036ca7b2f60543485ecbe3fbd4b3b4c28a7807af76526fb716328`. This is a local uncommitted client patch, **not** the immutable published v0.1.0 asset; its tag/assets and original release receipt remain unchanged. Existing TUI processes need reopening to load it; do not restart/shut down the live daemon. Reattachment still requires explicit ownership recovery when appropriate.

All three TUI hosts exit0 with original termios observed before host exit, owned drainers joined and hosts reaped. Private daemon29677 shuts down normally with exit0; socket, native PIDs29678/37058/55323 and peer55347 are absent. Unrelated sentinel55322 survives daemon cleanup, then its owner terminates/reaps it. `lsof +D` reports no root holders (exit1, empty stdout/stderr) before owned temporary-root removal. No global/live daemon, config or auth changes; zero new provider calls, budget12/cap12/0 remains unchanged.

No new permanent tests, dependencies, shell/OMP changes or independent review PASS. E05 parking, historical native unknown-settlement failures, source/security review caps, coverage/traceability waivers and platform/minimum-toolchain limits remain unchanged.