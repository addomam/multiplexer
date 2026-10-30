# amux

A project-oriented native terminal wrapper: a Rust daemon owns Zsh terminals, emulator state and explicit ACP conversations; a detachable TUI shows the selected terminal or conversation; the CLI controls the same local API. Shell editing and OMP remain their native interfaces.

amux is the execution substrate, not an autonomous orchestrator. It provides terminal/project control and observable agent state through the same local API used by the CLI and TUI. Task planning, scheduling, automatic replies, agent delegation and merge policy belong outside amux.

## Install and start

Tested with Rust/Cargo **1.95.0**, stock **Zsh 5.9**, native **OMP 18.8.7**, genuine ACP **OMP 18.12.6**, and macOS arm64. Linux has implementation paths, but attempted container validation was blocked by Docker VM filesystem I/O errors before any test ran. amux declares Rust **1.88** so its locked `time 0.3.47` dependency includes the RFC2822 stack-exhaustion fix; that minimum toolchain is not independently verified. Ratatui0.30.0 uses the existing Crossterm0.29 backend; the independent terminal-showcase package retains its own Rust1.86 declaration. Requires stock `/bin/zsh`. OMP is optional until launched.

The first binary release is the [v0.1.0 macOS arm64 preview](https://github.com/addomam/multiplexer/releases/tag/v0.1.0). Other platforms and the minimum Rust toolchain are not release-validated. Download `amux-v0.1.0-aarch64-apple-darwin.tar.gz` and `SHA256SUMS` from that release, then:

```sh
shasum -a 256 -c SHA256SUMS
tar -xzf amux-v0.1.0-aarch64-apple-darwin.tar.gz
mkdir -p "$HOME/.cargo/bin"
install -m 755 amux-v0.1.0-aarch64-apple-darwin/amux "$HOME/.cargo/bin/amux"
"$HOME/.cargo/bin/amux" --version
```

Keep `~/.cargo/bin` on your `PATH`, or invoke the installed path explicitly. The archive includes the MIT license and locked-dependency notices. The executable is linker ad-hoc signed, **not Apple Developer ID signed or notarized**; macOS may require local approval. Installing the binary does **not** restart or upgrade an existing daemon; finish valuable work before deliberately restarting an older daemon.

To build/install from this checkout instead:

```sh
cargo install --path . --locked
amux
```

The first invocation starts a background daemon and an unsaved project with a host shell. Its context follows the main shell's confirmed cwd, or the Git repository root, without changing the actual cwd. Other terminals keep their own locations.

```sh
amux snapshot
amux open --source main --name build
amux attach build
amux open --source main --omp --name agent
```

`amux attach` reattaches without restarting processes or running hooks. `amux daemon` runs in the foreground for supervision. `--socket PATH` and `AMUX_SOCKET` select a daemon; its directory must be private and owned by the current user.

Every command has short and long help with argument descriptions and examples: `amux -h`, `amux projects -h`, `amux projects add -h`, and `amux acp start -h`. Help does not connect to a daemon or launch an adapter.

## Daily operation

Start with `amux`; return with `amux attach`. In the picker, **Ctrl-Q** detaches only this client. Closing the client does not stop the daemon or its shells.

Save useful project configuration with `amux projects save NAME`. After a daemon restart, `amux projects open NAME` starts a **new shell** at the saved project context; it does not restore old processes or interpreter state.

**`amux shutdown` ends all managed work.** Finish valuable tasks before shutdown, reboot, or a daemon upgrade. Screen/scrollback, shell state and runtime authority exist only in memory. Supported open agent conversations retain bounded reopening metadata, not running processes or permission to continue work.

### Reopen conversations after a restart

On the next daemon launch, supported previously open conversations reopen automatically in their original directory/project, with their saved name and exact agent conversation identity. They **wait for your next instruction**: amux sends no continuation prompt, retries no interrupted turn/tool, answers no permission request and replays no terminal input. There is no recovery picker. New runtime/controller identities start without the old control grant.

- **ACP:** named configured/builtin profiles use advertised `session/load` or `session/resume`. The resolved launch command must still match; arbitrary raw-argv launches are not recorded. Agent-reported model/mode/effort/options are retained when known. Reopening restores acknowledged choices rather than remerging profile launch defaults; defaults still apply to new conversations. History stays in the agent's own backing store.
- **Native OMP:** the extension records the exact durable session ID/file and cwd. Reopening uses `--resume FILE`, never `--restore`. A session that was never written to disk cannot be recovered. Switching to an unsaved/unrecoverable conversation disables the old conversation's reopening intent; amux never substitutes the previous conversation.
- **Managed native Codex:** retain the exact thread ID and its durable native rollout store. Imports publish only complete, synchronized histories without replacing existing data; failed copies remain retryable, and unsafe destination aliases are rejected before writes. A frontend replacement whose new identity is unverified blocks the old thread's reopening intent rather than substituting it. Existing native Codex version/availability restrictions still apply; ACP does not depend on that native broker.

Explicit session close/normal root-agent exit forgets its reopening intent; detaching a client or stopping/crashing the daemon preserves it. If the original process is still alive, its identity is unknown, a backing conversation/profile is missing, capabilities are unsupported or the saved journal is unsafe, recovery fails visibly without killing anything, choosing “latest”, creating a replacement conversation or continuing work. Other conversations recover independently. Automatic attempts occur once per daemon start.

A reopening attempt is marked durably before launch. If a second crash happens before its new process identity is safely recorded, the unknown settlement blocks another automatic launch instead of risking a duplicate agent. Conclusively failed native starts can retry on a later daemon launch; unknown cleanup stays blocked. On macOS, process exclusion uses the immutable boot-session UUID and exact native process birth identity, not wall-clock-adjustable boot time.

A disposable blank startup shell does not consume a native reopening slot. Initial attachment waits for startup recovery instead of allocating another shell; Esc or Ctrl-Q quits the wait, and waiting input is discarded, never replayed. If no native terminal is restored or otherwise present, startup attempts a fresh shell within the same limit. Already published user terminals are never reused or taken away for restoration. A failed fallback is reported, and daemon-start hooks run only after a native or fallback shell is available.

Project labels and context may change while recovery is in progress without cancelling later saved conversations. Each conversation still uses its exact saved cwd and launch/session identity; close, replacement and uncertain process settlement remain exclusions.

To update this checkout's installed binary:

```sh
cargo install --path . --locked --force
```

Installing a binary alone does not replace a running daemon. The earlier client-only input optimization can reattach to a compatible daemon, but the terminal/configuration/admission/hook fixes require a fresh daemon. Finish valuable work before deliberately shutting down an old daemon; its managed processes and unsaved runtime state will end.

Startup requires an initialized version-1 API snapshot, not just a listening socket. Concurrent automatic starts are serialized; an existing unresponsive or incompatible listener returns a bounded readiness error and is not replaced. For startup failures, inspect `daemon.log` next to the selected socket. Default runtime directory: `$AMUX_RUNTIME_DIR`, otherwise `$XDG_RUNTIME_DIR/amux`, otherwise the OS temporary directory's `amux-<uid>`.

`AMUX_SOCKET` or `--socket PATH` chooses another socket. Configuration remains under `$XDG_CONFIG_HOME/amux/config.json` or `~/.config/amux/config.json`. Hook failures appear in `amux snapshot` diagnostics and do not disable the shell.

The [daily-use checkpoint](specs/verifications/daily-use-checkpoint.md) records the frozen source baseline, final checks, cleanup and remaining release limitations.

## Terminal capability showcase

`terminal-showcase` is a separate Rust workspace package. It queries and exercises the terminal endpoint through ordinary stdin/stdout, with no dependency on amux's implementation.

```sh
cargo run -p terminal-showcase
cargo run -p terminal-showcase -- --screenshot --demo colors
cargo run -p terminal-showcase -- --probe --json
cargo run -p terminal-showcase -- --headless --json
cargo install --path terminal-showcase --locked
```

Run the same binary in any attached amux shell. Space pauses; arrows select demonstrations; Tab expands evidence; D shows diagnostics; S freezes screenshot mode; Q/Escape exits. P/F record visual verdicts; E exports JSON. Clipboard, title, notifications and working-directory actions require an explicit key in the integration demo.

Build and test all packages with `cargo build --workspace` and `cargo test --workspace`; plain `cargo run` and the root default build still select amux. See [showcase usage, report semantics and limitations](specs/terminal-showcase.md). The explorer has already exposed [origin-mode CPR and ZWJ cursor incompatibilities](specs/bugs/BUG-showcase-endpoint-compatibility.md); a passing explorer regression suite is not a claim that amux supports every feature.

## Owned ACP conversations

An ACP view is a separate live agent target, not a terminal or a replacement stock-agent TUI. amux owns one stdio adapter connection and its session; the human editor and structured API address that same instance. Native terminal interaction remains a separate best-effort path: ACP does not repair the unresolved native input/application fence.

### Launch an installed agent

```sh
amux acp profiles
amux acp start omp --project "$PROJECT_ID"
amux acp start codex --project "$PROJECT_ID" --model "$ACP_MODEL_ID" --effort low
amux acp start grok --project "$PROJECT_ID" --mode "$ACP_MODE_ID"
```

`start` opens the human conversation and launches **once on that native attachment's original connection**. Project IDs or unambiguous project names are accepted. Builtins run installed `omp acp`, `codex-acp`, and `grok --no-auto-update agent stdio`; missing executables fail explicitly, with no automatic installation or substitute agent. Executables resolve through the daemon's PATH, without a shell. Existing `acp open` remains the raw absolute-argv API.
Symlinked executables retain their resolved invocation pathname, including virtual-environment and basename-selected launchers; validation follows metadata without substituting the canonical target.
Profile `available` means an executable was found, not that authentication or a particular account/model is usable.

Choose **Menu → New… → ACP agent**, then explicitly add the session to the current project or create an independent unsaved project. The basic form exposes Project/source, Profile and Name; **Advanced** reveals exact model, mode, effort and Options JSON. Blank names use a profile label and short unique suffix; launch names are limited to128 UTF8 bytes. Tab/BackTab and mouse move focus; arrows or j/k change selectors, but j/k/? stay literal in text fields. Enter advances a text field or activates the focused button. **Launch** submits once and retains the unchanged draft until admission; a definite rejection retains the full cause for correction and contextual Help, without retry. Escape cancels before submission; Ctrl-Q can detach while admission is pending without cancelling it. Ctrl-A remains an optional direct launch shortcut in Menu. If Ctrl-A is the prefix, navigate once before using it; prefix twice sends that literal key.

Optional `acp_profiles` configuration adds or overrides named profiles:

```json
{
  "acp_profiles": {
    "omp-low": {
      "label": "OMP · low effort",
      "command": ["omp", "acp"],
      "defaults": {"effort": "low"}
    }
  }
}
```

Profiles are credential-free argv/settings, not plugins. Explicit launch settings override profile defaults; `--option ID=VALUE` may be repeated for distinct advertised option IDs. Clearing a dialog setting returns to its selected profile default, shown as an inherited placeholder; option overrides merge with profile options, so deleting an option ID does not remove its profile default. Settings absent from both the request and the profile use agent defaults. Model/mode/effort IDs must actually be offered by that agent. amux waits for acknowledged, confirmed settings before admitting prompts; unsupported or unconfirmed requests fail rather than silently choosing another model. Inspect/snapshot `configuration` separates `requested` from actual `reported` values and marks unreported defaults or later agent-side changes. **Agent defaults are not a cheap-model or permission-policy guarantee.** Permission policy remains unreported unless the agent genuinely exposes it; a mode label alone is not proof.
For complete option identities, use `--options-json '{"fixture,detail=level":"brief,exact=value"}'`; the JSON object preserves commas, equals signs, backslashes and whitespace within quoted IDs and choices. The `--option ID=VALUE` shorthand splits at the first `=` and is only for IDs without `=`. Distinct shorthand entries may accompany the JSON object; duplicate explicit IDs, including duplicate JSON keys, are rejected rather than silently overwritten. CLI/native Options JSON is bounded to64KiB; advertised identity/count and total-settings bounds still apply.
Structured option selections retain exact advertised IDs even when independent options share a category; ambiguous semantic effort is rejected. Untouched native Options inherit profile JSON directly. Explicit native overrides use the same lossless JSON-object parser as the CLI; omitted IDs retain profile defaults.
Native launch edits are accepted or rejected in full. Oversized edits and control-containing scalar text leave the backing value unchanged and mark that field rejected; Launch stays blocked until each rejected field is deliberately corrected or reset. Editing another text field does not acknowledge it. Formatted Options JSON retains legal whitespace, with inert display rather than terminal control output.
Legacy `session/set_mode` accepts its protocol-valid empty `null` acknowledgement. Config-option setters still require a complete, value-confirming `configOptions` response; notifications alone do not confirm the requested setting.

The conversation prioritizes messages and the active question instead of protocol metadata. Genuine questions expose typed fields and separate Review/Submit actions; permissions expose the exact offered choices and require an explicit selection. Tab, mouse and contextual F1 help work across these controls. Completed questions remain available through Questions/History; their old editable defaults are not presented as the submitted answer. A completed response returns to the prompt without discarding its canonical record or original RPC identity.

Keyboard accelerators use the same displayed target/grant/interaction/review phase as buttons. Reviewed Submit and new/replaced permission, authentication and interruption actions arm only after complete visible presentation, conclusive input-empty readiness and a subsequent armed repaint. Settling choices use parentheses; armed choices use brackets. Queued old gestures are discarded, never promoted into later consent. Ordinary same-target prompt typing/paste followed by Enter and the first local Review do not require an extra confirmation.

**Details/F10** wraps and scrolls current/requested settings, exact target identity, canonical events, cursor/gap/resource diagnostics and full interaction context. Option metadata retains its explicit `[clipped]` display bound. While open, typing, paste, ordinary Enter, authentication and interruption stay local and are never drafted or replayed. Escape or an explicitly focused/clicked Back action returns; navigation preserves drafts and never requests control. First session establishment on the same live agent leaves read-only Details/History open; replacement identity still resets them and all authority-bound drafts. Working, needs-answer, permission-needed, finished and failed indicators come from canonical state rather than inferred screen text.

For a deliberate noninteractive launch, pass an explicit `--controller "$CONTROLLER_JSON"` to `acp start`; it returns the same JSON receipt instead of attaching. No temporary external controller is invented.

### Detach, reconnect and authority

Detach leaves the live ACP process/session and admitted work running. `amux acp attach "$ACP_VIEW_ID"` returns to that exact surviving view; it does not start another agent. A disconnected or sleep-paused attachment fences application input, discards drafts/queued mutations and old consent/control, then resynchronizes only through an existing daemon socket. It never replays input, automatically forces/reacquires control or resurrects a daemon. Another owner's authority stays intact; reacquisition is deliberate through the normal control UI.

Recovery verifies the daemon's runtime `daemon_id` and the original agent tuple. Initial session establishment may fill a previously null session ID on that exact instance/connection, including while disconnected; an established session never follows a reset. An acknowledged launch becomes the recovery target before further observation. If the UI loses a connection before consuming a parsed launch reply, teardown joins the old worker and retains only that successful target identity, never its authority or queued actions. Recovery neither repeats the launch nor returns to the previous agent. It observes and validates continuity before registering a new native controller; stopped recovered observers attempt guarded idle-controller cleanup, without releasing owners or cancelling pending claims. An unavailable daemon leaves cleanup retained rather than claiming reclamation. If the daemon was replaced or the target ended/disappeared, the UI says so rather than substituting a same-name session. Ctrl-Q/Escape can detach while disconnected. This is same-process reattachment, **not crash recovery or session restoration**.
An explicit keyboard or mouse selection becomes the recovery target before a fallible resize or subsequent observation, so recovery cannot silently return to the previous view.
Recovery drains events queued through registration and the first observer paint before announcing Reconnected. Stale control shortcuts are discarded too; Escape/Ctrl-Q can still detach, and a queue that cannot drain stays fenced. The current terminal geometry is reconciled before readiness; a changed size triggers another observer paint/input drain rather than replaying discarded events.
Ordinary native painting also reconciles the application PTY against the actual painted viewport if a resize notification is lost or coalesced. A changed viewport invalidates Force readiness and requires another paint/input-drain fence before confirmation becomes available.
The native worker checks its original observation and authority connections before dispatching queued work, and again after a claim's observation read. A fresh UI heartbeat alone cannot authorize a queued launch after observation loss; already admitted work is not implicitly interrupted. Its shutdown-handle allocation completes before creating the native controller, so that allocation failure cannot orphan a new registration.
The Force action is advertised only after the full warning has been painted and queued input drained. An early confirmation is consumed, never saved for later.

### Structured control

Register an external controller through `amux api '{"op":"controller-create","role":"external","label":"ACP operator"}'`. Use an exact project ID from `amux snapshot` and the returned private controller handle:

```sh
amux acp open --project "$PROJECT_ID" --controller "$CONTROLLER_JSON" --name agent -- /absolute/path/to/acp-adapter
amux acp inspect "$TARGET_JSON"
amux acp inspect "$TARGET_JSON" --content --after 0
amux acp attach "$ACP_VIEW_ID"
```

The initial target has a null session while initialization runs. Inspect the live descriptor to obtain the exact `kind`, `id`, `instance_id`, `connection_generation` and `session_id` tuple; capture it together with current control/generation and a unique call ID for each deliberate mutation. `acp-prompt`, `acp-authenticate`, `acp-answer`, `acp-permission`, `acp-interrupt` and `acp-close` use that captured authority. Responses additionally address the exact canonical interaction ID/revision. Stale or duplicate actions are rejected, never rebound or replayed.

In the ACP view, **Enter** submits a local prompt, **Alt/Shift-Enter** inserts a newline, and bracketed paste only edits the draft. During authentication, Up/Down chooses an exact advertised `method_id [kind]`; **F8**, its focused action or its clicked button authenticates that method deliberately. The selected method remains readable on narrow windows, with full inert identity/support context in **Help/F1**; unsupported client/terminal choices send no authentication. No automatic login, URL or credential form is opened. **Ctrl-C** requests interruption of the exact foreground prompt, not a process signal. Page keys and the mouse wheel navigate history without acquiring control. Revocation clears the old draft. Prefix opens the shared picker; prefix twice never forwards raw bytes into an ACP session.

Genuine ACP forms and tool permissions appear in this same conversation, including request-scoped forms during initialization/authentication. They are not inferred from terminal text. Form capability is advertised; URL elicitation, client filesystem and client terminal services are not.

| Interaction action | Key |
|---|---|
| Open/close read-only settings / return | F10 / F10 or Esc |
| Scroll settings while open | PgUp/PgDn, arrows, Home/End, mouse wheel |
| Cycle questions, permissions and history | F9 |
| Select form field | Tab / Shift-Tab |
| Edit text or numeric field | Typing / bracketed paste |
| Highlight an offered choice | Arrow keys |
| Toggle boolean or multiselect choice | Space |
| Explicitly unset a field | F4 |
| Validate/review form, then accept unchanged values | F5, then F5 |
| Decline form when offered / cancel form | F6 / F7 |
| Select the exact highlighted permission option | Enter |

Known flat string, number, integer, boolean, enum, titled-enum and multiselect controls are supported. Defaults start editable, never submit automatically; omitted optional values remain unset rather than becoming false, zero, empty or a default. Declared defaults may require explicit unset before the submitted answer fits **64 KiB**. Required values, offered choices, enum intersection, Unicode lengths, numeric bounds and selection counts are checked locally and again at admission. Pattern/format constraints are displayed but **not locally certified**; agent validation remains authoritative. Unknown/nested schemas or unsupported modes receive explicit protocol errors.

Only the first valid response is canonical; a stale losing answer is neither sent nor added as accepted history. Form decline, form cancel and explicit foreground interruption are distinct. Navigation, focus loss and detach never answer or deny. During explicit interruption, applicable pending and late questions receive Cancel/Cancelled without becoming selectable; the interrupt is confirmed only by the original prompt response, not by notification delivery. A written answer is not proof of agent application.

If the daemon definitely rejects a form or permission action before admission, the matching pending native editor retains its values/highlight and permits a new deliberate correction or cancel on the same session and grant. A late rejection cannot unlock a different call, revision or owner. Timeout/disconnect remains uncertain, never an automatic retry or proof of non-admission.

Grok's genuine `_x.ai/ask_user_question` ACP extension uses the same canonical form, authority and retained responder as standard elicitation. Single/multiple choices, descriptions, previews and free-form **Other** notes are preserved; duplicate question text or option labels are rejected. Selecting Other requires nonempty notes: shared local validation rejects missing notes before native capture, so the same grant can deliberately correct the draft or cancel the pending question. Grok offers accept/cancel, not decline: F6 sends nothing and the API rejects decline without consuming the question. Plan-specific chat/skip are not fabricated as decline or cancel.
Before offering a Grok question, amux conservatively budgets its encoded metadata twice, the full 64-KiB submitted-answer allowance, and bounded reply framing against the existing 1-MiB wire limit. This covers repeated question keys and previews: oversized requests receive an explicit protocol error on their original RPC before a native answer can be captured. The limit is conservative, not a promise to accept every otherwise in-frame request.

Codex ACP 2.2.2 exposes built-in `request_user_input` in Plan collaboration mode. Its explicit `/plan` prompt command toggles that mode on the existing session without an inference turn; use it again before requesting file changes. amux does not secretly change the adapter's mode, approval policy, authentication or model.

Ownership is fenced at **amux input admission**, not at provider application. Already accepted work may continue under its original IDs after handoff. Force changes authority only; it never interrupts or kills the adapter. Ordinary pending cancellation invalidates the old control, grants nobody and retains accepted work. A second foreground prompt is busy, not steering or an implicit queue.

Cooperative yield reserves the unowned gap for that requester; accepting its offer is explicit and may happen before the original prompt finishes. Questions, permissions and admitted work stay on the same live connection/session. Conditional keep/withdraw preserve the human's control; focus or activity changes can retract an offer, and reacquisition policy stays outside amux. A borrowed control does not acquire the original attachment's activity provenance. Dismiss the ownership panel with **Esc** before returning to the conversation editor; acquiring control never submits an old draft.

Default inspection, global snapshots and events are metadata-only. Transcript content is an explicit bounded incremental read with gap indicators. Closing a native shell does not close a sibling ACP view; explicit `acp-close` and daemon shutdown end the owned ACP lifetime. Unknown process-identity cleanup stays incomplete, retained and charged rather than being advertised as successful cleanup.

ACP pipe workers have independent cancellation: an escaped process retaining stdio cannot keep close or daemon shutdown waiting for EOF. The transport owner retains the original three endpoints through known-tree termination and worker joins, including idle-writer and early-reader exits. Final endpoint probes, not worker activity, determine cleanup certainty: a still-held or unclassifiable endpoint keeps cleanup failed/incomplete and its target slot charged. Cancellation does not rewrite successful I/O as failed delivery. This is not a claim that amux can identify or terminate every orphan.

Local deterministic-peer CLI/controlling-PTY evidence is in [transport verification](specs/verifications/e04s01-build.md), [interaction verification](specs/verifications/e04s02-build.md), [ownership verification](specs/verifications/e04s03-build.md) and [complete-broker verification](specs/verifications/e04s04-build.md); local peers alone do not prove provider compatibility. The separately authorized [Codex/Grok live matrix](specs/verifications/e04s04-live-codex-grok.json) records exercised OAuth/model/native/API/allow/deny results and failed attempts; it is not approval for new live runs or ACP publication.

Terminal-only input remains **best-effort**: unknown native/application settlement blocks a safe grant, even after visible output or whole-turn completion. ACP does not retrofit a fence into an upstream native TUI. The thin non-ACP integration API remains deferred until solid ACP acceptance.

## Menu and native interaction

The Ratatui interface has a **three-row title/tab bar**, a **Menu** button at the top right, and an edge-to-edge native terminal below it—no footer in the application pane. **F12** enters Menu navigation; **Ctrl-B** remains the default alternate prefix. Ordinary Esc, j and k belong to the native application. F12 is not an ASCII control character, but it is an application-bindable terminal escape sequence, not a universally unused key. macOS may require Fn. Configure `mode_key` as F1–F24, case-insensitively, or `none` to use only the prefix.

Menu is command/navigation mode; search and form inputs are explicit literal-text modes. Hierarchical rows show native/ACP type, useful names, project, Git/attention/exit status and leaf-preserving directory paths. Search covers session name, project, actual cwd and Git branch/commit. The action bar offers **New…**, **Save project**, **Projects** and **Actions**. Native and ACP names can be changed without replacing the live process/session.

Menu and saved-project action buttons sit side by side when their labels fit, stack vertically on narrow displays, and use compact rows when short. Press **?** or **F1** outside text fields for contextual, wrapping, scrollable help; **Escape** closes only help and preserves the search/draft. Arrows, j/k, PageUp/PageDown, Home/End and the mouse wheel scroll help. Menu help also retains the complete last action/error at its end. Ctrl+/ and distinct enhanced Ctrl+? remain optional aliases; legacy Backspace stays Backspace. Ownership warnings retain drawing/input priority, and covered keys, paste and clicks are consumed without replay.

Returning from Menu repaints the client canvas once, including background colors. It does not clear the managed application, restart its process or resend intercepted input.

| Menu action | Key |
|---|---|
| Select / open | j/k or arrows; g/G or Home/End; Enter |
| Return to current session | i / Escape |
| Enter literal search / leave search | / / Escape |
| New terminal or ACP, with explicit project placement | n |
| Save selected session's project | s |
| Saved projects | p |
| Session actions, including tree operations and reserved-key delivery | a |
| Rename native/ACP session | r |
| Confirm live close / remove exited native record | x |
| Request or manage control / explicit force panel | c / f |
| Detach this TUI | q / Ctrl-Q |
| Contextual help | ? / F1 |
| Send one literal reserved key through current control | Mode key twice / prefix twice |

Legacy Ctrl-N/O/A/S/G/T/P/R/D/X commands remain optional accelerators. Reported key-repeat events do not count as a deliberate second mode-key press; legacy terminals cannot distinguish every hardware autorepeat from another Press. After other navigation the mode key simply returns to the session. No intercepted input is buffered for later replay.

Closing a live native or ACP session asks for confirmation against its captured target. **Keep working** is the default; **Detach UI** leaves work running. ACP confirmation retains the original control/generation and fails rather than borrowing a later grant. Exited native records are removed without a live-process warning. `amux name ACP_VIEW_ID research` also renames the exact ACP view without changing its peer, tuple or pending work.

Upper F13–F24 recognize explicit extended-protocol keys and decoded legacy Shift-F1–F12 aliases. Legacy Shift-F3 is indistinguishable from a cursor-position reply in the terminal parser: F15 requires an unambiguous extended report or a different configured mode key. Deliberate pass-through uses the managed `xterm-256color` terminfo encoding, preserving additional Control/Alt modifiers. Native input stays authority-gated through both double-key delivery and Actions.

When a terminal exits, its view shows its actual exit code and **Close window** / **New terminal** buttons, not an ownership-resynchronization error. **X** removes its exited record; **N** or **Enter** starts a terminal from retained context. Removing the last native record keeps surviving ACP sessions selectable; only a genuinely empty client shows New terminal and Exit actions. Exit detaches the UI and the daemon keeps running. In Menu navigation q detaches; in search q, j and k are literal text. The list's mouse wheel moves selection, and exit/empty-card buttons use the same responsive layout.
Surface/action evidence: [modal Menu, ACP and project verification](specs/verifications/e07-ux.json), [UI verification](specs/verifications/UI-ratatui.md), [empty-window navigation](specs/verifications/UI-empty-switcher.md), [Menu/project workflow](specs/verifications/menu-projects.json), [earlier responsive Menu and installed-client proof](specs/verifications/responsive-menu.json). E07 local verification does not imply that its build is installed or published.

Mouse wheel and Shift-PageUp/PageDown scroll locally; typing returns to the live surface. Shift-drag selects text even when the foreground application requests mouse reporting; selection copies through the host terminal's OSC 52 support. Managed-application clipboard requests are not forwarded; client selection is the supported clipboard path. Focus, scrolling, selection and picker state belong to this client. Background terminals retain their last size, initially 80×24; opening a picker does not resize them. Only the visible surface follows client resize. If multiple local TUIs show the same terminal, the last attachment/resize determines its PTY geometry.

Each TUI registers its attachment on the original persistent connection and reports selected-terminal and outer-focus views; focus starts unknown. An unowned, unreserved, settled target may be acquired normally.

Local control is the default and needs no “Controlled here” badge or routine acquisition message. The ordinary project/session header stays visible unless a control interaction needs attention. Locked, pending/reserved, resynchronization, force-risk and conditional-handoff notices remain visible; the ownership panel still exposes detailed state.

All application keys, repeats, literal prefixes, paste and application mouse input require current control. Observers keep local navigation, scrolling and selection. Rejected or intercepted input is never replayed after acquisition.

**Prefix Ctrl-L** opens ownership request, owner-response and handoff progress actions. Hiding the panel preserves its claim. Yield and Release capture the original authority and exact requester, require complete presentation, an input-empty boundary and an armed repaint, and never reuse a queued gesture for a later request. **Preparing…** is not actionable; use a fresh deliberate action once armed. Release grants nobody and does not interrupt work. An incoming notice does not turn ordinary typing into consent; cancellation remains deliberate.

**Prefix Ctrl-F** opens explicit force confirmation against fresh ownership facts. Force does not interrupt a turn or kill a process. Stale authority requires resynchronization, never implicit force, reconnect or replay.

Conditional idle/background requests come from external orchestration. Native warnings require current draw-bound consent; human activity aborts the attempt without automatic retry. Keep/withdraw preserves the human's current control.

Detaching marks the attachment departed but does not release ownership or cancel its claim. Use explicit release/handoff actions. Unknown or departed attachment facts cannot establish safe conditional eligibility.


```sh
amux config '{"mode_key":"F11","prefix":"ctrl-a","projects_dir":"/home/me/projects"}'
amux detach       # request all currently attached TUIs to detach; processes remain running
amux shutdown     # deliberately terminate managed processes and stop the daemon
```

Use q in Menu navigation or Ctrl-Q in amux-owned UI to detach only the current TUI. External detach is asynchronous and does not release ownership. Reattachment starts after the latest detach generation.

## Input responsiveness

The TUI reuses its local connection, requests changed native rows and caches prepared styled cells instead of decoding the retained grid every frame. Native input gets an immediate presentation opportunity, with brief 1 ms interaction polling and 8 ms idle polling. Actual release-TUI PTY measurements on macOS: stock-Zsh echo medians **2.01 / 3.94 / 6.77 ms** at 120×40 / 300×103 / 500×163; 500×163 raw 24-byte burst **3.98 ms**. See [second-pass measurements and full-refresh outliers](specs/verifications/immediate-input.md). These measure emitted terminal rendering, not physical monitor paint. Cold full-refresh samples still reached 126–134 ms. Persistent clients expire after five seconds without a request: a paused or output-blocked TUI may disconnect, but its managed processes remain available for `amux attach`.

Advanced version1 API clients may opt in with `persistent:true`. For live `screen` requests, `row_updates:true` plus `since` can return changed-row deltas with complete metadata; retain the same viewport/base generation and apply `row_indices` to `cells`/`lines`. Viewport changes require a full read. Legacy CLI/extension callers continue receiving ordinary full frames/one-shot EOF.

The first runtime/API optimization requires restarting an older pre-change daemon; `amux shutdown` terminates its managed processes, so finish valuable work before doing that. This latest styled-cell/pacing optimization is **client-only**: rebuild with `cargo build --release` and reattach the new TUI to a daemon already supporting row updates, without restarting its processes. Verification used private daemons and did not restart or replace your live daemon.

## Shell execution versus typing

The `input` and `execute` commands require `--control` containing serialized **current ControlHandle JSON**. A controller registration credential, claim, offer or copied public ownership receipt cannot authorize input. Mutation targets use the terminal ID returned by `state`, not a name that may later resolve differently.

Register an external controller once and explicitly acquire an unowned target; this example uses `jq` to preserve JSON types:

```sh
controller=$(amux api '{"op":"controller-create","role":"external","label":"my script"}' |
  jq -ec '.result.controller')
state=$(amux state main)
terminal=$(printf '%s' "$state" | jq -er '.result.id')
observed=$(printf '%s' "$state" | jq -ec '.result.ownership.observed')
grant=$(amux api "$(jq -cn --argjson controller "$controller" --arg terminal "$terminal" \
  --argjson observed "$observed" \
  '{op:"ownership-claim",controller:$controller,terminal:$terminal,observed:$observed,mode:"cooperative",when:{kind:"now"}}')")
control=$(printf '%s' "$grant" | jq -ec 'select(.ok and .result.kind=="control") | .result.control')
```

Proceed only if a current control was returned. If the result is a pending claim, inspect that same claim and arrange explicit owner yield and offer acceptance; do not send input, repeat the claim or force automatically. Choose an OMP terminal instead of `main` before acquisition when sending application messages.

```sh
amux input "$terminal" --control "$control" 'a message for the foreground application'
amux input "$terminal" --control "$control" --key enter
amux input "$terminal" --control "$control" --key ctrl-c
amux input "$terminal" --control "$control" --paste 'multiline text'
amux input "$terminal" --control "$control" --bytes '27,91,65'

amux execute "$terminal" --control "$control" 'export MODE=debug; cd src; print $PWD'
amux execute "$terminal" --control "$control" 'print later' --queue
```

Input goes to the current foreground application. `execute` is shell **source**, not typed text: only an empty confirmed primary Zsh prompt can accept it. Busy applications, earlier pending submissions and partially entered lines are rejected; queueing is explicit. The adapter rechecks native prompt, buffer, generation and typeahead, with an input-epoch handshake that prevents a queued wake key from following user input into a foreground application. Ctrl-C records interrupted source completion without replacing native hooks. Exited/backpressured input and oversized paste display an error without closing the TUI.

New terminals default to children of the project's main terminal, even when captured from another terminal. They inherit captured cwd and exported environment, including export removals. While the source shell is busy, launch results report a **stale** capture. Aliases, functions, local variables, job tables and shell options are not cloned. A lost adapter disables structured execution; native raw interaction remains available. Details: [integration contracts](integrations/README.md).

Manual `omp` inside a managed shell loads the reporting extension. Existing `omp` functions are delegated to with the extension argument; existing aliases remain intact and must forward `-e "$AMUX_OMP_EXTENSION"`. The picker/CLI action uses `command omp -e "$AMUX_OMP_EXTENSION"` directly to avoid alias/function interception or duplicate extension loading. Use a project recipe for a customized launcher. Other OMP releases are not claimed compatible.

## Projects, recipes and hooks

Choose **Menu → New… → Terminal → New independent project**, `cd` to an existing directory, then choose **Menu → Save project**. The form previews the exact canonical cwd/repository root that will be saved. A changed context is rejected before mutation instead of silently bookmarking a different directory. Saving never creates or moves directories, restarts a session or clones interpreter state. Native and ACP creation both offer explicit current-project versus independent-project placement; independent placement does not inherit project hooks/recipes.

**Menu → Projects** lists saved projects and directories. Open reuses an existing represented native session or live ACP session without rerunning project-open hooks; an ACP-only project does not gain a shell. An ACP-only project whose peer has ended can reopen with a shell. Edit provides separate name, ordered hook source/timeout and recipe name/command controls, including add/edit/remove; raw JSON is not required. Changes stay in a local draft until Apply submits a complete valid patch. Tab/BackTab, mouse, contextual help and Escape work consistently; errors preserve the draft. Forget defaults to Cancel and removes only the bookmark, never files or native/ACP work.

```sh
amux projects list
amux projects save compiler
amux projects add scratch --directory /path/to/existing/work --config '{"recipes":{"agent":"omp","check":"cargo check"}}'
amux projects config scratch '{"recipes":{"agent":"omp","check":"cargo check"}}'
amux open --project scratch --recipe check --name checks
amux projects open scratch
amux config '{"daemon_start_hooks":[{"source":"printf daemon-started","timeout_ms":1000}]}'
```

`projects add` registers the caller's cwd, or an explicit existing `--directory PATH`; missing paths/files are rejected, never created. Initial `--config` accepts hooks and recipes. Saved projects keep stable IDs. UI Save resolves the exact selected native/ACP session; CLI Save without a terminal retains the default main-shell rule. Raw `project-save-preview` returns `{project, context}`, and `project-save` accepts `expected_context` to reject intervening context changes atomically. Explicit re-save updates that project's location/name. Configuration patches replace the supplied hooks array or recipes map rather than merging entries.

`amux projects list` returns only saved project definitions; `amux snapshot` also includes unsaved runtime projects. `amux projects remove PROJECT` forgets a saved definition without deleting its directory/files, terminating native/ACP sessions, cancelling admitted work or changing its runtime identity. The retained runtime project becomes unsaved; after daemon restart its removed definition does not return. Removing an unsaved entry fails. Reusing a name can make runtime selectors ambiguous, so use exact project IDs when necessary. This differs from top-level `amux remove TERMINAL`, which removes an exited native terminal record.

The former top-level project commands are replaced, without aliases: `project-save` → `projects save`, `project-new` → `projects add`, `project-open` → `projects open`, `project-config` → `projects config`. Raw `project-new` now requires an explicit existing `directory`; `project-save` accepts `terminal` to identify the actual session, and rejects a mismatched project. New session uses `open` with `new_project: true`, inheriting cwd/exports without joining the old project. New UI save/session actions require snapshot `project_save_terminal: true` and refuse an older daemon rather than silently targeting its main shell. List/remove also require the updated daemon. Installing a client does not replace a running daemon; finish its managed work before a separately approved upgrade.

Hooks run as noninteractive `/bin/sh -c` workers, in a single queue: daemon-start hooks first, then project-open jobs, each hook in configuration order. Daemon hooks use daemon startup cwd; project hooks use project context. Both inherit daemon exports, not mutable interpreter internals. Default timeout is 30 seconds, configurable from 1 ms to 300 seconds. Stdout/stderr and failures are visible in snapshot diagnostics/events; a failed hook leaves a usable shell and later hooks continue. Reattachment runs neither hook category. Opening an already represented project is reattachment; close and remove its terminal records before a fresh project-open, or reopen after daemon restart. Shutdown cancels the active hook's owned process group and skips pending hooks instead of waiting for its configured timeout.

During native reopening, explicit project creation/opening stays available within the normal quota. Its hook jobs wait behind the reserved daemon-start job until a native or fallback shell is available. Failed first-shell creation cancels daemon-start effects; shutdown cancels the pending startup gate and queued work.

Configuration and hooks live under `$XDG_CONFIG_HOME/amux/config.json` (or `~/.config/amux/config.json`). A private, bounded `agent-sessions-<socket-identity>.json` sibling stores only reopening metadata: conversation ID/path, cwd/project/name, supported agent/profile identity, acknowledged choices and an old-process identity pin. It stores no transcript, credentials, environment, queued input or old control. The journal is locked, owner-only, bounded to512 records/4MiB and replaced atomically with file/directory sync. Separate agent-native stores may contain conversation history; managed native Codex uses the journal's `.agents` sibling. No PTYs, process tree, screen or interpreter state are restored.

Configuration saves use a stable sibling lock and compare the exact configuration loaded by the daemon before replacing it. `config-busy` means another cooperating writer currently holds the lock; retry after it finishes. `config-conflict` means disk changed since this daemon loaded/saved it; the mutation is rejected rather than overwriting other projects or settings. There is no live full-config reload: finish valuable work before deliberately restarting the affected daemon to load the current file. Starting another daemon against that file also loads the current configuration, without stopping the existing daemon. File and containing directory are synced before success; a directory-sync failure after replacement reports an uncertain save and requires reload before another save. External editors that ignore the lock are not serialized.

`amux config '{"terminal_limit":64}'` sets daemon-wide admission (default 64, valid 1–256). Live terminals, retained exited records and pending opens all count. To free capacity, close a terminal, wait for `exited`, then remove its record; a failed open refunds its reservation. Lowering the limit never destroys existing terminals. This is a terminal-count bound, not an aggregate byte quota.

## Automation

API commands print versioned JSON envelopes and return nonzero on errors. UUIDs are stable within daemon lifetime; saved project UUIDs persist. Names are selectors, and ambiguity is an error.

Raw `amux api` exit status zero means **request success**, not an ownership grant, offer acceptance, shell completion or task success. Check `result.kind` and the current private state. Ordinary cooperative requests preserve the current owner's authority until an explicit yield/release; a ready offer is reserved but still requires explicit acceptance. Public `ownership.observed` is a compare receipt, never authority; public snapshots/events deliberately omit private handles.

Keep controller/claim capabilities in the caller's private memory and reuse them across independent one-shot calls. After a timeout, broken socket or lost accept response, inspect rather than replay:

```sh
amux api "$(jq -cn --argjson handle "$controller" \
  '{op:"control-handle",handle:$handle,method:"read"}')"
# If a control was granted, recover it from result.controls.
# To give up that current control explicitly (without stopping running work):
amux api "$(jq -cn --argjson handle "$control" \
  '{op:"control-handle",handle:$handle,method:"release"}')"
```

Reading a claim/controller never accepts an offer or reacquires ownership. Do not repeat an uncertain accept: an already-consumed offer rejects duplicates.

Cancelling an ordinary pending claim invalidates old control, leaves the target unowned, grants nobody and retains outstanding calls. A grant completed before cancellation remains valid; release it explicitly.

Force compares fresh ownership/reservation facts and warns `prior-calls-may-apply`, even with no listed calls. Force, cancellation and release never implicitly interrupt a turn, kill a process or erase prior work.

A PTY write acknowledgement is not application settlement. Unknown application input can block safe handoff. Conditional keep/withdraw retains human control; reacquisition policy stays external.

See the [wire contract](specs/contracts/pty-control.md) for exact capabilities, public clock, eligibility, consent, request/result shapes and errors.

### Codex broker acceptance boundary

The concrete broker is pinned to Codex 0.160.0. Its closed operations are `agent-open`, `agent-inspect`, `agent-prompt`, `agent-answer`, `agent-permission` and `agent-interrupt`; this is not a generic provider RPC relay.

The native frontend creates the conversation on the owned backend. The observer binds its live identity without an API-created conversation or setup model turn, then explicitly subscribes when the first requested turn materializes history. Native title auxiliaries are not replacement conversations.

Mutations require current PTY control and exact instance/session identities; answers, permission decisions and interrupts also bind their applicable turn/item/request identities. Receipts report admission and retained outcomes, not task completion.

Separate real-account checks passed native/API start and same-turn steer, a genuine native-originated question answered structurally, permission allow/deny with actual filesystem effects, and exact-turn interruption with process survival. Deny uses request-only `decline`, never `cancel` or a session grant.

Complete-broker acceptance remains unpassed. Unknown winning answers or native/application settlement must block safe grant; force-only results cannot substitute. Actual checks and limits live in [Build evidence](specs/verifications/e03-integration-build.md).

#### Accepted upstream Codex limitations

Raw PTY input has no generic application-consumption acknowledgement. With unmodified Codex 0.160.0, even the startup trust confirmation can leave native input unknown and cooperative handoff blocked indefinitely. amux keeps that blocker: PTY delivery, redraw, request resolution or turn completion never manufactures a safe grant. Explicit force remains a separate warned operation, without implicit interruption or process termination.

Codex can also display a losing locally submitted question answer as accepted. Its resolved notification does not identify the winner; do not treat native question history or a response-write receipt as authoritative winner proof. The user explicitly accepted these temporary upstream limitations and deferred complete-broker/all32 acceptance to future ACP integration. The recorded failed check remains failed, not waived into a PASS. ACP adoption must actually prove the native input cut and truthful answer presentation before closing the issue. No Codex patch, replacement UI or ACP connector is included here. See [accepted limitations](specs/security/EXCEPTIONS.md).


```sh
amux state agent
amux screen agent
amux screen main --offset 100 --limit 40
amux name build compiler-build
amux reparent agent compiler-build
amux promote agent
amux close agent
amux wait agent exited
amux remove agent
```

An agent is **settled** only after an explicit root `agent_end` with no continuation, retry or pending question/approval, and a matching completed generation. Turn completion, initial idle, process exit and shell readiness are different states. A crash or expired reporting lease is unknown, never successful completion. Child agents cannot overwrite root status. Settled means the reported agent cycle ended, not that its task succeeded or its changes are correct.

```sh
amux snapshot                                  # retain its result.seq
amux events --after 123 --timeout-ms 30000      # replay / long poll
amux wait agent agent-working --after 123
amux wait agent needs-input --after 123
amux wait agent agent-settled --after 123
amux wait main shell-ready
amux api '{"version":1,"op":"screen","terminal":"main","offset":0,"limit":24}'
```

Take a snapshot **before** sending work; chain waits using returned sequences. With `--after`, waits observe retained transitions even if later state has changed; inspect `observed` separately from current `state`. Completion waits require a new explicit root `agent_end`; idle heartbeats and maintenance do not count. Without `--after`, a wait is a current-state query. An event gap includes a replacement snapshot: resume from that snapshot's `seq`. A wait gap is an explicit error requiring a new snapshot. Active waits consume unmatched events as they go. Structured events contain no high-volume PTY byte stream.

Shell-context events may contain exported credentials. Treat the local control API and unfiltered event logs as privileged same-user data; do not publish them.

## Validation and limits

```sh
cargo test -- --nocapture
cargo clippy --all-targets -- -D warnings
cargo build --release
bun test integrations/omp
zsh -n integrations/zsh/adapter.zsh
```

[Architecture and API ownership](specs/tech-architecture/DESIGN_PLAN_LATEST.md), [implementation plan](specs/epics/e01-native-terminal/epic.yaml), and [verification evidence](specs/verifications/) record the installed bigpowers workflow and observations.

`vendor/crossterm` retains the pinned MIT-licensed 0.29.0 release with a local native-input repair: preserve pending input/resize readiness and descriptor flags shared with the parent shell. Safety-critical view/recovery/consent checks use an explicit Empty/Pending/Inconclusive probe, not filtered zero-time `poll(false)`. Internal terminal replies cannot hide queued human events; incomplete sequences and bounded reader yields keep authority fenced while resize/disconnect handling progresses. [Repair/provenance evidence](specs/verifications/e06-native-input-diagnosis.json) and [input-boundary verification](specs/verifications/e06-review-round10-repairs.json) explain the released-source patch rather than an unrelated upstream upgrade or the alternate zero-timeout-broken backend.

Current [ACP daily-use verification](specs/verifications/e06-daily-use-verify.json) includes451 workspace tests,44 distinct nonlive E06 cases, three fresh100/100 source/security reviews and [one genuine installed-OMP subscription run](specs/verifications/e06-omp-live-final.json). Native question answering, API handoff with exact permission deny/allow effects and original session/process/work continuity passed with normal owned cleanup. The live check used four MAI Flash/low requests, not synthetic model responses; this does not waive the separate terminal-only unknown-settlement limitation.

Latest [substrate verification](specs/verifications/substrate-reliability.md): **210 Rust tests and 8 OMP tests pass**, plus the explicit strict independent-showcase check, workspace Clippy, debug/release builds, Zsh syntax and private native CLI/TUI lifecycle smokes. The report retains an earlier queued-execution timeout whose cause was not established; the instrumented acceptance suite, final workspace suite and 24 cancellation probes passed without weakening tests. [Earlier latency/UI measurements](specs/verifications/immediate-input.md) and [MVP evidence](specs/verifications/MVP-e01.md) remain historical baselines, not fresh performance results. **The e01 independent-review cap remains exhausted (5/5); no new independent PASS or landing authorization is claimed for this fix branch.**

Bounded resources: daemon-wide terminal admission defaults to 64 (configurable 1–256, counting retained exits and in-flight opens); 50,000 scrollback rows per terminal; 64 KiB per raw input request with 128 queued writes; API request 1 MiB / response 64 MiB; PTY geometry 2–500 columns and 2–300 rows; event history 2,048 entries / 4 MiB; Git cache 256 workspaces with 100 file details per change category (full counts retained); 128 active agent identities per terminal; 256 managed execution records, pruning completed/rejected records first and exposing `executions_discarded`. Git overflow and pending-execution capacity produce explicit unavailable/busy results. All hook, launch and enqueue diagnostics share the latest-20 retention window. Diagnostic strings, including command/source and each visible output channel, are limited to 16 KiB at UTF-8 boundaries; source/command/output truncation is marked.

Grapheme composition uses the native narrow/wide cell model, with up to 4 KiB of final-cluster working text. Crossing that bound starts a new native scalar segment without discarding text; joining across the oversized boundary is not preserved. Upstream cell combining storage remains variable-sized.

No crash recovery, interpreter cloning, tiling, remote daemon protocol or graphical client. Synchronized updates, native mouse/paste, Unicode, alternate screens, scrolling and resize were exercised on macOS; no Linux runtime claim. Host fonts and terminal clipboard facilities affect presentation. Process cleanup uses kernel birth identities and freeze/rescan of known descendants, including foreground and separate-session jobs. Shutdown disconnects slow control clients and drains in-flight creation/cleanup before exit. Processes already orphaned outside observed ancestry/session cannot be guaranteed captured. OS permission changes or uninterruptible kernel I/O can prevent termination; freeze waits are bounded and emit diagnostics to the daemon log before best-effort signals. This is a same-user control environment, not a sandbox.
