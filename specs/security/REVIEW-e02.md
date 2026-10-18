# e02 terminal-showcase security review

type: security-review
context: terminal-endpoint

## Boundary and threat focus
Standalone stdin/stdout terminal application with local POSIX TTY management. No network listener, daemon socket, renderer/model access, remote commands, shell interpolation, shared-memory/file graphics or authentication product. The endpoint can inherently generate keyboard events; the application cannot authenticate a physical user's keystroke over ordinary terminal I/O. Safety focuses on preventing framed response/paste/mouse bytes from accidentally becoming consent actions, sanitizing endpoint-controlled display text, bounding resources, and restoring owned terminal state.

## Findings and remediation evidence
- Legacy mouse coordinate bytes reaching integration hotkeys: round-1 must-fix. Discovered settable SGR gates tracking; legacy byte/UTF-8 payloads are opaque. App/stream/modes regressions and actual inherited tracking/encoding restoration.
- Lost paste provenance at bounded startup/exercise queues: round-2 must-fix. Reproduced in an actual PTY: pasted C changed clipboard capability to Visual. Parser `PasteData` chunks now retain provenance independently of lossy boundary events; three producer-overflow cases pass without C/N/T/W/Q actions. No independent round-2 PASS claimed.
- Nested transport framing/paste continuation and malformed SS3: round-3 must-fixes corrected with persistent bounded transport/semantic state and payload quarantine. Parser/application regressions and real PTY boundaries passed; final dual review returned no findings.
- Round-4 UTF-8 C1 delimiter injection and completed-paste command suppression: reproduced via actual headless fixtures and corrected. Bare paste no longer interprets transport/quoting; UTF-8 continuation and delimiter overlap have explicit parser/live regressions and bounded scalar corpora. Post-fix native integration kept all consent states Unknown, accepted later E/q and restored terminal state; final dual review passed.
- Malformed/expired reply tails: reproduced and corrected; tails remain quarantined until a terminator and cannot become clipboard hotkeys. Ctrl-C remains available outside opaque paste/mouse payloads.
- Automatic legacy image emission: round-1 consent defect fixed. Only advertised Kitty is automatic; Sixel/iTerm require G and at most sixteen updates per opt-in. Text erasure/alternate-screen exit is not advertised as raster-memory reclamation.

## Source-to-sink controls
Endpoint identity/evidence and unmatched diagnostics are bounded and sanitized before ANSI presentation. Typed replies validate exact live parameters/IDs/deadlines before consuming requests. Missing/late/ambiguous responses do not establish unsupported features; graphics acknowledgment never establishes correct rendering. Payloads are bounded to 8192 bytes, wrappers to four layers, response fixture input to 1 MiB, evidence/log/deferred queues to fixed counts, viewport presentation to 500x200, and direct rasters to 64x32. Clipboard writes, title changes, notifications and cwd hints are authored demonstration bytes gated by explicit integration keys. No clipboard query or automatic modification. CLI-selected report paths and response fixtures are intentional local-user file access, not endpoint-derived paths. Reports contain no clipboard contents or live key transcript.

## Observed proof and limits
Actual browser/PTY interaction intercepted OSC52: none for mouse coordinates, malformed CSI, escape-looking paste, wrapped-paste/SS3/privacy/designation payloads or final Unicode/literal-header/overlapping-delimiter cases; one static marker only after the explicit C key in the earlier consent stage. q/Ctrl-C/signals restored terminal input state; inherited DRAG/SGR_PIXELS returned after exit. Native regressions test bounded protocols, mode group ownership, live paste/resize/signal behavior and process cleanup. Final post-review Rust gates passed 177 tests with four intentionally ignored, clippy/build/rustfmt, the ordinary-amux regression and six headless checks. Both fresh final reviewers passed with no findings. No vulnerability scanner, dependency audit database, minimum-toolchain/Linux/SSH/native-emulator certification or physical image/font correctness is claimed. Historical e01 security review is preserved in REVIEW.md.

## Scoped verdict — PASS
No unaddressed high/must-fix findings in the e02 boundary. Complete fifth-round static review records and AND-gate accounting: ../verifications/REVIEW-e02.md. Runtime/regression evidence: ../verifications/terminal-showcase.md. This is not a dependency-database audit or physical-emulator certification.
