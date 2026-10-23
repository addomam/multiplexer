# e02-round-2-b

{
  "findings": [
    {
      "severity": "must-fix",
      "title": "Preserve paste boundaries when startup and exercise queues overflow",
      "body": "In probe(), the 128-event limit silently drops every subsequent unmatched event, including CSI 200~. The parser nevertheless enters its persistent Paste state. If that queue fills with input events before a paste starts, paste content received during capture_margins(), an objective exercise, or the interactive loop can subsequently be retained without its opening boundary. app::run initializes its separate paste flag to false, so Inputs turns that content into commands: launching --demo integration can execute a pasted C/N/T/W action without consent, and a pasted Q can exit. capture_margins() and receive_cursor_response() have the same boundary-dropping behavior at 512 events. Preserve paste provenance independently of lossy queues, or use a s…
      "file": "terminal-showcase/src/app.rs",
      "line": 61
    },
    {
      "severity": "should-fix",
      "title": "Reuse traditional control-key decoding in the live input iterator",
      "body": "Inputs::next constructs every ASCII scalar as a Traditional key with code equal to the raw byte and modifiers=0, bypassing graphics::traditional(). Consequently an actual Ctrl-A byte is shown by the live inspector as code 1 with no Ctrl modifier rather than code 97 with modifier bit 4; Ctrl-J is likewise reported as code 10 instead of the normalized Enter code used by the public decoder. observe() then records successful decoding despite those incorrect fields. Existing graphics tests exercise the correct decoder, not this production Input path, so they do not cover the discrepancy. Factor the allocation-free single-byte normalization into a shared helper and use it from both paths, retaining Ctrl-C exit behavior and adding live-path assertion…
      "file": "terminal-showcase/src/app.rs",
      "line": 173
    }
  ],
  "assessed_items": 26,
  "must_fix": 1,
  "should_fix": 1,
  "score": 92.3076923076923,
  "verdict": "FAIL",
  "smells": "Duplicated Code is concrete: app::Inputs independently implements traditional-key interpretation already present in graphics::traditional(), causing the reported decoding divergence. Deferred-event buffering also duplicates inconsistent capacity/drop policies across probe(), capture_margins(), receive_cursor_response(), and enqueue(), contributing to the paste-boundary defect. app::run mixes dispatch, graphics scheduling, evidence mutation, rendering, and cleanup in one large function, increasing review and maintenance difficulty; this is not counted as an additional correctness finding. No separate actionable Mysterious Name, Feature Envy, Data Clumps, Primitive Obsession, Message Chains, or Middle Man finding met the evidence threshold.",
  "verification": "Read-only static review completed against the full brief, CONVENTIONS.md, root manifest/lock/documentation changes, all ten scoped source files, all ten behavioral-test files, the feature specification, scope YAML, epic plan, four stories, and their task definitions. Assessed areas: (1) workspace isolation/default package behavior; (2) CLI validation and mode selection; (3) headless fixtures and response-file bounds; (4) capability catalog and JSON schema; (5) objective/visual status independence; (6) endpoint text sanitization and evidence bounds; (7) read-only discovery queries; (8) exact response correlation and deadlines; (9) fragmented/multiple framing; (10) malformed/expired-frame quarantine; (11) C1, UTF-8, and nested-wrapper handli…
  "limitations": "No tests, builds, lint checks, formatters, runtime probes, or repository edits were performed. The two findings are established by static control/data-flow analysis, not newly executed reproductions. Parent-reported successful gates were treated as supplied evidence and were not independently rerun. Native minimum-toolchain support, Linux/SSH behavior, emulator-specific graphics combinations, actual raster/glyph appearance, pixel-coordinate units, and physical restoration remain uncertified by this review. The documented amux ZWJ and origin-CPR endpoint failures were not counted as implementation defects or used to weaken independent expectations. Pending workflow/delivery bookkeeping was intentionally not counted as a product bug."
}

[Some lines truncated to 768 chars]
