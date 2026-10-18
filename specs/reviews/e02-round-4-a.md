# e02-round-4-a

{
  "findings": [
    {
      "severity": "must-fix",
      "title": "Keep UTF-8 continuation bytes opaque inside bracketed paste",
      "body": "The paste state recognizes every 0x9b byte as a possible C1 closing delimiter without tracking UTF-8 continuations. Static trace: an ordinary bracketed paste containing `Û201~C` has payload bytes C3 9B 32 30 31 7E 43; the 9B inside the valid UTF-8 character starts `9B201~`, prematurely closes paste, and emits the remaining C as Event::Input. app.rs then routes that C through Inputs/command to the integration clipboard action, or a remaining Q exits, without an actual consent key. Ground and string framing already distinguish C1 bytes from UTF-8 continuations, but paste does not. Preserve UTF-8 continuation state across paste reads and decoded transport fragments, recognizing the C1 closing marker only outside a scalar. Add all-split and bytewi…
      "file": "/Users/addomam/projects/multiplexer/terminal-showcase/src/protocol.rs",
      "line": 824
    },
    {
      "severity": "should-fix",
      "title": "Recognize combined SGR commands when recording underline-color specimens",
      "body": "show_frame only records underline_color when a rendered row contains an SGR sequence beginning with `ESC[58:` or `ESC[58;`, but every underline-color specimen in presentation.rs:786-790 combines the parameter with the underline style (`ESC[4:3;58:2::...m`, `ESC[4;58;5;45m`, and `ESC[4;58;2;...m`). None matches these needles, and there is no other producer of underline_color evidence. Consequently, even after all typography pages are displayed, the capability stays Unknown and P/F silently skips it because app.rs:487-492 filters visual verdicts through seen. Recognize parameter 58 within the actual retained combined SGR sequences, or emit separately detectable underline-color SGR commands; add a render-to-show_frame-to-P/F regression that asser…
      "file": "/Users/addomam/projects/multiplexer/terminal-showcase/src/app.rs",
      "line": 89
    }
  ],
  "assessed_items": 24,
  "must_fix": 1,
  "should_fix": 1,
  "score": 91.66666666666667,
  "verdict": "FAIL",
  "smells": "Primitive Obsession: visual specimen eligibility is inferred from literal ANSI substrings, which misses the combined underline-color commands above. Duplicated Code: PTY child guards and output-drain/deadline harnesses recur across live, colors, overflow, late, modes and wrapped tests; this is a maintainability observation, not an additional correctness finding or score deduction. The standalone parser/evidence/presentation/terminal module boundaries otherwise maintain the independent endpoint oracle.",
  "verification": "Read-only static review; no builds, tests, lint, formatters or smoke checks executed. Read the brief, conventions, implementation modules, package/workspace changes, usage/changelog, scope, epic plan, all four stories/task files, and permanent parser, evidence, presentation, graphics, PTY, resize and multiplexer regressions. Traced parser events through live consumer dispatch and specimen eligibility through visual verdict/export. Assessed 24 areas: (1) standalone boundary/default preservation; (2) CLI validation/offline behavior; (3) exact query correlation; (4) conservative support semantics; (5) objective/visual precedence; (6) endpoint text sanitation/evidence bounds; (7) CSI/SS3 malformed recovery; (8) OSC/DCS/APC framing and limits; …
  "limitations": "Findings are evidence-backed static execution traces, not newly exercised reproductions. Parent-reported successful native/workspace/headless gates were accepted without rerunning or consulting other reviews/audits. Physical font/raster appearance, native minimum toolchain, Linux/SSH and emulator combinations remain uncertified, as stated in the brief. Pending workflow bookkeeping was intentionally not treated as a product defect."
}

[Some lines truncated to 768 chars]
