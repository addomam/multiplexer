//! Compatibility at the parsed ANSI boundary for the pinned emulator.
//!
//! Alacritty 0.26 lays out Unicode scalars and reports physical CPR rows. Keep
//! its parser, controls, attributes and one/two-cell layout, but compose extended
//! graphemes in the leading cell and report CPR relative to DECOM's top margin.
//!
//! Composition working text is limited to 4 KiB. An oversized cluster starts a
//! new native scalar segment at that boundary: text is retained, but joining
//! across the boundary is not. This bounds additional state and rescans, not
//! upstream Cell::zerowidth storage or terminal output bytes.

use alacritty_terminal::grid::{Cursor, Dimensions, Row};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{TermMode, cell::Cell};
use alacritty_terminal::vte::ansi::{self, Handler};
use unicode_segmentation::GraphemeCursor;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{Flags, Input, Listener, Size, Term, mpsc};

const MAX_GRAPHEME_BYTES: usize = 4096;

pub(super) struct Compatibility {
    top: usize,
    bottom: Option<usize>,
    history_limit: usize,
    first: Option<char>,
    text: String,
    anchor: Point,
    width: usize,
    undo: Option<Undo>,
    pub(super) dirty_rows: [bool; 300],
    pub(super) full_damage: bool,
}

impl Compatibility {
    pub(super) fn new(history_limit: usize) -> Self {
        Self {
            top: 0,
            bottom: None,
            history_limit,
            first: None,
            text: String::new(),
            anchor: Point::default(),
            width: 0,
            undo: None,
            dirty_rows: [false; 300],
            full_damage: false,
        }
    }

    fn boundary(&mut self) {
        self.first = None;
        self.text.clear();
        self.undo = None;
    }

    pub(super) fn resize(&mut self, term: &Term<Listener>, size: Size) {
        if term.columns() != size.cols || term.screen_lines() != size.rows {
            self.boundary();
            self.top = 0;
            self.bottom = None;
        }
    }

    fn dirty(&mut self, line: Line) {
        if let Some(dirty) = usize::try_from(line.0)
            .ok()
            .and_then(|line| self.dirty_rows.get_mut(line))
        {
            *dirty = true;
        }
    }
}

// Only the final grapheme may grow in the next PTY read. Retain the few cells
// its native write can overwrite, not a screen/history clone. Insert mode needs
// the shifted row; a wide glyph wrapping at the bottom needs the evicted row.
// These uncommon cases are the only row allocations in the adapter.
struct Undo {
    cursor: Cursor<Cell>,
    cells: [Option<(Point, Cell)>; 9],
    count: usize,
    inserted: Option<Row<Cell>>,
    scroll: Option<ScrollUndo>,
}

struct ScrollUndo {
    top: Line,
    bottom: Line,
    history: usize,
    oldest: Row<Cell>,
}

impl Undo {
    fn capture(term: &Term<Listener>, state: &Compatibility, width: usize) -> Self {
        let grid = term.grid();
        let point = grid.cursor.point;
        let mut undo = Self {
            cursor: grid.cursor.clone(),
            cells: std::array::from_fn(|_| None),
            count: 0,
            inserted: term
                .mode()
                .contains(TermMode::INSERT)
                .then(|| grid[point.line].clone()),
            scroll: None,
        };
        // Include neighboring halves of overwritten wide characters.
        for col in point.column.0.saturating_sub(1)..=(point.column.0 + 2).min(term.columns() - 1) {
            undo.cell(term, Point::new(point.line, Column(col)));
        }
        if point.column.0 <= 1 && point.line > term.topmost_line() {
            undo.cell(term, Point::new(point.line - 1, term.last_column()));
        }
        if width == 2
            && point.column == term.last_column()
            && term.mode().contains(TermMode::LINE_WRAP)
        {
            let bottom = Line(state.bottom.unwrap_or(term.screen_lines()) as i32);
            if point.line + 1 == bottom {
                let history = grid.history_size();
                let top = if state.top == 0 {
                    Line(-(history as i32))
                } else {
                    Line(state.top as i32)
                };
                undo.scroll = Some(ScrollUndo {
                    top,
                    bottom,
                    history,
                    oldest: grid[top].clone(),
                });
            } else {
                let target = Line((point.line.0 + 1).min(term.screen_lines() as i32 - 1));
                for col in 0..term.columns().min(3) {
                    undo.cell(term, Point::new(target, Column(col)));
                }
            }
        }
        undo
    }

