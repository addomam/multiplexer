use super::{Row, Selection, clip_label, selection_contains, text};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

const ACCENT: Color = Color::Rgb(103, 213, 224);
const CHROME: Color = Color::Rgb(22, 29, 43);
const MUTED: Color = Color::Rgb(149, 163, 184);
const SELECTED: Color = Color::Rgb(39, 57, 76);

pub(super) const ROW_HEIGHT: u16 = 2;
#[derive(Clone, Copy)]
pub(super) struct Geometry {
    pub header: Rect,
    pub switch: Rect,
    pub body: Rect,
    pub card: Rect,
    pub close: Rect,
    pub new: Rect,
    pub menu_list: Rect,
    pub new_session: Rect,
    pub save_project: Rect,
    pub projects: Rect,
    pub actions: Rect,
}
impl Geometry {
    pub fn hit(rect: Rect, x: u16, y: u16) -> bool {
        rect.contains(Position::new(x, y))
    }
}
// Keep rendering and mouse targets on the same responsive rectangles.
pub(super) fn action_buttons<const N: usize>(
    area: Rect,
    labels: [&str; N],
    reserve: u16,
) -> ([Rect; N], Rect) {
    let mut buttons = [Rect::default(); N];
    let count = labels.iter().filter(|label| !label.is_empty()).count() as u16;
    if count == 0 {
        return (buttons, area);
    }
    let minimum = labels
        .iter()
        .map(|label| label.width() as u16 + 4)
        .max()
        .unwrap_or(0);
    let stacked = u32::from(area.width) < u32::from(minimum) * u32::from(count);
    let rows = if stacked { count } else { 1 };
    let available = area.height.saturating_sub(reserve);
    let height = if available >= rows * 3 {
        3
    } else if available >= rows {
        1
    } else {
        return (buttons, area);
    };
    let content = Rect::new(area.x, area.y, area.width, area.height - rows * height);
    for (slot, (index, _)) in labels
        .iter()
        .enumerate()
        .filter(|(_, label)| !label.is_empty())
        .enumerate()
    {
        buttons[index] = if stacked {
            Rect::new(
                area.x,
                content.bottom() + slot as u16 * height,
                area.width,
                height,
            )
        } else {
            let left = (u32::from(area.width) * slot as u32 / u32::from(count)) as u16;
            let right = (u32::from(area.width) * (slot + 1) as u32 / u32::from(count)) as u16;
            Rect::new(area.x + left, content.bottom(), right - left, height)
        };
    }
    (buttons, content)
}

pub(super) fn help_key(key: crossterm::event::KeyEvent, plain: bool) -> bool {
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    // Crossterm decodes legacy Ctrl+/ (0x1f) as Ctrl-7, not Ctrl-_.
    key.kind == KeyEventKind::Press
        && !key.modifiers.contains(KeyModifiers::ALT)
        && ((plain && matches!(key.code, KeyCode::Char('?') | KeyCode::F(1)))
            || (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('?' | '/' | '_' | '7'))))
}

const SHORTCUTS: &str = "\
SESSION
F12 (configurable)  Open Menu navigation
Configured Ctrl prefix  Alternative Menu entry
Double mode key / double prefix  Send that key deliberately to a native session
Ordinary Esc, j and k belong to the application.
On macOS a function key may require Fn.
No universally unused terminal key exists.

MENU — NAVIGATION
j / k / arrows  Select a session
g / G / Home / End  First / last session
Enter  Open selected session
i / Esc  Return to current session
/  Enter literal search; j/k become text
Esc in search  Leave search; retain filter
n  New terminal or ACP agent…
s  Save selected project
p  Saved projects
a  Session actions
r  Rename session
x  Confirm closing live session; remove exited record
c  Request / manage control
f  Explicit force-control confirmation
v  Send reserved mode key to selected native session
P / m / D  Promote / move / demote native terminal
q  Detach UI; work and ownership keep running
Tab / Shift-Tab  Focus search and action buttons
? / F1  Contextual shortcut help

