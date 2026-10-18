# e02-round-3-a

{
  "findings": [
    {
      "severity": "must-fix",
      "title": "Preserve decoded framing and paste state across tmux wrappers",
      "body": "finish_string creates a fresh inner Parser for each tmux wrapper, calls expire(), and then discards it. Consequently, a wrapper containing a bracketed-paste opener followed by a subsequent wrapper containing C produces Csi(\"200~\") followed by Input(\"C\"), rather than PasteData(\"C\"). The application's Inputs iterator and command dispatcher then treat that pasted C as clipboard consent in the integration demo; pasted Q can similarly exit. Partial CSI/string framing is also lost between wrappers. Preserve the decoded stream's parser state across wrappers, including paste provenance, instead of expiring it at every outer terminator, and add fragmented/multiple-wrapper regressions through the command consumer.",
      "file": "terminal-showcase/src/protocol.rs",
      "line": 632
    },
    {
      "severity": "must-fix",
      "title": "Quarantine malformed SS3 tails instead of replaying them as hotkeys",
      "body": "The invalid-SS3 branch resets to Ground and recursively reprocesses the offending byte. For example, ESC O 1 ; 2 C emits a Malformed event followed by Input(\"1;2C\"); Inputs and command then dispatch C as an explicit clipboard action in the integration demo. ESC O space Q similarly pauses and exits instead of keeping the malformed sequence opaque. This bypasses the malformed-frame consent boundary that CSI and string frames already enforce. Either parse supported parameterized SS3 sequences or discard the malformed SS3 through its final byte without replaying payload bytes as ordinary input, retaining the documented emergency Ctrl-C behavior; add consumer-level regressions.",
      "file": "terminal-showcase/src/protocol.rs",
      "line": 397
    },
    {
      "severity": "should-fix",
      "title": "Require image emission before recording a graphics visual specimen",
      "body": "The app calls specimen whenever Graphics::frame returns any bytes, but nonempty output does not imply an image was transmitted. Entering a fresh Kitty experiment in phase 6 or 7 emits text/deletion commands without creating either image, and entering a fresh Sixel/iTerm experiment in phase 7 emits only erase/text commands because visible is zero. An explicit G selection during those phases therefore marks an unexercised graphics protocol Visual and admits P/F verdicts before any raster specimen exists; pausing preserves that false evidence indefinitely. Start newly selected experiments at a populated phase or return an explicit image-emitted indicator from Graphics::frame and gate specimen/seen on it. Add a regression for first entry into each…
      "file": "terminal-showcase/src/app.rs",
      "line": 692
    }
  ],
  "assessed_items": 20,
  "must_fix": 2,
  "should_fix": 1,
  "score": 85,
  "verdict": "FAIL",
  "smells": "The large app::run combines scheduling, event dispatch, consent, graphics lifecycle, evidence and cleanup, making state interactions difficult to inspect. Duplicated Code appears in the Kitty query bytes in Discovery::queries and Graphics::query, and in mouse-group constants shared conceptually by discovery, terminal management and tests. Primitive Obsession appears in string capability IDs and positional canvas tuples. These are maintainability observations, not additional correctness findings or score deductions.",
  "verification": "Completed read-only static review of every scoped Rust implementation file, all eleven permanent integration-test files and the application unit tests, workspace/package manifest and dependency changes, README/CHANGELOG updates, the usage specification, scope, epic plan, four stories and four task files. Twenty assessed areas: (1) standalone package isolation and workspace defaults; (2) CLI validation and output routing; (3) incremental CSI/OSC/DCS/APC framing; (4) nested-wrapper stream continuity; (5) malformed-frame quarantine; (6) paste provenance and bounded deferred queues; (7) exact query matching; (8) discovery deadlines and single-flight CPR correlation; (9) independent objective/visual evidence; (10) sanitized terminal text and JS…
  "limitations": "This review provides static causal evidence, not runtime reproduction of its findings. The passing gates and two known amux endpoint incompatibilities described in the brief were parent-reported evidence, not independently rerun; neither endpoint defect was counted as a showcase bug. Physical glyph/raster appearance, minimum-toolchain support, Linux, SSH and emulator combinations remain uncertified. Pending workflow bookkeeping was intentionally not counted as a product defect. No other review/audit reports, agent history or sibling messages were consulted."
}

[Some lines truncated to 768 chars]
