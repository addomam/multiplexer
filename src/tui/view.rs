use super::{Row, Selection, clip_label, selection_contains, text};
use ratatui::{
    Frame,
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
}
impl Geometry {
    pub fn hit(rect: Rect, x: u16, y: u16) -> bool {
        rect.contains(Position::new(x, y))
    }
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
    Geometry {
        header,
        switch,
        body,
        card,
        close,
        new,
    }
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
fn button(frame: &mut Frame<'_>, area: Rect, label: &str, primary: bool) {
    let style = if primary {
        Style::default().fg(ACCENT).bg(SELECTED)
    } else {
        Style::default().fg(MUTED).bg(CHROME)
    };
    frame.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style.add_modifier(Modifier::BOLD))
            .block(panel().border_style(style)),
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
fn native_surface(
    frame: &mut Frame<'_>,
    area: Rect,
    screen: &Value,
    offset: usize,
    focused: bool,
    selection: Option<Selection>,
) {
    if let Some(lines) = screen["cells"].as_array() {
        for (row, line) in lines.iter().take(area.height as usize).enumerate() {
            if let Some(cells) = line.as_array() {
                for (col, source) in cells.iter().take(area.width as usize).enumerate() {
                    if source["wide_spacer"] == true {
                        continue;
                    }
                    let symbol = source["text"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(" ");
                    // Never let a clipped wide glyph wrap over the next native row.
                    if symbol.width() > area.width as usize - col {
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
                    let mut style = Style::default()
                        .fg(rgb(&source["fg"]))
                        .bg(rgb(&source["bg"]))
                        .add_modifier(modifiers);
                    if selection_contains(selection, (col as u16, row as u16)) {
                        style = style.fg(Color::White).bg(SELECTED);
                    }
                    frame.buffer_mut()[(area.x + col as u16, area.y + row as u16)]
                        .set_symbol(symbol)
                        .set_style(style);
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
    button(frame, geometry.close, "Close window · X", false);
    button(frame, geometry.new, "New terminal · N", true);
}
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    frame: &mut Frame<'_>,
    screen: &Value,
    snapshot: &Value,
    active: &str,
    picker: bool,
    rows: &[Row],
    selected: usize,
    query: &str,
    status: &str,
    offset: usize,
    focused: bool,
    selection: Option<Selection>,
) -> Geometry {
    let geometry = geometry(frame.area());
    let terminal = snapshot["terminals"]
        .as_array()
        .and_then(|ts| ts.iter().find(|t| text(t, "id") == active));
    let project = terminal.and_then(|t| {
        snapshot["projects"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| text(p, "id") == text(t, "project_id")))
    });
    let title = if picker {
        " WINDOWS ".to_owned()
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
        "Enter open · ^N new · ^O OMP · ^X close · Esc return"
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
    if picker {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Search  ", Style::default().fg(ACCENT)),
                Span::raw(clip_label(
                    if query.is_empty() {
                        "name / project / directory / branch"
                    } else {
                        query
                    },
                    inner.width,
                )),
            ])),
            inner,
        );
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
                            terminal["workspace"]["cwd"].as_str().unwrap_or("")
                        ),
                        inner.width,
                    ),
                    Style::default().fg(MUTED),
                ),
            ])),
            inner,
        );
    }
    button(
        frame,
        geometry.switch,
        if picker { "Return" } else { "Switch" },
        true,
    );
    if picker {
        let items = rows.iter().map(|row| {
            let (summary, directory) = row.label.split_once('\n').unwrap_or((&row.label, ""));
            let indent = "  ".repeat(row.depth.min(20));
            ListItem::new(vec![
                Line::from(vec![
                    Span::raw(indent),
                    Span::raw(clip_label(summary, geometry.body.width.saturating_sub(4))),
                ]),
                Line::styled(
                    clip_label(
                        &format!("  {directory}"),
                        geometry.body.width.saturating_sub(4),
                    ),
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
            .with_offset(list_start(selected, geometry.body.height));
        let list = List::new(items).highlight_symbol("› ").highlight_style(
            Style::default()
                .fg(ACCENT)
                .bg(SELECTED)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(list, geometry.body, &mut state);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new("No matching windows. Ctrl-N starts a terminal.")
                    .style(Style::default().fg(MUTED)),
                geometry.body,
            );
        }
    } else if let Some(terminal) = terminal.filter(|t| t["alive"] == false) {
        exited(frame, geometry, terminal);
    } else {
        native_surface(frame, geometry.body, screen, offset, focused, selection);
    }
    geometry
}
