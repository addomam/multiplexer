# Open a terminal

Open terminal starts a second native shell from the startup terminal and shows both in a later snapshot. The first terminal stays up.

## Sub-features

- `open-named` creates a terminal named `verify-build`.
- `open-keeps-source` leaves the startup terminal in the snapshot.
- `open-same-project` puts the new terminal in the startup terminal's project.

## How to get to it (user POV)

- Run `amux open --source TERMINAL_ID --name verify-build`, using the startup terminal's id.

Menu paths that open a terminal inside the TUI are a different surface. This recipe uses the CLI entry only.

## Driving it with verify-amux

Preconditions:

- `.cursor/skills/verify-amux/bin/verify-amux doctor` prints `"ok": true`.
- The daemon has not yet opened `verify-build`.

- **Read the source.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- snapshot`. Exit code 0. Record `result.terminals[0].id` as `START_ID` and `result.terminals[0].project_id` as `PROJECT_ID`. `result.terminals` has length 1.
- **Open the terminal.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- open --source START_ID --name verify-build`, with the recorded id in place of `START_ID`. Exit code 0. `result.id` is a non-empty string different from `START_ID`. `result.name` is `verify-build`. `result.project_id` equals `PROJECT_ID`.
- **Confirm both terminals.** Run `.cursor/skills/verify-amux/bin/verify-amux cli -- snapshot`. Exit code 0. `result.terminals` has length 2. One entry has `id` equal to `START_ID`. One entry has `name` equal to `verify-build` and the `id` returned by open.
- **Proof.** Run `.cursor/skills/verify-amux/bin/verify-amux evidence open-terminal`. The artifact stdout is the second snapshot, and the transcript contains both the open command and that snapshot.

## Gotchas

- `--source main` matches a terminal named `main`. The startup shell does not have that name. Use its snapshot `id`.
- Open acknowledges a terminal record. The proof is the later snapshot, not the open response alone.
- Opening `verify-build` twice makes the name ambiguous for later name selectors. Use ids after the first open.
- This does not restore a saved project's old process. It starts a new shell.
