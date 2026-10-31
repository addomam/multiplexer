---
name: verify-amux
description: "Drive an isolated amux daemon through the amux CLI and prove startup shells, new terminals, saved projects, screen reads, and terminal close. Use when changing the CLI, daemon startup, projects, or native terminal lifecycle, or when a proof must show the real user path."
---

# Verify amux

amux is a local daemon plus a CLI and a detachable TUI. This skill drives the CLI against a private daemon. The TUI (`amux` with no command, and `amux attach`) is the other surface; do not attach it here. `tests/pty_control` and `specs/verifications` are historical evidence, not this recipe.

The helper is `.cursor/skills/verify-amux/bin/verify-amux`. Run it from the repo root. It builds `target/debug/amux`, starts `amux daemon` in its own session, and never uses the default socket, `~/.config/amux`, or an already running daemon.

## Launch

```sh
.cursor/skills/verify-amux/bin/verify-amux launch
```

Launch creates a private root under the system temp directory, with mode `0700` directories for the runtime, config home, project directory, workspace, and home. The daemon's `HOME`, `ZDOTDIR`, `XDG_CONFIG_HOME`, `AMUX_RUNTIME_DIR`, and `AMUX_SOCKET` point only at that root. Its cwd is the private workspace, so the startup shell's context is that directory.

Ready means `verify-amux doctor` prints `"ok": true`: the recorded pid is still that daemon, `amux snapshot` returns version-1 `"ok": true`, `startup_pending` is false, and `terminals` has at least one entry. A listening socket alone is not ready.

Launch runs the build, starts the daemon, and polls doctor for up to 45 seconds. If the daemon exits or never becomes ready, launch cleans up the private root and prints the daemon log.

## Doctor

```sh
.cursor/skills/verify-amux/bin/verify-amux doctor
```

Run doctor first whenever a later command looks wrong. Exit 0 and `"ok": true` means the instance is the one this run started and the startup shell is usable. Any other result means do not drive it.

## Drive

```sh
.cursor/skills/verify-amux/bin/verify-amux cli -- snapshot
.cursor/skills/verify-amux/bin/verify-amux cli -- open --source TERMINAL_ID --name verify-build
```

Arguments after `--` are `amux` arguments. The helper adds the private `--socket`. Every CLI response is one JSON object, `{"version":1,"ok":true,"result":...}` or `{"version":1,"ok":false,"error":...}`, and the helper exits with the amux status. The feature files name the selectors and the fields that prove each behavior.

Do not run a bare `amux`, `amux attach`, or `amux config`. Those start or attach the TUI, or write the user's real config. Do not point the helper at a socket you did not launch.

## Evidence

```sh
.cursor/skills/verify-amux/bin/verify-amux evidence FEATURE
```

This copies the latest command's argv, stdout, stderr, and exit code, plus the run transcript and daemon log, to `.cursor/skills/verify-amux/artifacts/FEATURE/`. Capture the command that performed the user action and a later read that shows the resulting state. A final snapshot alone is not the proof.

Proof has to follow a user path. `snapshot`, `open`, `projects`, `screen`, `state`, `wait`, and `close` are those paths. Do not call a test-only endpoint or write daemon memory. When a command writes config or a terminal record, read it back with `projects list`, `snapshot`, or `state`. The private config home is the side effect to check for `projects save`; do not read `~/.config/amux`.

## Cleanup

```sh
.cursor/skills/verify-amux/bin/verify-amux cleanup
```

Cleanup sends `SIGTERM`, then `SIGKILL` if needed, to the process group of the pid recorded at launch, and only if that pid's command line still contains the private socket. It then deletes the private root. It does not delete `.cursor/skills/verify-amux/artifacts/`. After cleanup, the evidence files must still be there.

Two launches at once are refused. Run cleanup before starting another one.

## Helpers

The only helper is `bin/verify-amux`, invoked as shown above. `launch` builds the binary. `doctor` checks the process, version, and snapshot. `cli --` runs one amux command and appends it to the transcript. `evidence` copies proof out of the private root. `cleanup` stops the recorded process group and removes the private root.