    fn cell(&mut self, term: &Term<Listener>, point: Point) {
        self.cells[self.count] = Some((point, term.grid()[point].clone()));
        self.count += 1;
    }

    fn restore(self, term: &mut Term<Listener>, state: &mut Compatibility) {
        if let Some(scroll) = self.scroll {
            let history_limit = if term.mode().contains(TermMode::ALT_SCREEN) {
                0
            } else {
                state.history_limit
            };
            let grid = term.grid_mut();
            // Reverse the one provisional wrap, including its history rotation.
            let top = if state.top == 0 {
                Line(-(grid.history_size() as i32))
            } else {
                scroll.top
            };
            grid.scroll_down::<ansi::Color>(&(top..scroll.bottom), 1);
            if grid.history_size() > scroll.history {
                grid.update_history(scroll.history);
                grid.update_history(history_limit);
            }
            grid[scroll.top] = scroll.oldest;
            state.full_damage = true;
        }
        let line = self.cursor.point.line;
        if let Some(row) = self.inserted {
            term.grid_mut()[line] = row;
            state.dirty(line);
        }
        for (point, cell) in self.cells.into_iter().flatten() {
            term.grid_mut()[point] = cell;
            state.dirty(point.line);
        }
        term.grid_mut().cursor = self.cursor;
    }
}

pub(super) struct TerminalHandler<'a> {
    term: &'a mut Term<Listener>,
    state: &'a mut Compatibility,
    input: &'a mpsc::SyncSender<Input>,
}

impl<'a> TerminalHandler<'a> {
    pub(super) fn new(
        term: &'a mut Term<Listener>,
        state: &'a mut Compatibility,
        input: &'a mpsc::SyncSender<Input>,
    ) -> Self {
        Self { term, state, input }
    }

    fn wrap_pending(&mut self) {
        if self.term.grid().cursor.input_needs_wrap
            && self.term.mode().contains(TermMode::LINE_WRAP)
        {
            let point = self.term.grid().cursor.point;
            self.term.grid_mut()[point].flags.insert(Flags::WRAPLINE);
            self.state.dirty(point.line);
            self.term.linefeed();
            let cursor = &mut self.term.grid_mut().cursor;
            cursor.point.column = Column(0);
            cursor.input_needs_wrap = false;
        }
    }

    fn anchor(&self, width: usize) -> Option<Point> {
        let cursor = &self.term.grid().cursor;
        let end = cursor.point.column.0 + usize::from(cursor.input_needs_wrap);
        end.checked_sub(width)
            .map(|col| Point::new(cursor.point.line, Column(col)))
    }

    fn wide_refused(&self, width: usize) -> bool {
        width == 2
            && self.term.grid().cursor.point.column == self.term.last_column()
            && !self.term.mode().contains(TermMode::LINE_WRAP)
    }

    fn start(&mut self, c: char) {
        self.state.boundary();
        let Some(width) = c.width() else { return };
        if width == 0 {
            // Preserve native behavior for isolated combining/control-format scalars.
            self.term.input(c);
            self.state.dirty(self.term.grid().cursor.point.line);
            return;
        }
        let width = width.min(2);
        // Ordinary ASCII only records a scalar and point: no string allocation,
        // segmentation, width-string scan or cell snapshots on the hot path.
        let may_change_width = !c.is_ascii() || c.is_ascii_digit() || c == '#' || c == '*';
        if may_change_width {
            self.wrap_pending();
            self.state.undo = Some(Undo::capture(self.term, self.state, width));
        }
        let refused = self.wide_refused(width);
        self.term.input(c);
        let Some(anchor) = self.anchor(width) else {
            self.state.boundary();
            return;
        };
        // Native no-wrap mode refuses a wide glyph which cannot fit.
        if refused {
            self.state.boundary();
            return;
        }
        self.state.first = Some(c);
        self.state.anchor = anchor;
        self.state.width = width;
        if !c.is_ascii() {
            self.state.text.push(c);
        }
    }

