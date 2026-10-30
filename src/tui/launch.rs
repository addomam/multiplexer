//! Deliberate profile launch; only safe daemon catalog metadata enters this UI.
use anyhow::{Context, Result, ensure};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use serde_json::{Value, json};

const LAUNCH: usize = 7;
const CANCEL: usize = 8;
const ADVANCED: usize = 9;
const HELP: usize = 10;
const HELP_TEXT: &str = "NEW ACP SESSION

Project/source selects the existing project whose directory is used. The placement chosen before this form is fixed: either add a session to that existing project, or create an independent unsaved project. Independent placement does not inherit project hooks or create directories.

Tab / Shift-Tab move through fields and visible actions. Click a field to focus it. Project and Profile use arrows or j/k; clicking their left/right halves selects previous/next. Enter on a selector chooses next. Unavailable profiles stay visible and cannot launch.

Name is literal text, including j, k and ?, limited to128 UTF8 bytes. Leave it blank to use the displayed profile-based unique name. Backspace edits and Ctrl-U clears the focused text field. Text and paste are accepted in full or rejected in full, never silently filtered or truncated.

Enter on a text field moves to the next control. Enter or Space on a button activates it. Launch is an explicit action; Ctrl-Enter is an optional shortcut. Escape cancels the form before submission. A submitted launch is attempted once: its unchanged draft is retained while admission is pending; Ctrl-Q detaches without cancelling the request. Definite rejection retains the draft and complete cause for correction, never automatic retry. F1, the Help button, or ? outside text fields opens this help; Escape returns to the unchanged form.

