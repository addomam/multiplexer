//! Deliberate profile launch; only safe daemon catalog metadata enters this UI.
use anyhow::{Context, Result, ensure};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};
use serde_json::{Value, json};
use std::fmt::Write as _;

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
    error: String,
    display: String,
}

impl Dialog {
    pub(super) fn new(snapshot: &Value, source: &str, catalog: Value) -> Result<Self> {
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
            error: String::new(),
            display: String::new(),
        };
        dialog.defaults();
        dialog.render();
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
            "agent":profile["id"],"settings":settings});
        if !self.fields[0].is_empty() {
            request["name"] = json!(self.fields[0]);
        }
        Ok(request)
    }

    pub(super) fn event(&mut self, event: &Event) -> Action {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Esc => return Action::Cancel,
                    KeyCode::Tab => self.field = (self.field + 1) % 7,
                    KeyCode::BackTab => self.field = (self.field + 6) % 7,
                    KeyCode::Enter => match self.request() {
                        Ok(request) => return Action::Submit(request),
                        Err(error) => self.error = error.to_string(),
                    },
                    KeyCode::Left | KeyCode::Up if self.field < 2 => self.choose(false),
                    KeyCode::Right | KeyCode::Down if self.field < 2 => self.choose(true),
                    KeyCode::Backspace if self.field >= 2 => {
                        let index = self.field - 2;
                        if self.fields[index].pop().is_some() {
                            self.rejected[index] = false;
                            self.error.clear();
                        }
                    }
                    KeyCode::Char('u') if control && self.field >= 2 => {
                        let index = self.field - 2;
                        self.fields[index].clear();
                        self.rejected[index] = false;
                        self.error.clear();
                    }
                    KeyCode::Char(character)
                        if !control && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        let mut bytes = [0; 4];
                        self.append(character.encode_utf8(&mut bytes));
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => self.append(text),
            _ => {}
        }
        self.render();
        Action::None
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
    }

    fn append(&mut self, text: &str) {
        if self.field < 2 || text.is_empty() {
            return;
        }
        let limit = if self.field == 6 {
            crate::acp_profiles::OPTIONS_JSON_BYTES
        } else {
            256
        };
        let index = self.field - 2;
        let field = &mut self.fields[index];
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

    fn render(&mut self) {
        self.display.clear();
        let profile = &self.profiles[self.profile];
        let values = [
            self.projects[self.project].1.as_str(),
            profile["label"].as_str().unwrap_or("profile"),
            self.fields[0].as_str(),
            self.fields[1].as_str(),
            self.fields[2].as_str(),
            self.fields[3].as_str(),
            self.fields[4].as_str(),
        ];
        for (index, (label, value)) in [
            "Project",
            "Profile",
            "Name",
            "Model",
            "Mode",
            "Effort",
            "Options JSON",
        ]
        .into_iter()
        .zip(values)
        .enumerate()
        {
            let inherited = match index {
                3 => profile["defaults"]["model"].as_str(),
                4 => profile["defaults"]["mode"].as_str(),
                5 => profile["defaults"]["effort"].as_str(),
                6 if profile["defaults"]["options"]
                    .as_object()
                    .is_some_and(|options| !options.is_empty()) =>
                {
                    Some("profile option defaults")
                }
                _ => None,
            }
            .filter(|_| value.is_empty());
            let _ = writeln!(
                self.display,
                "{} {label}{}: {}{}{}{}",
                if index == self.field { ">" } else { " " },
                if index >= 2 && self.rejected[index - 2] {
                    " [rejected]"
                } else {
                    ""
                },
                if inherited.is_some() {
                    "[inherited: "
                } else {
                    ""
                },
                super::clip_label(inherited.unwrap_or(value), 4096),
                if inherited.is_some() { "]" } else { "" },
                if index == 1 {
                    if profile["available"] == true {
                        " [available]"
                    } else {
                        " [unavailable]"
                    }
                } else {
                    ""
                }
            );
        }
        self.display.push_str("Tab/Shift-Tab fields · arrows select · Ctrl-U clears · Enter launch · Esc cancel\nBlank settings inherit profile defaults; absent defaults use agent defaults.\nOptions JSON: {\"id\":\"choice\"}; omitted IDs keep their profile defaults.\nAgent defaults are unreported until confirmed. No permission policy is inferred.\nUse exact advertised identifiers; unsupported settings fail, never fall back.\n");
        self.display.push_str(&super::clip_label(&self.error, 1024));
    }

    pub(super) fn draw(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let width = area.width.saturating_sub(2).min(100);
        let height = area.height.saturating_sub(2).min(15);
        let dialog = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, dialog);
        frame.render_widget(
            Paragraph::new(self.display.as_str())
                .style(Style::default().bg(Color::Black).fg(Color::White))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" New ACP agent "),
                ),
            dialog,
        );
    }
}
