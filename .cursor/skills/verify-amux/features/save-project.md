# Save a project

Save project stores a name and directory for the unsaved startup project. It does not start a shell, stop a shell, or restore an old process.

## Sub-features

- `save-name` marks the startup project saved under `verify-project`.
- `save-list` shows that project from a second read.
- `save-no-process` leaves the existing terminal ids unchanged.

## How to get to it (user POV)

- Run `amux projects save verify-project`.
- Run `amux projects list` to see saved projects.

`amux projects open verify-project` is a separate entry. While this daemon already has a terminal in that project, open reattaches and does not start another shell.

## Driving it with verify-amux

Preconditions:

- `.cursor/skills/verify-amux/bin/verify-amux doctor` prints `"ok": true`.
- No project has been saved in this private daemon yet.

- **Record terminals.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- snapshot`. Exit code 0. Record the set of `result.terminals[].id` as `BEFORE`.
- **Save.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- projects save verify-project`. Exit code 0. `result.name` is `verify-project` and `result.saved` is true.
- **List.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- projects list`. Exit code 0. `result` is an array containing one object whose `name` is `verify-project` and whose `saved` is true.
- **Confirm processes.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- snapshot`. Exit code 0. The set of `result.terminals[].id` equals `BEFORE`.
- **Proof.** Run `.cursor/skills/verify-amux/bin/verify-amux evidence save-project`. The artifact stdout is that confirming snapshot, and the transcript contains the save and the list.

## Gotchas

- Save writes the private config home created by launch. A change under `~/.config/amux` means the run was not isolated.
- `projects open` on a project that already has a live terminal returns `reattached` true and does not add a terminal. A new shell from open is observable only after a restart that has no terminal for that project.
- Saving does not keep the shell's process across `verify-amux cleanup`. Cleanup ends the private daemon and its shells.
- A second save of the same name fails because that saved name already exists.
