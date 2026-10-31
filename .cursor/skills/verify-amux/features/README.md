# amux verification map

This directory is the maintained source for verifying the user-facing CLI behavior of amux. Read the index before driving the app, then use the matching feature file as the recipe.

## Baseline preconditions

- Run `.cursor/skills/verify-amux/bin/verify-amux launch` and require `"ok": true`.
- The daemon uses a private temp root. Its config, socket, projects, and shell home are not the user's.
- The startup shell's context is that private workspace.
- Run `.cursor/skills/verify-amux/bin/verify-amux doctor` before a recipe when a command fails, and require `"ok": true`.
- Never drive a daemon that this launch did not start.

## Driving conventions

- Start every recipe from the baseline state unless its preconditions say otherwise.
- Treat every command as literal. Keep quoted names and flags unchanged.
- Run daemon commands through `verify-amux cli --`.
- A CLI result is `result` inside the version-1 JSON object. An `"ok": false` object is a failure, not a successful empty state.
- Do not remove proof artifacts during cleanup.

## Proof and skip reporting

- Capture the user action and the resulting state, not only the final snapshot.
- CLI proof includes the command, stdout, stderr, and exit code in the feature's artifact directory.
- Mutation proof includes a second read: `snapshot`, `state`, or `projects list`.
- Record the feature id and the entry point with every artifact.
- Report an unreachable path with the attempted command and the unmet precondition.
- Do not report a skipped entry point as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior. It then uses exactly four H2 sections in this order.

1. `Sub-features` lists short IDs with one line for each behavior.
2. `How to get to it (user POV)` lists every user entry point.
3. `Driving it with verify-amux` starts with `Preconditions:` and uses labeled bullets that pair each user action with an exact command and observable result.
4. `Gotchas` lists traps that can waste or invalidate a verification run.

Keep implementation details out of the map. Name only user paths, stable handles, required state, commands, and observable proof.

## Features

- [Startup shell](./startup-shell.md) covers the shell a fresh daemon serves before any other command.
- [Open a terminal](./open-terminal.md) covers opening a second native terminal from that shell.
- [Save a project](./save-project.md) covers saving project configuration without starting or restoring a process.
- [Read the screen](./read-screen.md) covers waiting until the shell is ready and reading its screen.
- [Close a terminal](./close-terminal.md) covers ending one terminal and leaving the daemon and its other terminal up.
