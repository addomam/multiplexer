# e02-round-1-a

{
  "findings": [
    {
      "title": "Require explicit consent before selecting Sixel output",
      "body": "When DA1 advertises attribute 4 and Kitty is not advertised, `select_graphics` selects Sixel automatically. The dashboard subsequently reaches the graphics demo through its normal twelve-second cycling and calls `images.frame(Protocol::Sixel, ...)` without any G action. This bypasses the documented requirement that legacy image experiments require explicit G consent and emits raster operations whose storage cannot be reclaimed by the application's addressable cleanup. Leave legacy protocols unselected until the user opts in, even when DA1 advertises them.",
      "priority": 2,
      "confidence": 0.99,
      "file_path": "/Users/addomam/projects/multiplexer/terminal-showcase/src/app.rs",
      "line_start": 171,
      "line_end": 176
    },
    {
      "title": "Ignore lock-state bits when dispatching Kitty hotkeys",
      "body": "Kitty key events carry Caps Lock (64) and Num Lock (128) in the modifier bitset, and the decoder correctly retains those bits. With either lock enabled, this `modifiers > 1` check rejects ordinary Q, Space, Tab and every consent/navigation key; even Ctrl-C is rejected because its modifiers are no longer exactly 4. Since raw mode also disables terminal-generated SIGINT, users of such endpoints cannot exit through the documented keys until they toggle their lock state. Mask the lock-state bits before deciding whether a key has actionable Shift/Alt/Ctrl modifiers.",
      "priority": 2,
      "confidence": 0.99,
      "file_path": "/Users/addomam/projects/multiplexer/terminal-showcase/src/app.rs",
      "line_start": 120,
      "line_end": 126
    },
    {
      "title": "Gate mouse tracking on a reporting format the parser can decode",
      "body": "`enable_input` enables mouse tracking even when discovery explicitly reports mode 1006 as unrecognized. An endpoint supporting legacy mouse tracking but not SGR then sends `ESC [ M` followed by three coordinate/button bytes. The parser finishes the CSI at M, `decode_input` rejects its empty mouse parameters, and `enqueue` splits the remaining printable bytes into ordinary hotkeys. For example, a left click produces a Space byte (pause), and a coordinate byte C while the integration demo is active can issue the clipboard mutation without the user pressing C. Only enable tracking when a supported, decodable reporting format is established, or frame/decode the legacy three-byte mouse payload rather than sending it to command dispatch.",
      "priority": 1,
      "confidence": 0.99,
      "file_path": "/Users/addomam/projects/multiplexer/terminal-showcase/src/terminal.rs",
      "line_start": 132,
      "line_end": 135
    },
    {
      "title": "Preserve mouse modes displaced by the requested modes",
      "body": "The preserved mode set omits 9/1001 and 1005/1015, although the [xterm protocol](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Mouse-Tracking) defines mutually exclusive mouse-tracking and encoding groups. Enabling 1003 or 1006 in `Terminal::enable_input` therefore also clears a previously active omitted mode, such as UTF-8 mouse encoding 1005. Restoration only replays `changed` modes from the queried map, so it cannot re-enable that original encoding/tracking mode and leaves the surrounding terminal application with altered mouse state. Capture and restore the displaced members of these groups as well as the explicitly emitted mode numbers.",
      "priority": 2,
      "confidence": 0.98,
      "file_path": "/Users/addomam/projects/multiplexer/terminal-showcase/src/capability.rs",
      "line_start": 17,
      "line_end": 19
    },
    {
      "title": "Use the physical viewport width for autowrap expectations",
      "body": "On a TTY wider than 500 columns, `size` substitutes 500 for the real width, and `exercises::run` uses that value to generate its autowrap transaction. A conforming 600-column endpoint receives only 500 A characters plus B, correctly reports row 9, column 502, and is classified Failed because the explorer expects row 10, column 2; `--probe --check` consequently exits nonzero for a working endpoint. Keep bounded rendering dimensions separate from physical cursor-test geometry, or skip this objective exercise when the physical width cannot be represented within the rendering bound.",
      "priority": 2,
      "confidence": 0.99,
      "file_path": "/Users/addomam/projects/multiplexer/terminal-showcase/src/terminal.rs",
      "line_start": 92,
      "line_end": 95
    },
    {
      "title": "Preserve hotkeys in mixed UTF-8 input batches",
      "body": "Ordinary input is split into individual dispatchable bytes only when the entire parser event is ASCII. If a read contains a Unicode character followed by q, such as `界q`, the parser emits one non-ASCII `Event::Input`, the decoder returns a multi-character Text event, and `command` drops q because `text.len() != 1`. More seriously, `界` followed by the traditional Ctrl-C byte is rejected altogether by `traditional` because the batch contains a control character; raw mode means no SIGINT rescues that shortcut. Split ordinary input at valid UTF-8 boundaries while preserving individual ASCII/control-key events instead of making dispatch depend on how stdin happens to batch bytes.",
      "priority": 2,
      "confidence": 0.99,
      "file_path": "/Users/addomam/projects/multiplexer/terminal-showcase/src/app.rs",
      "line_start": 34,
      "line_end": 42
    }
  ],
  "overall_correctness": "incorrect",
  "explanation": "FAIL — quality score 70% (20 assessed items, 1 must-fix and 5 should-fix): legacy mouse payloads can invoke consent-bound actions, and additional defects affect Sixel consent, lock-modified and mixed-UTF-8 hotkeys, restoration of displaced mouse modes, and oversized-viewport compatibility evidence. Static review covered all standalone implementation modules, test sources, Cargo boundaries, and changed documentation/specifications, including producer-to-consumer dispatch and endpoint-to-ANSI data flow; no amux implementation changes or intentionally exposed endpoint failures were flagged. No tests, builds, linters, or live terminal experiments were executed for this review; physical rendering, platform-specific cleanup, and runtime verificat…
  "confidence": 0.98
}

[Some lines truncated to 768 chars]