FORMS AND PROJECTS
Tab / Shift-Tab and mouse  Focus fields/actions
j / k  Navigate lists/selectors, never text fields
Esc  Close help first, then cancel/back
Visible Apply/Save/Launch buttons  Explicit submission
Delete/Backspace  Edit selected text
Advanced  Exact settings or raw project configuration
Saved Forget removes only the bookmark, never files/work.

CONTROL
Control panels retain explicit yield/keep/release/cancel.
Force never interrupts a turn or kills a process.
Detaching never releases control or cancels a claim.
Intercepted input is consumed, never replayed.

COMPATIBILITY ACCELERATORS
Ctrl-Q detach; Ctrl-S save; Ctrl-G projects; Ctrl-T New
Ctrl-N native terminal; Ctrl-O native OMP; Ctrl-A ACP
Ctrl-X close; Ctrl-P promote; Ctrl-R move; Ctrl-D demote
Prefix then Ctrl-L control; prefix then Ctrl-F force
Ctrl+/ or enhanced Ctrl+? help in supported editors
Legacy Ctrl+? is Backspace; it is not stolen.

HELP
j / k / arrows / wheel  Scroll
PageUp / PageDown  Scroll a page
Home / End  First / last page
Esc  Close help without replaying covered input.";

pub(super) struct Help {
    scroll: u16,
    max_scroll: u16,
    page: u16,
    paragraph: Paragraph<'static>,
}
impl Default for Help {
    fn default() -> Self {
        Self::new(SHORTCUTS)
    }
}
impl Help {
    pub(super) fn new(text: impl Into<ratatui::text::Text<'static>>) -> Self {
        Self {
            scroll: 0,
            max_scroll: 0,
            page: 1,
            paragraph: Paragraph::new(text).wrap(Wrap { trim: false }),
        }
    }
    pub(super) fn with_status(status: &str) -> Self {
        Self::with_context_status(SHORTCUTS, status)
    }
    pub(super) fn with_context_status(context: &str, status: &str) -> Self {
        let status: String = status
            .chars()
            .filter(|c| {
                (!c.is_control() || *c == '\n' || *c == '\t')
                    && !matches!(*c, '\u{061c}' | '\u{200e}' | '\u{200f}'
                        | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
            .collect();
        Self::new(format!("{context}\n\nLAST ACTION / ERROR\n{status}"))
    }
    pub(super) fn event(&mut self, event: &crossterm::event::Event) -> bool {
        use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc => return true,
                KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(self.page),
                KeyCode::PageDown => self.scroll = self.scroll.saturating_add(self.page),
                KeyCode::Home => self.scroll = 0,
                KeyCode::End => self.scroll = self.max_scroll,
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
                MouseEventKind::ScrollDown => self.scroll = self.scroll.saturating_add(3),
                _ => {}
            },
            _ => {}
        }
        self.scroll = self.scroll.min(self.max_scroll);
        false
    }
    pub(super) fn draw(&mut self, frame: &mut Frame<'_>, screen: Rect) {
        let width = screen.width.min(72);
        let height = screen.height.min(30);
        let area = Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        );
        let hint = if width >= 28 {
            " Esc close · arrows scroll "
        } else if width >= 13 {
            " Esc close "
        } else {
            ""
        };
        let block = panel().title(" Shortcuts ").title_bottom(hint);
        let inner = block.inner(area);
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(block, area);
        if inner.is_empty() {
            return;
        }
        self.max_scroll = self
            .paragraph
            .line_count(inner.width)
            .saturating_sub(usize::from(inner.height))
            .min(usize::from(u16::MAX)) as u16;
        self.page = inner.height.saturating_sub(1).max(1);
        self.scroll = self.scroll.min(self.max_scroll);
        self.paragraph = std::mem::take(&mut self.paragraph).scroll((self.scroll, 0));
        frame.render_widget(&self.paragraph, inner);
    }
}

pub(super) fn menu_search(
    frame: &mut Frame<'_>,
    area: Rect,
    query: &str,
    color: Color,
    searching: bool,
) {
    let label = if area.width >= 20 {
        if searching { " SEARCH  " } else { " NAV  " }
    } else {
        ""
    };
    let placeholder = if searching {
        "name / project / directory / branch"
    } else {
        "Sessions"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, Style::default().fg(color)),
            Span::raw(clip_label(
                if query.is_empty() { placeholder } else { query },
                area.width.saturating_sub(label.width() as u16),
            )),
        ]))
        .style(Style::default().fg(color)),
        area,
    );
}