Advanced exposes exact model, mode, effort and Options JSON identifiers. Blank settings inherit profile defaults; absent defaults use agent defaults. Options JSON is an object mapping exact option IDs to exact choice IDs, for example {\"id\":\"choice\"}. Omitted IDs keep structured profile defaults. Unsupported settings fail; no premium/model fallback or permission policy is inferred. Requested settings are not agent-reported settings until confirmed.

On short screens the form scrolls to the focused field or action. Page Up/Down or the mouse wheel scroll the form without changing its draft. Arrows or j/k navigate when an action is focused. Errors remain indicated at the bottom and their complete text is also in the scrollable form.";

pub(super) enum Action {
    None,
    Cancel,
    Submit(Value),
}

pub(super) struct Dialog {
    projects: Vec<(String, String)>,
    profiles: Vec<Value>,
    project: usize,
    profile: usize,
    field: usize,
    fields: [String; 5],
    rejected: [bool; 5],
    independent: bool,
    advanced: bool,
    discriminator: String,
    error: String,
    pending: bool,
    help: Option<super::view::Help>,
    scroll: u16,
    max_scroll: u16,
    page: u16,
    follow_focus: bool,
    targets: Vec<(usize, Rect)>,
}

impl Dialog {
    pub(super) fn new(
        snapshot: &Value,
        source: &str,
        catalog: Value,
        independent: bool,
    ) -> Result<Self> {
        let projects = snapshot["projects"]
            .as_array()
            .context("no project catalog")?
            .iter()
            .filter_map(|project| {
                Some((
                    project["id"].as_str()?.to_owned(),
                    project["name"].as_str().unwrap_or("project").to_owned(),
                ))
            })
            .collect::<Vec<_>>();
        let profiles = catalog["profiles"]
            .as_array()
            .context("no ACP profile catalog")?
            .clone();
        ensure!(
            !projects.is_empty(),
            "Create a project before launching an ACP agent"
        );
        ensure!(!profiles.is_empty(), "No ACP agent profiles configured");
        let selected = super::agent_view(snapshot, source)
            .or_else(|| {
                snapshot["terminals"]
                    .as_array()?
                    .iter()
                    .find(|terminal| terminal["id"] == source)
            })
            .and_then(|view| view["project_id"].as_str());
        let project = projects
            .iter()
            .position(|(id, _)| Some(id.as_str()) == selected)
            .unwrap_or(0);
        let profile = profiles
            .iter()
            .position(|profile| profile["available"] == true)
            .unwrap_or(0);
        let mut dialog = Self {
            projects,
            profiles,
            project,
            profile,
            field: 0,
            fields: std::array::from_fn(|_| String::new()),
            rejected: [false; 5],
            independent,
            advanced: false,
            discriminator: uuid::Uuid::new_v4().simple().to_string()[..8].to_owned(),
            error: String::new(),
            pending: false,
            help: None,
            scroll: 0,
            max_scroll: 0,
            page: 1,
            follow_focus: true,
            targets: Vec::new(),
        };
        dialog.defaults();
        Ok(dialog)
    }

    fn defaults(&mut self) {
        let defaults = &self.profiles[self.profile]["defaults"];
        for (index, key) in ["model", "mode", "effort"].iter().enumerate() {
            self.fields[index + 1] = defaults[key].as_str().unwrap_or("").to_owned();
        }
        // Inherit structured defaults; only explicit overrides use the text editor.
        self.fields[4].clear();
        self.rejected[1..].fill(false);
    }

    fn default_name(&self) -> String {
        let profile = &self.profiles[self.profile];
        let label = profile["label"]
            .as_str()
            .or_else(|| profile["id"].as_str())
            .unwrap_or("profile");
        let mut end = label.len().min(128 - self.discriminator.len() - 1);
        while !label.is_char_boundary(end) {
            end -= 1;
        }
        format!("{} {}", &label[..end], self.discriminator)
    }

    fn request(&self) -> Result<Value> {
        ensure!(
            !self.rejected.contains(&true),
            "Correct rejected launch input before launching"
        );
        let profile = &self.profiles[self.profile];
        ensure!(
            profile["available"] == true,
            "Selected agent executable is unavailable; nothing launched"
        );
        ensure!(
            self.fields[0].is_empty() || !self.fields[0].trim().is_empty(),
            "Name must contain a non-whitespace character"
        );
        let mut settings = json!({});
        for (index, key) in ["model", "mode", "effort"].iter().enumerate() {
            if !self.fields[index + 1].is_empty() {
                settings[key] = json!(self.fields[index + 1]);
            }
        }
        if !self.fields[4].trim().is_empty() {
            settings["options"] =
                Value::Object(crate::acp_profiles::parse_options_json(&self.fields[4])?);
        }
        let mut request = json!({"op":"acp-open","project":self.projects[self.project].0,
            "agent":profile["id"],"settings":settings,
            "name":if self.fields[0].is_empty() { self.default_name() } else { self.fields[0].clone() }});
        if self.independent {
            request["new_project"] = json!(true);
        }
        Ok(request)
    }

    fn is_text(&self) -> bool {
        (2..=6).contains(&self.field)
    }

    fn focus_order(&self) -> &'static [usize] {
        if self.advanced {
            &[0, 1, 2, 3, 4, 5, 6, LAUNCH, CANCEL, ADVANCED, HELP]
        } else {
            &[0, 1, 2, LAUNCH, CANCEL, ADVANCED, HELP]
        }
    }

    fn focus(&mut self, next: bool) {
        let order = self.focus_order();
        let position = order
            .iter()
            .position(|field| *field == self.field)
            .unwrap_or(0);
        self.field = order[if next {
            (position + 1) % order.len()
        } else {
            (position + order.len() - 1) % order.len()
        }];
        self.follow_focus = true;
    }

    fn open_help(&mut self) {
        self.help = Some(super::view::Help::with_context_status(
            HELP_TEXT,
            &self.error,
        ));
    }

    pub(super) fn set_pending(&mut self) {
        self.pending = true;
        self.error.clear();
        self.targets.clear();
        self.follow_focus = true;
    }

    pub(super) fn failure(&mut self, error: String) {
        self.pending = false;
        self.error = error;
        self.help = None;
        self.targets.clear();
        self.follow_focus = true;
    }

    fn activate(&mut self) -> Action {
        self.targets.clear();
        match self.field {
            LAUNCH => match self.request() {
                Ok(request) => return Action::Submit(request),
                Err(error) => {
                    self.error = error.to_string();
                    self.follow_focus = true;
                }
            },
            CANCEL => return Action::Cancel,
            ADVANCED => {
                self.advanced = !self.advanced;
                self.targets.clear();
                self.follow_focus = true;
            }
            HELP => self.open_help(),
            0 | 1 => self.choose(true),
            _ => self.focus(true),
        }
        Action::None
    }

    pub(super) fn event(&mut self, event: &Event) -> Action {
        if let Some(help) = self.help.as_mut() {
            if help.event(event) {
                self.help = None;
            }
            return Action::None;
        }
        if self.pending {
            match event {
                Event::Key(key)
                    if key.kind != KeyEventKind::Release
                        && matches!(key.code, KeyCode::F(1) | KeyCode::Char('?')) =>
                {
                    self.open_help()
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && self.targets.iter().any(|(field, rect)| {
                            *field == HELP
                                && super::view::Geometry::hit(*rect, mouse.column, mouse.row)
                        }) =>
                {
                    self.open_help()
                }
                Event::Resize(_, _) => self.targets.clear(),
                _ => {}
            }
            return Action::None;
        }
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                let plain = !control && !key.modifiers.contains(KeyModifiers::ALT);
                match key.code {
                    KeyCode::Esc => return Action::Cancel,
                    KeyCode::F(1) => self.open_help(),
                    KeyCode::Char('?') if plain && !self.is_text() => {
                        self.open_help();
                    }
                    KeyCode::Tab => self.focus(true),
                    KeyCode::BackTab => self.focus(false),
                    KeyCode::Enter if control => {
                        self.field = LAUNCH;
                        return self.activate();
                    }
                    KeyCode::Enter => return self.activate(),
                    KeyCode::Char(' ') if plain && self.field >= LAUNCH => {
                        return self.activate();
                    }
                    KeyCode::Left | KeyCode::Up if self.field < 2 => self.choose(false),
                    KeyCode::Right | KeyCode::Down if self.field < 2 => self.choose(true),
                    KeyCode::Char('k') if plain && self.field < 2 => self.choose(false),
                    KeyCode::Char('j') if plain && self.field < 2 => self.choose(true),
                    KeyCode::Up => self.focus(false),
                    KeyCode::Down => self.focus(true),
                    KeyCode::Left if !self.is_text() => self.focus(false),
                    KeyCode::Right if !self.is_text() => self.focus(true),
                    KeyCode::Char('k') if plain && !self.is_text() => self.focus(false),
                    KeyCode::Char('j') if plain && !self.is_text() => self.focus(true),
                    KeyCode::Home if !self.is_text() => {
                        self.field = 0;
                        self.follow_focus = true;
                    }
                    KeyCode::End if !self.is_text() => {
                        self.field = HELP;
                        self.follow_focus = true;
                    }
                    KeyCode::PageUp => self.scroll_by(false, self.page),
                    KeyCode::PageDown => self.scroll_by(true, self.page),
                    KeyCode::Backspace if self.is_text() => {
                        let index = self.field - 2;
                        if self.fields[index].pop().is_some() {
                            self.rejected[index] = false;
                            self.error.clear();
                            self.follow_focus = true;
                        }
                    }
                    KeyCode::Char('u') if control && self.is_text() => {
                        let index = self.field - 2;
                        self.fields[index].clear();
                        self.rejected[index] = false;
                        self.error.clear();
                        self.follow_focus = true;
                    }
                    KeyCode::Char(character) if plain => {
                        let mut bytes = [0; 4];
                        self.append(character.encode_utf8(&mut bytes));
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => self.append(text),
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((field, rect)) = self.targets.iter().copied().find(|(_, rect)| {
                        super::view::Geometry::hit(*rect, mouse.column, mouse.row)
                    }) {
                        self.field = field;
                        self.follow_focus = true;
                        if field < 2 {
                            self.choose(mouse.column >= rect.x + rect.width / 2);
                        } else if field >= LAUNCH {
                            return self.activate();
                        }
                    }
                }
                MouseEventKind::ScrollUp => self.scroll_by(false, 3),
                MouseEventKind::ScrollDown => self.scroll_by(true, 3),
                _ => {}
            },
            Event::Resize(_, _) => {
                self.targets.clear();
                self.follow_focus = true;
            }
            _ => {}
        }
        self.targets.clear();
        Action::None
    }

    fn scroll_by(&mut self, next: bool, amount: u16) {
        self.scroll = if next {
            self.scroll.saturating_add(amount).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(amount)
        };
        self.follow_focus = false;
    }

    fn choose(&mut self, next: bool) {
        let (selection, length) = if self.field == 0 {
            (&mut self.project, self.projects.len())
        } else {
            (&mut self.profile, self.profiles.len())
        };
        *selection = if next {
            (*selection + 1) % length
        } else {
            (*selection + length - 1) % length
        };
        if self.field == 1 {
            self.defaults();
        }
        self.error.clear();
        self.follow_focus = true;
    }

    fn append(&mut self, text: &str) {
        if !self.is_text() || text.is_empty() {
            return;
        }
        let limit = match self.field {
            2 => 128,
            6 => crate::acp_profiles::OPTIONS_JSON_BYTES,
            _ => 256,
        };
        let index = self.field - 2;
        let field = &mut self.fields[index];
        self.follow_focus = true;
        if field.len().saturating_add(text.len()) > limit {
            self.rejected[index] = true;
            self.error = format!("Input exceeds {limit} bytes; edit rejected in full");
            return;
        }
        if text.chars().any(|character| {
            character.is_control() && (self.field != 6 || !matches!(character, '\n' | '\r' | '\t'))
        }) {
            self.rejected[index] = true;
            self.error = "Invalid control character; edit rejected in full".to_owned();
            return;
        }
        field.push_str(text);
        self.rejected[index] = false;
        self.error.clear();
    }

    fn field_text(&self, index: usize) -> String {
        let profile = &self.profiles[self.profile];
        let value = match index {
            0 => format!(
                "< {} [{}] >",
                self.projects[self.project].1, self.projects[self.project].0
            ),
            1 => format!(
                "< {} [{}] > [{}]",
                profile["label"].as_str().unwrap_or("profile"),
                profile["id"].as_str().unwrap_or(""),
                if profile["available"] == true {
                    "available"
                } else {
                    "UNAVAILABLE — cannot launch"
                },
            ),
            2 if self.fields[0].is_empty() => format!("{} [automatic]", self.default_name()),
            3..=5 if self.fields[index - 2].is_empty() => format!(
                "[inherited: {}]",
                profile["defaults"][["model", "mode", "effort"][index - 3]]
                    .as_str()
                    .unwrap_or("agent default; unreported"),
            ),
            6 if self.fields[4].is_empty() => {
                let defaults = &profile["defaults"]["options"];
                if defaults
                    .as_object()
                    .is_some_and(|options| !options.is_empty())
                {
                    format!("[inherited: {defaults}]")
                } else {
                    "[inherited: agent defaults; unreported]".to_owned()
                }
            }
            _ => self.fields[index - 2].clone(),
        };
        let label = [
            "Project/source",
            "Profile",
            "Name",
            "Model",
            "Mode",
            "Effort",
            "Options JSON",
        ][index];
        format!(
            "{} {label}{}: {}",
            if self.field == index { ">" } else { " " },
            if index >= 2 && self.rejected[index - 2] {
                " [rejected]"
            } else {
                ""
            },
            visible_text(&value),
        )
    }

    pub(super) fn draw(&mut self, frame: &mut Frame<'_>) {
        let mut screen = frame.area();
        let header = screen.height.min(super::HEADER);
        screen.y += header;
        screen.height -= header;
        let width = screen.width.min(100);
        let height = screen.height.min(28);
        let area = Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        );
        self.targets.clear();
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(if width >= 4 && height >= 4 {
                Borders::ALL
            } else {
                Borders::NONE
            })
            .title(" New ACP session ")
            .title_bottom(if self.pending {
                " Launch pending · Ctrl-Q detach · F1 help "
            } else if width >= 55 {
                " Tab focus · F1 help · Esc cancel · PgUp/PgDn scroll "
            } else {
                " Tab · F1 help · Esc back "
            })
            .style(Style::default().bg(Color::Black).fg(Color::White));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.is_empty() {
            return;
        }
        let error_height = if self.error.is_empty() || inner.height < 2 {
            0
        } else {
            Paragraph::new(format!("Error: {}", visible_text(&self.error)))
                .wrap(Wrap { trim: false })
                .line_count(inner.width)
                .min(usize::from((inner.height / 3).max(1))) as u16
        };
        let body = Rect::new(inner.x, inner.y, inner.width, inner.height - error_height);
        let scope = if self.independent {
            "Independent project: source supplies its directory only. Creates an unsaved project; no inherited hooks or directory creation."
        } else {
            "Existing project: adds ACP to the selected project. Existing terminals stay in that project, unchanged."
        };
        let mut rows = vec![(None, scope.to_owned())];
        if self.pending {
            rows.push((None, "Launch submitted once; awaiting admission. Draft unchanged. Ctrl-Q detaches without cancelling the request.".to_owned()));
        }
        let field_count = if self.advanced { 7 } else { 3 };
        for field in 0..field_count {
            rows.push((Some(field), self.field_text(field)));
        }
        if self.advanced {
            rows.push((None, "Exact IDs only. Blank settings inherit defaults; omitted option IDs retain structured defaults. Requested is not agent-reported. No fallback.".to_owned()));
        }
        if !self.error.is_empty() {
            rows.push((None, format!("Error: {}", visible_text(&self.error))));
        }
        let mut y = 0usize;
        let mut layout = Vec::with_capacity(rows.len());
        let mut focused = None;
        for (field, text) in rows {
            let height = Paragraph::new(text.as_str())
                .wrap(Wrap { trim: false })
                .line_count(body.width)
                .max(1);
            if field == Some(self.field) {
                focused = Some((y, height));
            }
            layout.push((field, text, y, height));
            y += height;
        }
        let labels = [
            if self.pending {
                "Launching…"
            } else {
                "Launch"
            },
            if self.pending { "Waiting…" } else { "Cancel" },
            if self.pending {
                "Waiting…"
            } else if self.advanced {
                "Basic"
            } else {
                "Advanced"
            },
            "Help",
        ];
        let minimum = labels
            .iter()
            .map(|label| label.len() + 4)
            .max()
            .unwrap_or(0);
        let button_rows = if usize::from(body.width) < minimum * labels.len() {
            4
        } else {
            1
        };
        let (buttons, _) =
            super::view::action_buttons(Rect::new(0, 0, body.width, button_rows), labels, 0);
        if self.field >= LAUNCH {
            let button = buttons[self.field - LAUNCH];
            focused = Some((y + usize::from(button.y), usize::from(button.height)));
        }
        self.max_scroll = (y + usize::from(button_rows))
            .saturating_sub(usize::from(body.height))
            .min(usize::from(u16::MAX)) as u16;
        self.page = body.height.saturating_sub(1).max(1);
        if self.follow_focus {
            if let Some((start, height)) = focused {
                let end = start + height;
                let top = usize::from(self.scroll);
                let viewport = usize::from(body.height);
                let next = if height > viewport {
                    if self.is_text() {
                        end.saturating_sub(viewport)
                    } else {
                        start
                    }
                } else if start < top {
                    start
                } else if end > top + viewport {
                    end.saturating_sub(viewport)
                } else {
                    top
                };
                self.scroll = next.min(usize::from(self.max_scroll)) as u16;
            }
            self.follow_focus = false;
        }
        self.scroll = self.scroll.min(self.max_scroll);
        let top = usize::from(self.scroll);
        let bottom = top + usize::from(body.height);
        for (field, text, start, height) in layout {
            let first = start.max(top);
            let last = (start + height).min(bottom);
            if first >= last {
                continue;
            }
            let rect = Rect::new(
                body.x,
                body.y + (first - top) as u16,
                body.width,
                (last - first) as u16,
            );
            let style = if field == Some(self.field) {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if field.is_some() {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .scroll(((first - start).min(usize::from(u16::MAX)) as u16, 0))
                    .style(style),
                rect,
            );
            if let Some(field) = field {
                self.targets.push((field, rect));
            }
        }
        for (index, button) in buttons.into_iter().enumerate() {
            let start = y + usize::from(button.y);
            if start < top || start >= bottom || button.is_empty() {
                continue;
            }
            let rect = Rect::new(
                body.x + button.x,
                body.y + (start - top) as u16,
                button.width,
                1,
            );
            super::view::button(frame, rect, labels[index], self.field == LAUNCH + index);
            self.targets.push((LAUNCH + index, rect));
        }
        if error_height > 0 {
            frame.render_widget(
                Paragraph::new(format!("Error: {}", visible_text(&self.error)))
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(Color::Red)),
                Rect::new(inner.x, body.bottom(), inner.width, error_height),
            );
        }
        if self.max_scroll > 0 && width >= 16 && height >= 4 {
            frame.render_widget(
                Paragraph::new(format!(
                    " {}/{} ",
                    self.scroll.saturating_add(1),
                    self.max_scroll.saturating_add(1)
                ))
                .style(Style::default().fg(Color::Gray)),
                Rect::new(area.right().saturating_sub(15), area.y, 14, 1),
            );
        }
        if let Some(help) = self.help.as_mut() {
            help.draw(frame, screen);
        }
    }
}

// Presentation only: retain every accepted byte in the draft and request. Prevent
// control/bidi display effects without truncating the scrollable value.
fn visible_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            (!character.is_control() || matches!(*character, '\n' | '\t'))
                && !matches!(*character, '\u{061c}' | '\u{200e}' | '\u{200f}'
                | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}
