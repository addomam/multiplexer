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