pub(super) fn geometry(area: Rect) -> Geometry {
    let header_height = area.height.min(super::HEADER);
    let switch_width = area.width.min(12);
    let header = Rect::new(area.x, area.y, area.width - switch_width, header_height);
    let switch = Rect::new(header.right(), area.y, switch_width, header_height);
    let body = Rect::new(
        area.x,
        area.y + header_height,
        area.width,
        area.height - header_height,
    );
    let ([new_session, save_project, projects, actions], menu_list) = action_buttons(
        body,
        ["New…", "Save project", "Projects", "Actions"],
        ROW_HEIGHT,
    );
    let (card, close, new) = window_card(body);
    Geometry {
        header,
        switch,
        body,
        card,
        close,
        new,
        menu_list,
        new_session,
        save_project,
        projects,
        actions,
    }
}
fn window_card(body: Rect) -> (Rect, Rect, Rect) {
    let width = body.width.saturating_sub(2).min(60);
    let stacked = width < 46;
    let height = body.height.min(if stacked { 14 } else { 11 });
    let card = Rect::new(
        body.x + (body.width - width) / 2,
        body.y + (body.height - height) / 2,
        width,
        height,
    );
    let button_width = card.width.saturating_sub(4).min(22);
    let button_height = card.height.saturating_sub(6).min(3);
    let close = Rect::new(
        card.x + card.width.saturating_sub(button_width) / 2,
        card.y + card.height.saturating_sub(if stacked { 7 } else { 4 }),
        button_width,
        button_height,
    );
    let (close, new) = if stacked {
        (
            close,
            Rect::new(close.x, close.y + close.height, button_width, button_height),
        )
    } else {
        (
            Rect::new(card.x + 3, close.y, button_width, button_height),
            Rect::new(
                card.right().saturating_sub(button_width + 3),
                close.y,
                button_width,
                button_height,
            ),
        )
    };
    (card, close, new)
}
pub(super) fn list_start(selected: usize, height: u16) -> usize {
    selected.saturating_sub(usize::from(height / ROW_HEIGHT).saturating_sub(1))
}
fn panel() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .style(Style::default().bg(CHROME).fg(Color::White))
}
pub(super) fn button(frame: &mut Frame<'_>, area: Rect, label: &str, primary: bool) {
    let style = if primary {
        Style::default().fg(ACCENT).bg(SELECTED)
    } else {
        Style::default().fg(MUTED).bg(CHROME)
    };
    frame.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style.add_modifier(Modifier::BOLD))
            .block(if area.height >= 3 {
                panel().border_style(style)
            } else {
                Block::default()
            }),
        area,
    );
}
fn rgb(value: &Value) -> Color {
    if let Some(rgb) = value.as_array().filter(|rgb| rgb.len() == 3) {
        Color::Rgb(
            rgb[0].as_u64().unwrap_or(0) as u8,
            rgb[1].as_u64().unwrap_or(0) as u8,
            rgb[2].as_u64().unwrap_or(0) as u8,
        )
    } else {
        Color::Reset
    }
}
#[derive(Default)]
pub(super) struct NativeSurface {
    buffer: Buffer,
}
impl NativeSurface {
    pub(super) fn update(&mut self, update: &Value) {
        let area = Rect::new(
            0,
            0,
            update["cols"].as_u64().unwrap_or(0).min(500) as u16,
            update["rows"].as_u64().unwrap_or(0).min(300) as u16,
        );
        if update["delta"] != true {
            self.buffer.resize(area);
            self.buffer.reset();
        }
        let Some(lines) = update["cells"].as_array() else {
            return;
        };
        for (index, line) in lines.iter().enumerate() {
            let row = if update["delta"] == true {
                update["row_indices"][index].as_u64().unwrap_or(0) as usize
            } else {
                index
            };
            if row >= usize::from(area.height) {
                continue;
            }
            let width = usize::from(area.width);
            let target = &mut self.buffer.content[row * width..(row + 1) * width];
            for cell in target.iter_mut() {
                cell.reset();
            }
            let Some(cells) = line.as_array() else {
                continue;
            };
            for (col, (cell, source)) in target.iter_mut().zip(cells).enumerate() {
                if source["wide_spacer"] == true {
                    continue;
                }
                let symbol = source["text"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(" ");
                if symbol.width() > width - col {
                    continue;
                }
                let mut modifiers = Modifier::empty();
                for (field, modifier) in [
                    ("bold", Modifier::BOLD),
                    ("italic", Modifier::ITALIC),
                    ("underline", Modifier::UNDERLINED),
                    ("inverse", Modifier::REVERSED),
                    ("dim", Modifier::DIM),
                    ("strikeout", Modifier::CROSSED_OUT),
                ] {
                    if source[field] == true {
                        modifiers |= modifier;
                    }
                }
                cell.set_symbol(symbol).set_style(
                    Style::default()
                        .fg(rgb(&source["fg"]))
                        .bg(rgb(&source["bg"]))
                        .add_modifier(modifiers),
                );
            }
        }
    }
}
fn native_surface(
    frame: &mut Frame<'_>,
    area: Rect,
    surface: &NativeSurface,
    screen: &Value,
    offset: usize,
    focused: bool,
    selection: Option<Selection>,
) {
    let width = area.width.min(surface.buffer.area.width);
    let height = area.height.min(surface.buffer.area.height);
    let target = frame.buffer_mut();
    for row in 0..height {
        let source_start = usize::from(row) * usize::from(surface.buffer.area.width);
        let target_start = target.index_of(area.x, area.y + row);
        let cells = &mut target.content[target_start..target_start + usize::from(width)];
        cells.clone_from_slice(
            &surface.buffer.content[source_start..source_start + usize::from(width)],
        );
        // A client can shrink before a shared PTY's next full snapshot arrives.
        if width < surface.buffer.area.width
            && cells.last().is_some_and(|cell| cell.symbol().width() > 1)
        {
            cells.last_mut().expect("nonempty clipped row").reset();
        }
        if selection.is_some() {
            for (col, cell) in cells.iter_mut().enumerate() {
                if selection_contains(selection, (col as u16, row)) {
                    cell.set_style(Style::default().fg(Color::White).bg(SELECTED));
                }
            }
        }
    }
    if offset == 0 && focused && screen["cursor"]["visible"].as_bool().unwrap_or(true) {
        let row = screen["cursor"]["row"].as_u64().unwrap_or(0);
        let col = screen["cursor"]["col"].as_u64().unwrap_or(0);
        if row < u64::from(area.height) && col < u64::from(area.width) {
            frame.set_cursor_position((area.x + col as u16, area.y + row as u16));
        }
    }
}
fn exited(frame: &mut Frame<'_>, geometry: Geometry, terminal: &Value) {
    let title = clip_label(
        text(terminal, "name"),
        geometry.card.width.saturating_sub(4),
    );
    frame.render_widget(
        panel()
            .title(format!(" {title} "))
            .border_style(Style::default().fg(ACCENT)),
        geometry.card,
    );
    let code = terminal["exit_status"]
        .as_u64()
        .map(|code| format!("Exit code: {code}"))
        .unwrap_or_else(|| "Waiting for exit status…".into());
    let content = vec![
        Line::styled(
            "Process exited",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::styled(code, Style::default().fg(ACCENT)),
        Line::styled(
            "Close this window or start another terminal.",
            Style::default().fg(MUTED),
        ),
    ];
    frame.render_widget(
        Paragraph::new(content)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        Rect::new(
            geometry.card.x + 1,
            geometry.card.y + 1,
            geometry.card.width.saturating_sub(2),
            geometry.card.height.saturating_sub(5),
        ),
    );
    button(frame, geometry.close, "Close window", false);
    button(frame, geometry.new, "New terminal", true);
}
fn empty_windows(frame: &mut Frame<'_>, geometry: Geometry) {
    frame.render_widget(
        panel()
            .title(" No windows ")
            .border_style(Style::default().fg(ACCENT)),
        geometry.card,
    );
    frame.render_widget(
        Paragraph::new("Start a terminal or exit to your host shell.\n\nExiting this view leaves the daemon running.")
            .alignment(Alignment::Center)
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: true }),
        Rect::new(
            geometry.card.x + 1,
            geometry.card.y + 1,
            geometry.card.width.saturating_sub(2),
            geometry.card.height.saturating_sub(5),
        ),
    );
    button(frame, geometry.close, "Exit", false);
    button(frame, geometry.new, "New terminal", true);
}
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    frame: &mut Frame<'_>,
    screen: &Value,
    surface: &NativeSurface,
    snapshot: &Value,
    active: &str,
    picker: bool,
    rows: &[Row],
    selected: usize,
    query: &str,
    navigation: &super::menu::Navigation,
    status: &str,
    offset: usize,
    focused: bool,
    selection: Option<Selection>,
) -> Geometry {
    let mut geometry = geometry(frame.area());
    let terminal = snapshot["terminals"]
        .as_array()
        .and_then(|ts| ts.iter().find(|t| text(t, "id") == active));
    let agent = super::agent_view(snapshot, active);
    let empty = active.is_empty() && !super::has_views(snapshot);
    if picker && empty {
        (geometry.card, geometry.close, geometry.new) = window_card(geometry.menu_list);
    }
    let project = terminal.or(agent).and_then(|t| {
        snapshot["projects"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| text(p, "id") == text(t, "project_id")))
    });
    let title = if picker {
        " MENU ".to_owned()
    } else {
        format!(
            " amux · {} ",
            project.map(|p| text(p, "name")).unwrap_or("project")
        )
    };
    let failure = snapshot["diagnostics"]
        .as_array()
        .and_then(|ds| {
            ds.iter()
                .rev()
                .find(|d| d["diagnostic"]["ok"] == false || d["message"].is_string())
        })
        .map(|d| {
            format!(
                "{}: {}",
                text(d, "kind"),
                d["diagnostic"]["error"]
                    .as_str()
                    .or_else(|| d["message"].as_str())
                    .unwrap_or("hook failed; amux snapshot shows output")
            )
        })
        .unwrap_or_default();
    let hint = if !status.is_empty() {
        status
    } else if picker {
        if geometry.header.width >= 10 {
            "? help"
        } else {
            ""
        }
    } else if !failure.is_empty() {
        &failure
    } else if offset > 0 {
        "scrollback · type to return to live surface"
    } else {
        ""
    };
    let mut block = panel()
        .title(clip_label(&title, geometry.header.width.saturating_sub(2)))
        .title_bottom(clip_label(hint, geometry.header.width.saturating_sub(4)));
    if !status.is_empty() || !failure.is_empty() {
        block = block.border_style(Style::default().fg(Color::Yellow));
    }
    let inner = block.inner(geometry.header);
    frame.render_widget(block, geometry.header);
    if picker && empty {
        frame.render_widget(
            Paragraph::new(" Start a terminal").style(Style::default().fg(ACCENT)),
            inner,
        );
    } else if picker {
        menu_search(frame, inner, query, ACCENT, navigation.searching());
    } else if let Some(terminal) = terminal {
        let name = clip_label(text(terminal, "name"), inner.width);
        let activity = terminal["integration"]["agent"]["activity"]
            .as_str()
            .filter(|s| *s != "unknown")
            .unwrap_or("");
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {name} "),
                    Style::default()
                        .fg(ACCENT)
                        .bg(SELECTED)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    clip_label(
                        &format!(
                            "  {activity}  {}",
                            super::clip_path(
                                terminal["workspace"]["cwd"].as_str().unwrap_or(""),
                                inner
                                    .width
                                    .saturating_sub((name.width() + activity.width() + 5) as u16)
                            )
                        ),
                        inner.width,
                    ),
                    Style::default().fg(MUTED),
                ),
            ])),
            inner,
        );
    } else if let Some(agent) = agent {
        frame.render_widget(
            Paragraph::new(format!(
                " ACP · {} · {}",
                text(agent, "name"),
                super::agent_status(agent)
            ))
            .style(Style::default().fg(ACCENT)),
            inner,
        );
    }
    button(
        frame,
        geometry.switch,
        if picker && empty {
            "Exit"
        } else if picker {
            "Return"
        } else {
            "Menu"
        },
        true,
    );
    if picker && empty {
        empty_windows(frame, geometry);
    } else if picker {
        let items = rows.iter().map(|row| {
            let (summary, directory) = row.label.split_once('\n').unwrap_or((&row.label, ""));
            let indent = "  ".repeat(row.depth.min(20));
            ListItem::new(vec![
                Line::from(vec![
                    Span::raw(indent),
                    Span::raw(clip_label(summary, geometry.body.width.saturating_sub(4))),
                ]),
                Line::styled(
                    super::clip_path(directory, geometry.body.width.saturating_sub(4)),
                    Style::default().fg(MUTED),
                ),
            ])
        });
        let mut state = ListState::default()
            .with_selected(if rows.is_empty() {
                None
            } else {
                Some(selected)
            })
            .with_offset(list_start(selected, geometry.menu_list.height));
        let list = List::new(items).highlight_symbol("› ").highlight_style(
            Style::default()
                .fg(ACCENT)
                .bg(SELECTED)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(list, geometry.menu_list, &mut state);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new("No matching windows.").style(Style::default().fg(MUTED)),
                geometry.menu_list,
            );
        }
    } else if let Some(terminal) = terminal.filter(|t| t["alive"] == false) {
        exited(frame, geometry, terminal);
    } else if agent.is_none() {
        native_surface(
            frame,
            geometry.body,
            surface,
            screen,
            offset,
            focused,
            selection,
        );
    }
    if picker {
        button(
            frame,
            geometry.new_session,
            "New…",
            navigation.button() == Some(0),
        );
        button(
            frame,
            geometry.save_project,
            "Save project",
            navigation.button() == Some(1),
        );
        button(
            frame,
            geometry.projects,
            "Projects",
            navigation.button() == Some(2),
        );
        button(
            frame,
            geometry.actions,
            "Actions",
            navigation.button() == Some(3),
        );
    }
    geometry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn responsive_actions_keep_full_labels_and_disjoint_hit_targets() {
        for (width, height) in [(110, 28), (32, 42), (32, 12), (80, 6)] {
            let area = Rect::new(0, 3, width, height - 3);
            let (buttons, content) = action_buttons(
                area,
                ["New session", "Save project", "Projects"],
                ROW_HEIGHT,
            );
            assert!(content.height >= ROW_HEIGHT);
            for (index, rect) in buttons.iter().enumerate() {
                assert!(!rect.is_empty());
                assert!(rect.width >= 12);
                assert!(rect.x >= area.x && rect.y >= content.bottom());
                assert!(rect.right() <= area.right() && rect.bottom() <= area.bottom());
                for other in &buttons[index + 1..] {
                    assert!(rect.intersection(*other).is_empty());
                }
            }
            if width == 32 {
                assert!(buttons[0].bottom() <= buttons[1].y && buttons[1].bottom() <= buttons[2].y);
            } else {
                assert!(buttons.iter().all(|button| button.y == buttons[0].y));
            }
        }
        let (buttons, _) = action_buttons(Rect::new(0, 0, 32, 12), ["Save", "", "", "Cancel"], 6);
        assert!(buttons[1].is_empty() && buttons[2].is_empty());
        assert!(!buttons[0].is_empty() && !buttons[3].is_empty());
    }

    #[test]
    fn shortcut_help_does_not_steal_backspace_editor_text_or_releases() {
        assert!(help_key(
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            true
        ));
        assert!(help_key(
            KeyEvent::new(KeyCode::Char('_'), KeyModifiers::CONTROL),
            false
        ));
        assert!(help_key(
            KeyEvent::new(KeyCode::Char('7'), KeyModifiers::CONTROL),
            false
        ));
        assert!(help_key(
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::CONTROL),
            false
        ));
        assert!(!help_key(
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            true
        ));
        assert!(!help_key(
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            false
        ));
        assert!(!help_key(
            KeyEvent::new_with_kind(
                KeyCode::Char('?'),
                KeyModifiers::CONTROL,
                KeyEventKind::Release
            ),
            true
        ));
    }
}
