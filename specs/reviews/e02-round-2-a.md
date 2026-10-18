# e02-round-2-a

{
  "findings": [
    {
      "severity": "must-fix",
      "title": "Preserve paste boundaries when deferred input queues overflow",
      "body": "During startup, probe() silently drops every unmatched event after its deferred queue reaches 128 entries, including the bracketed-paste start marker. For example, 128 focus events followed by CSI 200~ fill this queue and lose the marker while the persistent Parser enters Paste state. Paste text arriving during capture_margins() or the objective exercises is subsequently retained because those producers allow 512 entries. The interactive consumer initializes paste=false and relies exclusively on the queued start marker, so that text is dispatched as ordinary keyboard input: with --demo integration, a pasted C invokes the clipboard write, and pasted Q exits. receive_cursor_response() has the analogous marker-loss problem at its 512-entry limit.…
      "file": "terminal-showcase/src/app.rs",
      "line": 60
    },
    {
      "severity": "should-fix",
      "title": "Normalize traditional control keys in the live input iterator",
      "body": "Inputs::next() constructs every ASCII input byte as a Traditional key with its raw byte value and modifiers=0, bypassing the existing normalization in graphics::traditional(). Consequently, an ordinary Ctrl-A received as 0x01 appears in the live input inspector as code 1 with no Ctrl modifier instead of code 97 with modifier 4; Ctrl-P, Ctrl-Space and Backspace similarly lose their established key semantics. observe() then marks traditional keyboard decoding Verified despite these incorrect fields. The decoder regression exercises graphics::decode_input() directly, so it does not cover the iterator actually used by the application. Share the allocation-free single-byte normalization between both paths while preserving batching, and test the liv…
      "file": "terminal-showcase/src/app.rs",
      "line": 173
    }
  ],
  "assessed_items": 24,
  "must_fix": 1,
  "should_fix": 1,
  "score": 91.66666666666667,
  "verdict": "FAIL",
  "smells": "Duplicated Code is consequential here: app::Inputs and graphics::traditional implement different ASCII-key semantics, while probe, capture_margins, receive_cursor_response and enqueue implement separate deferred-event policies. These divergences contribute directly to the findings. Primitive Obsession is present in byte hotkeys, string capability IDs and string exercise outcomes, but fixed catalogs and strict protocol decoders substantially constrain it; no additional actionable defect is established from that smell. No separate evidence-backed Feature Envy, Message Chains or Middle Man finding.",
  "verification": "Read-only static review of the scoped implementation, all ten permanent behavior-test files, package/workspace manifests and lockfile changes, README/CHANGELOG, feature specification, epic plan and four story/task contracts. Traced framed events through discovery, objective exercises, deferred queues, input decoding, hotkey dispatch, terminal output and restoration. Assessed 24 contract areas: (1) independent package/root defaults; (2) CLI options and errors; (3) headless fixtures and bounds; (4) CSI framing; (5) OSC/DCS/APC framing; (6) nested wrappers/C1/UTF-8; (7) malformed/timeout quarantine; (8) conservative query coverage; (9) live reply correlation/deadlines; (10) objective/visual evidence separation; (11) single-flight CPR and late…
  "limitations": "No tests, builds, lint, formatters, runtime probes or repository modifications were performed. The parent's reported passing gates are prior evidence, not independently exercised verification. Both findings are established by static producer-to-consumer traces; fresh runtime reproductions were not run. Physical glyph/image appearance, actual pixel-coordinate units, raster reclamation on legacy endpoints, minimum-toolchain behavior and Linux/SSH/emulator combinations remain unverified. Workflow bookkeeping and pending delivery evidence were not treated as product bugs."
}

[Some lines truncated to 768 chars]
