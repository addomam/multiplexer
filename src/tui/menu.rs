//! Menu navigation is distinct from literal text entry. Dialogs capture one target;
//! destructive confirmation never rebinds to a later selection or ownership grant.
use super::{agent_target, agent_view, clip_label, text, view};
use anyhow::{Context, Result, ensure};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use serde_json::{Value, json};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Focus {
    #[default]
    List,
    Search,
    Button(usize),
}
#[derive(Default)]
pub(super) struct Navigation {
    pub focus: Focus,
}
impl Navigation {
    pub fn searching(&self) -> bool {
        self.focus == Focus::Search
    }
    pub fn button(&self) -> Option<usize> {
        if let Focus::Button(index) = self.focus {
            Some(index)
        } else {
            None
        }
    }
    pub fn next(&mut self, reverse: bool) {
        let index = match self.focus {
            Focus::List => 0,
            Focus::Search => 1,
            Focus::Button(i) => i + 2,
        };
        let index = (index + if reverse { 5 } else { 1 }) % 6;
        self.focus = match index {
            0 => Focus::List,
            1 => Focus::Search,
            index => Focus::Button(index - 2),
        };
    }
}

pub(super) fn mode_key(config: &str) -> Option<KeyEvent> {
    let number = config
        .strip_prefix('F')
        .or_else(|| config.strip_prefix('f'))?
        .parse::<u8>()
        .ok()?;
    (1..=24)
        .contains(&number)
        .then(|| KeyEvent::new(KeyCode::F(number), KeyModifiers::NONE))
}

