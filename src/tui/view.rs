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
    button(frame, geometry.close, "Close window · X", false);
    button(frame, geometry.new, "New terminal · N", true);
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
    button(frame, geometry.close, "Exit · Esc / Q", false);
    button(frame, geometry.new, "New terminal · Enter", true);
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
    status: &str,
    offset: usize,
    focused: bool,
    selection: Option<Selection>,
) -> Geometry {
    let geometry = geometry(frame.area());
    let terminal = snapshot["terminals"]
        .as_array()
        .and_then(|ts| ts.iter().find(|t| text(t, "id") == active));
    let agent = super::agent_view(snapshot, active);
    let empty = active.is_empty()
        && snapshot["terminals"]
            .as_array()
            .is_none_or(|terminals| terminals.is_empty());
    let project = terminal.or(agent).and_then(|t| {
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
    } else if picker && empty {
        "N / Enter new · Esc / Q exit"
    } else if picker {
        "^Q detach · Esc return · Enter open · ^N new · ^O OMP · ^X close"
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
    } else if let Some(agent) = agent {
        frame.render_widget(
            Paragraph::new(format!(
                " ACP · {} · {}",
                text(agent, "name"),
                text(agent, "lifecycle")
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
            "Switch"
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
    geometry
}
