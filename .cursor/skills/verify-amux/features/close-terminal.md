# Close a terminal

Close terminal ends the second shell opened as `verify-build` and leaves the startup shell and the daemon running.

## Sub-features

- `close-one` reports the closed terminal dead.
- `close-other` leaves the startup terminal alive.
- `close-daemon` leaves snapshot working after the close.

## How to get to it (user POV)

- Run `amux close TERMINAL_ID` on the `verify-build` terminal.
- Run `amux state TERMINAL_ID` and `amux snapshot` to see the result.

Ctrl-Q in the TUI detaches the client and is not this entry. Close ends the terminal's processes.

## Driving it with verify-amux

Preconditions:

- `.cursor/skills/verify-amux/bin/verify-amux doctor` prints `"ok": true`.
- `verify-build` was opened in this run by the open-terminal recipe. Record its id as `BUILD_ID` and the startup id as `START_ID`.

- **Close.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- close BUILD_ID`, with the recorded id in place of `BUILD_ID`. Exit code 0.
- **See it stopped.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- wait BUILD_ID exited`. Exit code 0. `result.id` equals `BUILD_ID` and `result.alive` is false.
- **See the other shell.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- state START_ID`. Exit code 0. `result.id` equals `START_ID` and `result.alive` is true.
- **Proof.** Run `.cursor/skills/verify-amux/bin/verify-amux evidence close-terminal`. The artifact stdout is the startup state, and the transcript contains the close and the exited wait.

## Gotchas

- Close on the only remaining terminal still leaves the daemon running, but later recipes that expect a startup shell then have no live shell. Close `verify-build`, not `START_ID`, in this recipe.
- `remove` is a different command. It forgets an exited record and is not required to prove the process ended.
- Name selectors become unsafe after two terminals exist. Close by id.
- Cleanup is what stops the daemon. A successful close does not remove proof artifacts or the private socket.
