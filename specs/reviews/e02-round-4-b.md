# e02-round-4-b

{
  "findings": [
    {
      "severity": "must-fix",
      "title": "Recognize the outer paste terminator inside an unterminated literal wrapper",
      "body": "Static control-flow evidence: feeding `\\x1b[200~\\x1bPtmux;literal\\x1b[201~q` enters TransportState::Literal, whose branch appends every subsequent byte to paste data and exits only on ST. Consequently, the genuine outer bracketed-paste terminator never reaches the semantic paste matcher, and the following q remains PasteData. Parser::expire also leaves this state intact. In app.rs, PasteData explicitly bypasses command dispatch, so this completed paste disables subsequent keyboard controls, including Q, Escape and Ctrl-C, until an unrelated ST arrives or an external signal terminates the process. Literal wrapper-looking pasted content must remain opaque without acquiring authority over the outer paste boundary. Recognize the actual outer cl…
      "file": "terminal-showcase/src/protocol.rs",
      "line": 345
    },
    {
      "severity": "must-fix",
      "title": "Preserve overlapping paste delimiters when the payload ends in ESC",
      "body": "Static control-flow evidence: for `\\x1b[200~text\\x1b\\x1b[201~q`, the first of the consecutive ESC bytes is the final payload byte and the second begins the real terminal-supplied closing marker. This branch instead treats both as a quoted ESC pair, emits both as paste data and resets the delimiter match to zero. The remaining `[201~q` therefore stays pasted content; subsequent keyboard commands remain suppressed by app.rs's PasteData dispatch, and idle expiry cannot recover. Ordinary bracketed paste does not introduce doubled-ESC quoting; tmux transport unquoting already occurs in decode_wrapper. Preserve delimiter overlap by retaining the second ESC as a possible marker start rather than consuming it as quoting. Add byte-boundary regressio…
      "file": "terminal-showcase/src/protocol.rs",
      "line": 803
    }
  ],
  "assessed_items": 24,
  "must_fix": 2,
  "should_fix": 0,
  "score": 91.66666666666667,
  "verdict": "FAIL",
  "smells": "Non-blocking maintainability observations: Duplicated Code in the independent PTY child guards, reader-drain setup and deadline loops across integration tests; Primitive Obsession in string-valued exercise outcomes and manually interpreted protocol modifier bits. These were not counted as findings because no additional patch-introduced consumer-visible defect was established. Production module boundaries otherwise separate framing, evidence, presentation, graphics and terminal ownership without coupling the explorer to amux internals.",
  "verification": "Read-only static review of the working-tree diff, every named implementation module, the permanent tests, workspace/package manifests, public documentation, scope, epic plan, story acceptances and task definitions. Assessed 24 distinct areas: (1) standalone/workspace isolation, (2) CLI contracts, (3) CSI framing, (4) OSC/DCS/APC framing, (5) C1 and UTF-8 handling, (6) nested transport wrappers, (7) literal-wrapper paste boundaries, (8) overlapping paste delimiters, (9) legacy mouse opacity, (10) input decoding and command dispatch, (11) exact query correlation, (12) deadlines and untagged CPR single-flight safety, (13) physical geometry and resize invalidation, (14) objective/visual evidence semantics, (15) termios and RAII restoration, (1…
  "limitations": "No builds, tests, lint, formatting, smoke runs or repository modifications were performed, as instructed. The two findings are proven by static state-transition and consumer-dispatch inspection, not by newly executed reproductions. Parent-reported verification results were accepted without rerunning them. Physical glyph/raster appearance, minimum-toolchain compatibility, Linux/SSH behavior and additional emulator combinations remain uncertified by this review. No other reviews, audits, agent histories or sibling messages were consulted."
}

[Some lines truncated to 768 chars]
