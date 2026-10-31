# Startup shell

Starting amux creates a private daemon and one native shell in an unsaved project. The user can see that shell from the CLI before opening anything else.

## Sub-features

- `startup-ready` serves a snapshot only after the startup shell exists.
- `startup-identity` reports a daemon id, one terminal, and an unsaved project.
- `startup-isolated` keeps that state in the private root from launch.

## How to get to it (user POV)

- Run `amux daemon` against a private socket and project directory.
- Run `amux snapshot` once that daemon is up.

The TUI entry `amux` with no command is a different surface. This recipe does not attach it.

## Driving it with verify-amux

Preconditions:

- `.cursor/skills/verify-amux/bin/verify-amux launch` already printed `"ok": true`.
- No earlier `open`, `projects`, or `close` command has been sent to this daemon.

- **Doctor.** Confirm the instance. Run `.cursor/skills/verify-amux/bin/verify-amux doctor`. Exit code 0 and `"ok": true`. `terminals` is at least 1 and `startup_pending` is false.
- **Read the shell.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- snapshot`. Exit code 0. `result.startup_pending` is false. `result.terminals` has one object. That object's `id` and `name` are non-empty strings, and `project_id` is a non-empty string.
- **See the unsaved project.** In the same snapshot, `result.projects` has one object whose `id` equals the terminal's `project_id` and whose `saved` is false.
- **Proof.** Run `.cursor/skills/verify-amux/bin/verify-amux evidence startup-shell`. The artifact stdout is the snapshot above, and the transcript contains the snapshot command.

## Gotchas

- A snapshot that fails or still has `startup_pending` true is not a startup shell. Wait for doctor.
- The startup terminal is not named `main`. Its name is generated from the private workspace directory. Select it by `id` from the snapshot.
- `amux` without a subcommand attaches the TUI and is not this entry point.
- Launch refuses a second isolated daemon. Cleanup the first run before launching again.
