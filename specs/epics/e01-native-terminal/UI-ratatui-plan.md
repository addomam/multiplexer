# Ratatui presentation cutover

## Requirements and decisions
- Use Ratatui for rendering/diffing; retain Crossterm input and daemon-owned alacritty terminal semantics.
- Three-row rounded title/tab bar: project/current window on the left, clickable Switch on the right. Native terminal occupies every remaining row and column, no footer or terminal border.
- Switch/prefix replaces the terminal with a full-screen hierarchical searchable window list; Enter/click opens directly, Escape/Return restores the selected terminal. Existing launch/tree shortcuts remain available in the list header.
- Interpret user “terminal exists” as “terminal exits”: show actual exit code (or pending status, never fabricated zero), Close window removes its exited record, New terminal opens from captured context and leaves old record available. X/N/Enter keyboard alternatives work only on exited surfaces.
- A shared geometry function supplies rendered/hit-test rectangles. Picker never resizes background PTYs. Native mouse coordinates continue to subtract three header rows. Native colors, modifiers, wide/combining cells, cursor, paste and alternate screens remain untouched by UI chrome.

## Prior Art
Adopt Ratatui 0.30.0 with only crossterm_0_29 backend feature (existing Crossterm0.29). Official Frame/Cell API docs: https://docs.rs/ratatui/0.30.0/ratatui/struct.Frame.html and https://docs.rs/ratatui/0.30.0/ratatui/buffer/struct.Cell.html. Cargo metadata reports MSRV1.86; raise declared minimum accordingly, without claiming minimum-toolchain verification. Context7 route is not mounted; use official versioned documentation, not an assumed API. No cached Ratatui sources initially; cargo info fetched documented version. Native emulator remains alacritty_terminal, not a Ratatui terminal parser/widget.

## Increments / verification
1. Replace manual full-screen ANSI renderer with Ratatui buffer/widget projection; preserve current event loop and PTY sizing.
2. Wire exact Switch/list/exit-card hitboxes and keyboard actions; refresh selection/screen when opening/removing windows.
3. Build/typecheck/lint, existing suite; actual native TUI with mouse switch/row click, input and Unicode/colors, resize while switching, natural exit codes and Close/New controls. Update README/architecture/changelog/evidence. Existing whole-MVP independent review cap is not reset or claimed passed by this UI request.