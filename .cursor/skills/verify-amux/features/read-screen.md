# Read the screen

Read the screen shows the startup shell's current text after the shell reports that it is ready. The read does not send input.

## Sub-features

- `screen-wait` waits until the startup shell is ready.
- `screen-read` returns that terminal's lines and cells.
- `screen-unchanged` leaves the terminal id in place.

## How to get to it (user POV)

- Run `amux wait TERMINAL_ID shell-ready`.
- Run `amux screen TERMINAL_ID`.

## Driving it with verify-amux

Preconditions:

- `.cursor/skills/verify-amux/bin/verify-amux doctor` prints `"ok": true`.
- The startup terminal id comes from a snapshot in this same run.

- **Read the id.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- snapshot`. Exit code 0. Record `result.terminals[0].id` as `START_ID`.
- **Wait.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- wait START_ID shell-ready`, with the recorded id in place of `START_ID`. Exit code 0. `result.seq` is a number, `result.state.id` equals `START_ID`, and `result.state.integration.shell.ready` is true.
- **Read.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- screen START_ID`, with the same id. Exit code 0. `result.lines` and `result.cells` are arrays of equal length. At least one string in `result.lines` contains a non-space character.
- **Confirm the terminal remains.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- state START_ID`. Exit code 0. `result.id` equals `START_ID` and `result.alive` is true.
- **Proof.** Run `.cursor/skills/verify-amux/bin/verify-amux evidence read-screen`. The artifact stdout is the state read, and the transcript contains the wait and the screen command.

## Gotchas

- Reading the screen before `shell-ready` can show an empty grid. That does not prove the shell failed. Wait first.
- `screen` does not acquire control and does not type. A changed shell command after a screen read means some other command sent input.
- The prompt text depends on the private zsh, which has no user rc. Assert a non-space line, not a particular prompt string.
- `wait` without a terminal id fails. Use the snapshot id, not the display name.