#[derive(Clone, Copy)]
pub(super) enum SessionAction {
    Rename,
    Close,
    Control,
    Force,
    SendKey,
    Promote,
    Reparent,
    Demote,
}
const SESSION_ACTIONS: [(SessionAction, &str); 8] = [
    (SessionAction::Rename, "Rename session"),
    (SessionAction::Close, "Close session…"),
    (SessionAction::Control, "Request / manage control"),
    (
        SessionAction::Force,
        "Force control… (never interrupts work)",
    ),
    (SessionAction::SendKey, "Send reserved mode key"),
    (SessionAction::Promote, "Promote native terminal"),
    (SessionAction::Reparent, "Move under another terminal"),
    (SessionAction::Demote, "Demote under another terminal"),
];
pub(super) enum Action {
    None,
    Cancel,
    Detach,
    New {
        source: String,
        agent: bool,
        independent: bool,
    },
    Session {
        source: String,
        action: SessionAction,
    },
    Submit(Value),
}
enum Kind {
    Actions,
    New {
        agent: bool,
        independent: bool,
    },
    Close {
        request: Value,
    },
    Rename {
        value: String,
        limit: usize,
        replace: bool,
        rejected: bool,
    },
}
pub(super) struct Dialog {
    source: String,
    name: String,
    project: String,
    kind: Kind,
    focus: usize,
    fields: [Rect; 8],
    buttons: [Rect; 3],
    error: String,
    help: Option<view::Help>,
    drawn: bool,
    confirmable: bool,
}
const HELP: &str = "SESSION ACTIONS\n\nTab / Shift-Tab: move focus. Arrows or j/k navigate choices and actions; text fields keep ordinary letters.\n\nNew: Left/Right or Space changes Terminal/ACP and independent/current-project placement. Enter creates the chosen kind; t/a select its kind.\n\nRename: initial name is selected; typing replaces it. Backspace edits, Delete clears. Enter saves deliberately.\n\nClose: y confirms only after the complete warning is painted and queued input is drained. n/Esc keeps working; d detaches this UI without stopping work or releasing control.\n\n? / F1 opens this help outside text entry. Esc closes help first, then cancels the action. Nothing intercepted is replayed.";
impl Dialog {
    fn base(snapshot: &Value, source: &str, kind: Kind) -> Self {
        let session = agent_view(snapshot, source).or_else(|| {
            snapshot["terminals"]
                .as_array()?
                .iter()
                .find(|t| t["id"] == source)
        });
        let project = session.and_then(|session| {
            snapshot["projects"]
                .as_array()?
                .iter()
                .find(|p| p["id"] == session["project_id"])
        });
        Self {
            source: source.into(),
            name: session.map(|s| text(s, "name")).unwrap_or("session").into(),
            project: project
                .map(|p| text(p, "name"))
                .unwrap_or("current project")
                .into(),
            kind,
            focus: 0,
            fields: [Rect::default(); 8],
            buttons: [Rect::default(); 3],
            error: String::new(),
            help: None,
            drawn: false,
            confirmable: false,
        }
    }
    pub fn new(snapshot: &Value, source: &str) -> Self {
        Self::base(
            snapshot,
            source,
            Kind::New {
                agent: false,
                independent: true,
            },
        )
    }
    pub fn actions(snapshot: &Value, source: &str) -> Self {
        Self::base(snapshot, source, Kind::Actions)
    }
    pub fn close(snapshot: &Value, source: &str, grant: Option<&Value>) -> Result<Self> {
        let request = if let Some(agent) = agent_view(snapshot, source) {
            let grant = grant.context("Viewing only: request control before closing this agent")?;
            json!({"op":"acp-close","target":agent_target(agent),"control":grant["control"],
                "owner_generation":grant["owner_generation"],"call_id":uuid::Uuid::new_v4().to_string()})
        } else {
            ensure!(
                snapshot["terminals"]
                    .as_array()
                    .is_some_and(|ts| ts.iter().any(|t| t["id"] == source)),
                "Session is no longer available"
            );
            json!({"op":"close","terminal":source})
        };
        let mut dialog = Self::base(snapshot, source, Kind::Close { request });
        dialog.focus = 1; // Keep working is the safe default, not destructive Enter.
        Ok(dialog)
    }
    pub fn rename(snapshot: &Value, source: &str) -> Result<Self> {
        let session = agent_view(snapshot, source)
            .or_else(|| {
                snapshot["terminals"]
                    .as_array()?
                    .iter()
                    .find(|t| t["id"] == source)
            })
            .context("Select a session to rename")?;
        let limit = 256;
        Ok(Self::base(
            snapshot,
            source,
            Kind::Rename {
                value: text(session, "name").into(),
                limit,
                replace: true,
                rejected: false,
            },
        ))
    }
    pub fn failure(&mut self, error: String) {
        self.error = error;
    }
    pub fn after_draw(&mut self) -> bool {
        if self.drawn && !self.confirmable {
            self.confirmable = true;
            true
        } else {
            false
        }
    }
    fn submit(&mut self) -> Action {
        match &self.kind {
            Kind::Actions => Action::Session {
                source: self.source.clone(),
                action: SESSION_ACTIONS[self.focus.min(7)].0,
            },
            Kind::New { agent, independent } => Action::New {
                source: self.source.clone(),
                agent: *agent,
                independent: *independent,
            },
            Kind::Close { request } => {
                if !self.confirmable {
                    self.error = "Review the complete close warning before confirming; enlarge the window if needed.".into();
                    Action::None
                } else {
                    Action::Submit(request.clone())
                }
            }
            Kind::Rename {
                value, rejected, ..
            } => {
                if *rejected || value.trim().is_empty() {
                    self.error = "Enter a valid name; rejected input was not saved.".into();
                    Action::None
                } else {
                    Action::Submit(json!({"op":"name","terminal":self.source,"name":value}))
                }
            }
        }
    }
    fn append(&mut self, input: &str) {
        let Kind::Rename {
            value,
            limit,
            replace,
            rejected,
        } = &mut self.kind
        else {
            return;
        };
        if (if *replace { 0 } else { value.len() }).saturating_add(input.len()) > *limit
            || input.chars().any(|c| c.is_control() || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) {
            *rejected = true;
            self.error = format!("Name must be safe text, at most {limit} bytes. Entire edit rejected.");
            return;
        }
        if *replace {
            value.clear();
            *replace = false;
        }
        value.push_str(input);
        *rejected = false;
        self.error.clear();
    }
    fn activate(&mut self, index: usize) -> Action {
        match index {
            0 => self.submit(),
            1 => Action::Cancel,
            _ if matches!(self.kind, Kind::Close { .. }) => Action::Detach,
            _ => {
                self.help = Some(view::Help::with_context_status(HELP, &self.error));
                Action::None
            }
        }
    }
    pub fn event(&mut self, event: &Event) -> Action {
        let fields = std::mem::take(&mut self.fields);
        let buttons = std::mem::take(&mut self.buttons);
        if let Some(help) = self.help.as_mut() {
            if help.event(event) {
                self.help = None;
                self.drawn = false;
                self.confirmable = false;
            }
            return Action::None;
        }
        if matches!(event, Event::Resize(..)) {
            self.drawn = false;
            self.confirmable = false;
        }
        if let Event::Mouse(mouse) = event {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some(index) = buttons
                    .iter()
                    .position(|r| r.contains((mouse.column, mouse.row).into()))
                {
                    return self.activate(index);
                }
                if let Some(index) = fields
                    .iter()
                    .position(|r| r.contains((mouse.column, mouse.row).into()))
                {
                    self.focus = index;
                    if matches!(self.kind, Kind::Actions) {
                        return self.submit();
                    }
                }
            }
            return Action::None;
        }
        if let Event::Paste(input) = event {
            if self.focus == 0 {
                self.append(input);
            }
            return Action::None;
        }
        let Event::Key(key) = event else {
            return Action::None;
        };
        if key.kind != KeyEventKind::Press {
            return Action::None;
        }
        if key.code == KeyCode::Esc {
            return Action::Cancel;
        }
        let editing = matches!(self.kind, Kind::Rename { .. }) && self.focus == 0;
        if key.code == KeyCode::F(1) || (!editing && key.code == KeyCode::Char('?')) {
            self.help = Some(view::Help::with_context_status(HELP, &self.error));
            return Action::None;
        }
        let count = match self.kind {
            Kind::New { .. } => 5,
            Kind::Rename { .. } => 4,
            Kind::Close { .. } => 3,
            Kind::Actions => 11,
        };
        let reverse = key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.focus = (self.focus + if reverse { count - 1 } else { 1 }) % count;
            return Action::None;
        }
        if !editing
            && key.modifiers.is_empty()
            && matches!(
                key.code,
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k')
            )
        {
            let up = matches!(key.code, KeyCode::Up | KeyCode::Char('k'));
            self.focus = (self.focus + if up { count - 1 } else { 1 }) % count;
            return Action::None;
        }
        if matches!(self.kind, Kind::Close { .. }) {
            match key.code {
                KeyCode::Char('y') if key.modifiers.is_empty() => return self.submit(),
                KeyCode::Char('n') if key.modifiers.is_empty() => return Action::Cancel,
                KeyCode::Char('d') if key.modifiers.is_empty() => return Action::Detach,
                KeyCode::Left => self.focus = self.focus.saturating_sub(1),
                KeyCode::Right => self.focus = (self.focus + 1).min(2),
                KeyCode::Enter => return self.activate(self.focus),
                _ => {}
            }
            return Action::None;
        }
        if key.code == KeyCode::Enter {
            let fields = match self.kind {
                Kind::New { .. } => 2,
                Kind::Actions => 8,
                _ => 1,
            };
            return if self.focus >= fields {
                self.activate(self.focus - fields)
            } else {
                self.submit()
            };
        }
        match &mut self.kind {
            Kind::New { agent, independent } => match key.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                    if self.focus == 0 {
                        *agent = !*agent;
                    } else if self.focus == 1 {
                        *independent = !*independent;
                    }
                }
                KeyCode::Char('t') => *agent = false,
                KeyCode::Char('a') => *agent = true,
                _ => {}
            },
            Kind::Rename {
                value,
                replace,
                rejected,
                ..
            } if editing => match key.code {
                KeyCode::Backspace | KeyCode::Delete => {
                    if *replace || key.code == KeyCode::Delete {
                        value.clear();
                    } else if let Some((index, _)) = value.grapheme_indices(true).next_back() {
                        value.truncate(index);
                    }
                    *replace = false;
                    *rejected = false;
                    self.error.clear();
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let mut bytes = [0; 4];
                    self.append(c.encode_utf8(&mut bytes));
                }
                _ => {}
            },
            _ => {}
        }
        Action::None
    }
    pub fn draw(&mut self, frame: &mut Frame<'_>, body: Rect) {
        self.fields.fill(Rect::default());
        self.buttons.fill(Rect::default());
        let width = body.width.saturating_sub(2).max(body.width.min(2)).min(76);
        let height = body.height.min(16);
        let area = Rect::new(
            body.x + (body.width - width) / 2,
            body.y + (body.height - height) / 2,
            width,
            height,
        );
        let title = match self.kind {
            Kind::New { .. } => " New session ",
            Kind::Close { .. } => " Close session? ",
            Kind::Rename { .. } => " Rename session ",
            Kind::Actions => " Session actions ",
        };
        let block = Block::default()
            .title(title)
            .borders(if width >= 4 && height >= 4 {
                Borders::ALL
            } else {
                Borders::NONE
            });
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let labels = if matches!(self.kind, Kind::Close { .. }) {
            [
                if self.confirmable {
                    "Close session"
                } else {
                    "Preparing…"
                },
                "Keep working",
                "Detach UI",
            ]
        } else if matches!(self.kind, Kind::New { .. }) {
            ["Continue", "Cancel", "Help"]
        } else if matches!(self.kind, Kind::Actions) {
            ["Select", "Return", "Help"]
        } else {
            ["Save name", "Cancel", "Help"]
        };
        let (buttons, mut content) = view::action_buttons(inner, labels, 1);
        self.buttons = buttons;
        let field_count = match self.kind {
            Kind::New { .. } => 2,
            Kind::Rename { .. } => 1,
            Kind::Close { .. } => 0,
            Kind::Actions => 8,
        };
        for (index, (rect, label)) in buttons.into_iter().zip(labels).enumerate() {
            view::button(frame, rect, label, self.focus == index + field_count);
        }
        if !self.error.is_empty() && content.height > 1 {
            let error = Rect::new(content.x, content.bottom() - 1, content.width, 1);
            content.height -= 1;
            frame.render_widget(
                Paragraph::new(clip_label(&self.error, error.width))
                    .style(Style::default().fg(Color::Yellow)),
                error,
            );
        }
        match &self.kind {
            Kind::Actions => {
                let selected = self.focus.min(7);
                let start = selected.saturating_sub(usize::from(content.height).saturating_sub(1));
                for (index, (_, label)) in SESSION_ACTIONS.iter().enumerate().skip(start) {
                    if content.is_empty() {
                        break;
                    }
                    let rect = Rect::new(content.x, content.y, content.width, 1);
                    self.fields[index] = rect;
                    frame.render_widget(
                        Paragraph::new(*label).style(if index == selected {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        }),
                        rect,
                    );
                    content.y += 1;
                    content.height -= 1;
                }
            }
            Kind::Close { .. } => {
                let warning = format!(
                    "Close {} in {}?\nThis ends the session and its running work.\nDetach UI keeps work running and does not release control.\ny closes; n/Esc keeps working; d detaches.",
                    clip_label(&self.name, 256),
                    clip_label(&self.project, 256)
                );
                let paragraph = Paragraph::new(warning).wrap(Wrap { trim: false });
                self.drawn = content.width > 0
                    && paragraph.line_count(content.width) <= usize::from(content.height)
                    && !buttons[0].is_empty();
                self.confirmable &= self.drawn;
                frame.render_widget(paragraph, content);
            }
            Kind::New { agent, independent } => {
                let lines = [
                    format!("Kind: {}", if *agent { "ACP agent" } else { "Terminal" }),
                    format!(
                        "Placement: {}",
                        if *independent {
                            "New independent project".into()
                        } else {
                            format!("Current project ({})", clip_label(&self.project, 128))
                        }
                    ),
                ];
                let start = if content.height < 4 && self.focus == 1 {
                    1
                } else {
                    0
                };
                for (index, line) in lines.iter().enumerate().skip(start) {
                    if content.is_empty() {
                        break;
                    }
                    let paragraph = Paragraph::new(line.as_str())
                        .wrap(Wrap { trim: false })
                        .style(if self.focus == index {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        });
                    let rows =
                        (paragraph.line_count(content.width).max(1) as u16).min(content.height);
                    let rect = Rect::new(content.x, content.y, content.width, rows);
                    self.fields[index] = rect;
                    frame.render_widget(paragraph, rect);
                    content.y += rows;
                    content.height -= rows;
                }
            }
            Kind::Rename { value, replace, .. } => {
                self.fields[0] = content;
                frame.render_widget(
                    Paragraph::new(format!(
                        "{}{}",
                        if *replace {
                            "Name (selected): "
                        } else {
                            "Name: "
                        },
                        clip_label(value, u16::MAX)
                    ))
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(if self.focus == 0 {
                        Color::Cyan
                    } else {
                        Color::White
                    })),
                    content,
                );
            }
        }
        if let Some(help) = self.help.as_mut() {
            help.draw(frame, body);
        }
    }
}