    fn extend(&mut self, c: char, width: usize) {
        if width == self.state.width {
            self.term.grid_mut()[self.state.anchor].push_zerowidth(c);
            self.state.dirty(self.state.anchor.line);
            return;
        }
        if let Some(undo) = self.state.undo.take() {
            // Replay using the base's attributes, but preserve SGR/hyperlink
            // metadata changes for the input which follows this grapheme.
            let template = self.term.grid().cursor.template.clone();
            undo.restore(self.term, self.state);
            self.state.undo = Some(Undo::capture(self.term, self.state, width));
            let refused = self.wide_refused(width);
            // Use the native handler for wrapping, insert/overwrite semantics,
            // wide spacers and attributes; the carrier never escapes the grid.
            self.term.input(if width == 2 { '\u{3000}' } else { 'a' });
            self.term.grid_mut().cursor.template = template;
            let Some(anchor) = self.anchor(width) else {
                self.state.boundary();
                return;
            };
            if refused {
                self.state.boundary();
                return;
            }
            let cell = &mut self.term.grid_mut()[anchor];
            cell.c = self.state.first.unwrap();
            for c in self.state.text.chars().skip(1) {
                cell.push_zerowidth(c);
            }
            self.state.anchor = anchor;
            self.state.width = width;
            self.state.dirty(anchor.line);
        } else {
            // ASCII bases other than keycap starters have fixed width.
            self.term.grid_mut()[self.state.anchor].push_zerowidth(c);
            self.state.dirty(self.state.anchor.line);
        }
    }
}

// Handler has default no-op methods. Explicitly forward its complete vte 0.15
// surface, including methods not currently overridden by Term, so adding this
// boundary never drops a native terminal operation. Controls end extension;
// metadata and SGR are forwarded without splitting a printable grapheme.
macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {$(
        fn $name(&mut self, $($arg: $ty),*) {
            self.state.boundary();
            self.term.$name($($arg),*);
        }
    )*};
}
macro_rules! metadata {
    ($($name:ident($($arg:ident: $ty:ty),*);)*) => {$(
        fn $name(&mut self, $($arg: $ty),*) {
            self.term.$name($($arg),*);
        }
    )*};
}

