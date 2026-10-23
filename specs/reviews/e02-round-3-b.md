# e02-round-3-b

{
  "findings": [
    {
      "severity": "must-fix",
      "title": "Preserve decoded framing and paste state across tmux wrappers",
      "body": "Each completed tmux wrapper creates a fresh inner Parser, feeds its decoded bytes, immediately expires it, and then discards its state. For example, a valid wrapper containing only CSI 200~ emits a paste-start event but leaves the outer session parser in Ground; subsequent unwrapped CQ or a second wrapper containing CQ emits ordinary Input rather than PasteData. app.rs routes that Input through Inputs and command, allowing pasted C to execute the clipboard action in the integration demo and pasted Q to exit. Partial CSI/string replies split across wrappers likewise lose their quarantine state. Preserve a persistent decoded-stream parser/state across wrapper boundaries and session reads, retaining bounded nesting, and add behavior regressions t…
      "file": "terminal-showcase/src/protocol.rs",
      "line": 631
    },
    {
      "severity": "should-fix",
      "title": "Invalidate objective cursor checks when the physical viewport changes",
      "body": "The entire CPR suite constructs its cases from report.columns once and never checks physical winsize while emitting or awaiting transactions. If the terminal grows from 100 to 120 columns during startup or a manual R suite before the autowrap case, the case still writes 100 A characters followed by B and expects [10,2]; a correct endpoint instead returns [9,102], which is recorded as an objective Failed and can make --probe --check exit nonzero. Shrinking below the required height similarly invalidates fixed row expectations. Snapshot physical dimensions for the suite and check them before emission and after receiving each CPR; classify geometry-invalidated checks as Unknown rather than endpoint failures, stopping safely without reassigning an…
      "file": "terminal-showcase/src/exercises.rs",
      "line": 99
    },
    {
      "severity": "should-fix",
      "title": "Track the emitted default-color specimen for visual verdicts",
      "body": "The colors demo emits an actual CSI 39;49m default foreground/background specimen, but show_frame never registers default_colors in seen. The P/F consumer filters every verdict through seen, so even after this line has been visibly emitted, neither P nor F can mark default foreground/background; its exported visual field remains null, including when the user reports a visible defect. Add detection or explicit final-frame specimen metadata for that default-color run, only when it survives clipping/paging, and test that P/F records its independent visual verdict without changing objective query evidence. Keep palette query evidence unmarked unless a corresponding visual experiment was actually emitted.",
      "file": "terminal-showcase/src/app.rs",
      "line": 84
    }
  ],
  "assessed_items": 24,
  "must_fix": 1,
  "should_fix": 2,
  "score": 87.5,
  "verdict": "FAIL",
  "smells": "The large app::run orchestration method combines input dispatch, scheduling, evidence bookkeeping and raw presentation, making state transitions harder to inspect. Protocol mode numbers, capability IDs and command bytes are largely explicit wire/schema values rather than independently actionable Primitive Obsession defects. No additional evidence-backed bugs attributable to Mysterious Name, Duplicated Code, Feature Envy, Data Clumps, Message Chains or Middle Man were identified; no style-only findings were counted.",
  "verification": "Completed read-only static review of every scoped implementation module and all eleven permanent test files, plus package/workspace manifests, dependency patch, README/CHANGELOG changes, the feature specification, scope requirements, epic plan, four stories and four task files. Assessed 24 areas: (1) package independence/workspace defaults; (2) CLI validation and mode routing; (3) bounded headless fixtures and JSON; (4) CSI framing/recovery; (5) OSC/DCS/APC framing/quarantine; (6) nested wrapper decoding and state propagation; (7) UTF-8/C1 fragmentation; (8) paste provenance across producer phases and bounded queues; (9) legacy mouse framing; (10) read-only discovery, exact correlation and deadlines; (11) conservative evidence classificati…
  "limitations": "No tests, builds, lint, formatters or runtime experiments were run, and no repository files were modified. Findings are backed by static code paths and concrete input/resize conditions, not newly executed reproductions. Existing tests cover complete nested wrappers, direct-stream paste safety, deferred-queue overflow, post-startup resize and late CPR, but do not cover the failing combinations identified above. Parent-reported passing gates and observed amux ZWJ/origin CPR defects were treated as supplied facts, not independently revalidated; those endpoint defects were not counted as showcase bugs. Native minimum-toolchain, Linux, SSH, physical glyph/raster appearance and emulator-specific restoration behavior remain uncertified. Pending wo…
}

[Some lines truncated to 768 chars]