impl Handler for TerminalHandler<'_> {
    fn input(&mut self, c: char) {
        let Some(first) = self.state.first else {
            self.start(c);
            return;
        };
        if first.is_ascii() && self.state.text.is_empty() && c.is_ascii() {
            self.start(c);
            return;
        }
        if self.state.text.is_empty() {
            self.state.text.push(first);
        }
        let boundary = self.state.text.len();
        if boundary + c.len_utf8() > MAX_GRAPHEME_BYTES {
            self.start(c);
            return;
        }
        self.state.text.push(c);
        let mut cursor = GraphemeCursor::new(boundary, self.state.text.len(), true);
        if cursor
            .is_boundary(&self.state.text, 0)
            .expect("complete grapheme context")
        {
            self.start(c);
        } else {
            // Terminal cells support narrow and wide glyphs. Unicode width may
            // count more cells for an unshaped cluster; native Term has the same
            // two-cell ceiling for individual scalars.
            let width = self.state.text.width().clamp(1, 2);
            self.extend(c, width);
        }
    }

    fn device_status(&mut self, arg: usize) {
        if arg == 6 && self.term.mode().contains(TermMode::ORIGIN) {
            let pos = self.term.grid().cursor.point;
            let row = (pos.line.0 - self.state.top as i32).max(0) + 1;
            let _ = self.input.try_send(Input {
                bytes: format!("\x1b[{row};{}R", pos.column.0 + 1).into_bytes(),
                ack: None,
            });
        } else {
            self.term.device_status(arg);
        }
    }

    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        self.state.boundary();
        let end = bottom.unwrap_or(self.term.screen_lines());
        if top < end {
            self.state.top = top.saturating_sub(1).min(self.term.screen_lines());
            self.state.bottom = Some(end.min(self.term.screen_lines()));
        }
        self.term.set_scrolling_region(top, bottom);
    }

    fn reset_state(&mut self) {
        self.state.boundary();
        self.state.top = 0;
        self.state.bottom = None;
        self.term.reset_state();
    }

    fn set_private_mode(&mut self, mode: ansi::PrivateMode) {
        self.state.boundary();
        if mode == ansi::PrivateMode::Named(ansi::NamedPrivateMode::ColumnMode) {
            self.state.top = 0;
            self.state.bottom = None;
        }
        self.term.set_private_mode(mode);
    }

    fn unset_private_mode(&mut self, mode: ansi::PrivateMode) {
        self.state.boundary();
        if mode == ansi::PrivateMode::Named(ansi::NamedPrivateMode::ColumnMode) {
            self.state.top = 0;
            self.state.bottom = None;
        }
        self.term.unset_private_mode(mode);
    }

    metadata! {
        set_title(title: Option<String>);
        set_cursor_style(style: Option<ansi::CursorStyle>);
        set_cursor_shape(shape: ansi::CursorShape);
        bell();
        identify_terminal(intermediate: Option<char>);
        terminal_attribute(attr: ansi::Attr);
        report_mode(mode: ansi::Mode);
        report_private_mode(mode: ansi::PrivateMode);
        set_color(index: usize, color: ansi::Rgb);
        dynamic_color_sequence(prefix: String, index: usize, terminator: &str);
        reset_color(index: usize);
        clipboard_store(clipboard: u8, data: &[u8]);
        clipboard_load(clipboard: u8, terminator: &str);
        push_title();
        pop_title();
        text_area_size_pixels();
        text_area_size_chars();
        set_hyperlink(hyperlink: Option<ansi::Hyperlink>);
        set_mouse_cursor_icon(icon: ansi::cursor_icon::CursorIcon);
        report_keyboard_mode();
        report_modify_other_keys();
    }

    forward! {
        goto(line: i32, col: usize);
        goto_line(line: i32);
        goto_col(col: usize);
        insert_blank(count: usize);
        move_up(rows: usize);
        move_down(rows: usize);
        move_forward(cols: usize);
        move_backward(cols: usize);
        move_down_and_cr(rows: usize);
        move_up_and_cr(rows: usize);
        put_tab(count: u16);
        backspace();
        carriage_return();
        linefeed();
        substitute();
        newline();
        set_horizontal_tabstop();
        scroll_up(rows: usize);
        scroll_down(rows: usize);
        insert_blank_lines(count: usize);
        delete_lines(count: usize);
        erase_chars(count: usize);
        delete_chars(count: usize);
        move_backward_tabs(count: u16);
        move_forward_tabs(count: u16);
        save_cursor_position();
        restore_cursor_position();
        clear_line(mode: ansi::LineClearMode);
        clear_screen(mode: ansi::ClearMode);
        clear_tabs(mode: ansi::TabulationClearMode);
        set_tabs(interval: u16);
        reverse_index();
        set_mode(mode: ansi::Mode);
        unset_mode(mode: ansi::Mode);
        set_keypad_application_mode();
        unset_keypad_application_mode();
        set_active_charset(index: ansi::CharsetIndex);
        configure_charset(index: ansi::CharsetIndex, charset: ansi::StandardCharset);
        decaln();
        push_keyboard_mode(mode: ansi::KeyboardModes);
        pop_keyboard_modes(count: u16);
        set_keyboard_mode(mode: ansi::KeyboardModes, behavior: ansi::KeyboardModesApplyBehavior);
        set_modify_other_keys(mode: ansi::ModifyOtherKeys);
        set_scp(path: ansi::ScpCharPath, mode: ansi::ScpUpdateMode);
    }
}
