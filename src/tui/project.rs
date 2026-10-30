//! Saved project metadata only. All mutations return to the original UI connection.
use super::text;
use anyhow::{Context, Result, ensure};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};
use serde_json::{Map, Value, json};
use unicode_width::UnicodeWidthChar;

pub(super) enum Action {
    None,
    Cancel,
    Submit(Value),
}

#[derive(Default)]
enum Mode {
    #[default]
    List,
    Save {
        project: String,
        terminal: String,
        context: String,
    },
    Config,
    Hook(Option<usize>),
    Recipe(Option<String>),
    Remove {
        project: String,
        name: String,
    },
}

struct Draft {
    project: String,
    context: String,
    name: Field,
    hooks: Vec<Value>,
    recipes: Map<String, Value>,
}

#[derive(Clone, Default)]
struct Field {
    value: String,
    cursor: usize,
    multiline: bool,
    rejected: bool,
}

impl Field {
    fn new(value: String, multiline: bool) -> Self {
        Self {
            cursor: value.len(),
            value,
            multiline,
            rejected: false,
        }
    }

    fn insert(&mut self, addition: &str) -> Result<()> {
        let limit = if self.multiline {
            crate::client::MAX_FRAME as usize / 8
        } else {
            256
        };
        if self.value.len().saturating_add(addition.len()) > limit
            || addition
                .chars()
                .any(|c| c.is_control() && !(self.multiline && matches!(c, '\n' | '\r' | '\t')))
        {
            self.rejected = true;
            anyhow::bail!(
                "Input rejected: control characters or field size limit; correct this field before saving"
            );
        }
        self.value.insert_str(self.cursor, addition);
        self.cursor += addition.len();
        self.rejected = false;
        Ok(())
    }

    fn previous(&self) -> usize {
        self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.value[self.cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Submit,
    Back,
    Help,
    Edit,
    Forget,
    AddHook,
    AddRecipe,
    RemoveEntry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Field(usize),
    Context,
    Project(usize),
    Hook(usize),
    Recipe(String),
    Button(Command),
}

struct Item {
    label: String,
    target: Option<Target>,
    field: Option<usize>,
}

impl Item {
    fn text(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            target: None,
            field: None,
        }
    }

    fn control(label: impl Into<String>, target: Target) -> Self {
        Self {
            label: label.into(),
            target: Some(target),
            field: None,
        }
    }

    fn field(label: &str, index: usize) -> Self {
        Self {
            label: label.into(),
            target: Some(Target::Field(index)),
            field: Some(index),
        }
    }

    fn button(label: &str, command: Command) -> Self {
        Self::control(format!("[ {label} ]"), Target::Button(command))
    }
}

struct Row {
    text: String,
    target: Option<Target>,
    cursor: Option<u16>,
    error: bool,
}

#[derive(Default)]
pub(super) struct Dialog {
    projects: Vec<Value>,
    selected: usize,
    mode: Mode,
    draft: Option<Draft>,
    fields: Vec<Field>,
    focus: Option<Target>,
    error: String,
    reveal_error: bool,
    reveal_focus: bool,
    scroll: usize,
    max_scroll: usize,
    page: usize,
    hits: Vec<(Rect, Target)>,
    help: Option<super::view::Help>,
}

impl Dialog {
    pub(super) fn manage(snapshot: &Value) -> Self {
        Self {
            projects: snapshot["projects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|project| project["saved"] == true)
                .cloned()
                .collect(),
            focus: Some(Target::Project(0)),
            ..Self::default()
        }
    }

    pub(super) fn save(snapshot: &Value, source: &str, preview: Value) -> Result<Self> {
        ensure!(
            snapshot["project_save_terminal"] == true,
            "Save project needs an updated daemon. Finish live work before upgrading; nothing was saved."
        );
        let native = snapshot["terminals"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|terminal| terminal["id"] == source);
        let view = native
            .or_else(|| super::agent_view(snapshot, source))
            .context("Select a session before saving a project")?;
        let project = snapshot["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|project| project["id"] == view["project_id"])
            .context("Session project is unavailable")?;
        let id = project["id"]
            .as_str()
            .context("Session project has no identity")?;
        ensure!(
            preview["project"].as_str() == Some(id),
            "Save preview belongs to a different project; reopen Save project"
        );
        let context = preview["context"]
            .as_str()
            .filter(|context| !context.is_empty())
            .context("Daemon did not provide a saved directory preview")?
            .to_owned();
        let name = if project["saved"] == true {
            text(project, "name").to_owned()
        } else {
            String::new()
        };
        Ok(Self {
            mode: Mode::Save {
                project: id.to_owned(),
                terminal: source.to_owned(),
                context,
            },
            fields: vec![Field::new(name, false)],
            focus: Some(Target::Field(0)),
            reveal_focus: true,
            ..Self::default()
        })
    }

    pub(super) fn refresh(&mut self, snapshot: &Value) {
        let selected = self
            .projects
            .get(self.selected)
            .map(|project| project["id"].clone());
        let mut updated = Self::manage(snapshot);
        updated.selected = updated
            .projects
            .iter()
            .position(|project| Some(&project["id"]) == selected.as_ref())
            .unwrap_or(self.selected.min(updated.projects.len().saturating_sub(1)));
        updated.focus = Some(Target::Project(updated.selected));
        *self = updated;
    }

    pub(super) fn failure(&mut self, message: String) {
        self.error = message;
        self.reveal_error = true;
    }

    fn reset_page(&mut self, focus: Target) {
        self.focus = Some(focus);
        self.scroll = 0;
        self.reveal_focus = true;
        self.error.clear();
        self.reveal_error = false;
        self.hits.clear();
    }

    fn edit_config(&mut self) {
        let Some(project) = self.projects.get(self.selected) else {
            return;
        };
        let result = (|| -> Result<Draft> {
            Ok(Draft {
                project: text(project, "id").to_owned(),
                context: text(project, "context").to_owned(),
                name: Field::new(
                    project["name"]
                        .as_str()
                        .context("Project name is not text")?
                        .to_owned(),
                    false,
                ),
                hooks: project["hooks"]
                    .as_array()
                    .context("Project hooks are unavailable")?
                    .clone(),
                recipes: project["recipes"]
                    .as_object()
                    .context("Project recipes are unavailable")?
                    .clone(),
            })
        })();
        match result {
            Ok(draft) => {
                self.fields = vec![draft.name.clone()];
                self.draft = Some(draft);
                self.mode = Mode::Config;
                self.reset_page(Target::Field(0));
            }
            Err(error) => self.failure(format!("{error:#}")),
        }
    }

    fn open_hook(&mut self, index: Option<usize>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        draft.name.clone_from(&self.fields[0]);
        let hook = index.and_then(|i| draft.hooks.get(i));
        let source = hook.map_or("", |hook| text(hook, "source")).to_owned();
        let timeout = hook
            .and_then(|hook| hook["timeout_ms"].as_u64())
            .unwrap_or(30_000)
            .to_string();
        self.fields = vec![Field::new(source, true), Field::new(timeout, false)];
        self.mode = Mode::Hook(index);
        self.reset_page(Target::Field(0));
    }

    fn open_recipe(&mut self, name: Option<String>) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        draft.name.clone_from(&self.fields[0]);
        let command = name
            .as_ref()
            .and_then(|name| draft.recipes.get(name))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        self.fields = vec![
            Field::new(name.clone().unwrap_or_default(), false),
            Field::new(command, true),
        ];
        self.mode = Mode::Recipe(name);
        self.reset_page(Target::Field(0));
    }

    fn return_to_config(&mut self) {
        let name = self
            .draft
            .as_ref()
            .expect("entry editor has project draft")
            .name
            .clone();
        self.fields = vec![name];
        self.mode = Mode::Config;
        self.reset_page(Target::Field(0));
    }

    fn back(&mut self) -> Action {
        match self.mode {
            Mode::Hook(_) | Mode::Recipe(_) => self.return_to_config(),
            Mode::Config | Mode::Remove { .. } => {
                self.mode = Mode::List;
                self.draft = None;
                self.fields.clear();
                self.reset_page(Target::Project(self.selected));
            }
            Mode::List | Mode::Save { .. } => return Action::Cancel,
        }
        Action::None
    }

    fn submit(&mut self) -> Action {
        let request = (|| -> Result<Option<Value>> {
            ensure!(
                !self.fields.iter().any(|field| field.rejected),
                "Correct rejected input before saving"
            );
            match &self.mode {
                Mode::Save {
                    project,
                    terminal,
                    context,
                } => {
                    ensure!(
                        !self.fields[0].value.trim().is_empty(),
                        "Enter a project name"
                    );
                    Ok(Some(
                        json!({"op":"project-save","project":project,"terminal":terminal,
                        "name":self.fields[0].value,"expected_context":context}),
                    ))
                }
                Mode::Config => {
                    let draft = self
                        .draft
                        .as_ref()
                        .context("Project draft is unavailable")?;
                    ensure!(
                        !self.fields[0].value.trim().is_empty(),
                        "Enter a project name"
                    );
                    Ok(Some(json!({"op":"project-config","project":draft.project,
                        "config":{"name":self.fields[0].value,"hooks":draft.hooks,"recipes":draft.recipes}})))
                }
                Mode::Hook(index) => {
                    ensure!(!self.fields[0].value.trim().is_empty(), "Enter hook source");
                    let timeout: u64 = self.fields[1]
                        .value
                        .parse()
                        .context("Timeout must be a whole number of milliseconds")?;
                    ensure!(
                        (1..=300_000).contains(&timeout),
                        "Timeout must be between 1 and 300000 milliseconds"
                    );
                    let draft = self
                        .draft
                        .as_mut()
                        .context("Project draft is unavailable")?;
                    let mut hook = index
                        .map(|i| draft.hooks[i].clone())
                        .unwrap_or_else(|| json!({}));
                    hook["source"] = json!(self.fields[0].value);
                    hook["timeout_ms"] = json!(timeout);
                    if let Some(index) = index {
                        draft.hooks[*index] = hook;
                    } else {
                        draft.hooks.push(hook);
                    }
                    Ok(None)
                }
                Mode::Recipe(original) => {
                    let name = &self.fields[0].value;
                    ensure!(!name.trim().is_empty(), "Enter a recipe name");
                    let draft = self
                        .draft
                        .as_mut()
                        .context("Project draft is unavailable")?;
                    ensure!(
                        original.as_ref() == Some(name) || !draft.recipes.contains_key(name),
                        "A recipe with this name already exists; choose another name"
                    );
                    if let Some(original) = original {
                        draft.recipes.remove(original);
                    }
                    draft
                        .recipes
                        .insert(name.clone(), json!(self.fields[1].value));
                    Ok(None)
                }
                Mode::Remove { project, .. } => {
                    Ok(Some(json!({"op":"project-remove","project":project})))
                }
                Mode::List => {
                    let project = self
                        .projects
                        .get(self.selected)
                        .context("No saved projects")?;
                    Ok(Some(json!({"op":"project-open","project":project["id"]})))
                }
            }
        })();
        match request {
            Ok(Some(request)) => Action::Submit(request),
            Ok(None) => {
                self.return_to_config();
                Action::None
            }
            Err(error) => {
                self.failure(format!("{error:#}"));
                Action::None
            }
        }
    }

    fn activate(&mut self, target: Target) -> Action {
        match target {
            Target::Button(Command::Submit) => return self.submit(),
            Target::Button(Command::Back) => return self.back(),
            Target::Button(Command::Help) => {
                self.help = Some(super::view::Help::new(
                    "Projects\n\nTab / Shift-Tab moves through fields and actions. Click a field or action. Enter activates a button or selected entry. j/k and arrows navigate only outside text fields; j, k and ? are literal text inside fields. F1 or Help opens this page. Escape returns one level without applying that level's changes.\n\nSave project shows the exact authoritative directory. It may be a Git root, not the session's cwd. Select the directory and use Page Up/Down or the mouse wheel to inspect all of it. A changed directory is rejected by the daemon; reopen Save to preview again.\n\nEdit changes the saved name, ordered hooks and named recipe commands. The saved directory cannot be edited. Add/Edit/Remove changes only the local draft. Apply sends the complete name/hooks/recipes patch once. Cancel discards it. A rejected Apply retains the draft.\n\nHook source and recipe commands support literal multiline paste and Enter. Tab reaches Keep/Cancel/Remove. Left/Right, Home/End, Backspace and Delete edit text; Ctrl-U clears the focused field. Ctrl-S is an optional save accelerator. Input is rejected as a whole, never silently filtered or truncated.\n\nForget requires confirmation and removes only the saved bookmark. It never deletes files or stops native/ACP sessions.\n\nPage Up/Down or the mouse wheel scrolls long forms, directories and errors. Tab brings the next focused field or action into view.",
                ));
            }
            Target::Button(Command::Edit) => self.edit_config(),
            Target::Button(Command::Forget) => {
                if let Some(project) = self.projects.get(self.selected) {
                    self.mode = Mode::Remove {
                        project: text(project, "id").to_owned(),
                        name: text(project, "name").to_owned(),
                    };
                    self.fields.clear();
                    self.reset_page(Target::Button(Command::Back));
                }
            }
            Target::Button(Command::AddHook) => self.open_hook(None),
            Target::Button(Command::AddRecipe) => self.open_recipe(None),
            Target::Button(Command::RemoveEntry) => {
                if let Some(draft) = self.draft.as_mut() {
                    match &self.mode {
                        Mode::Hook(Some(index)) => {
                            draft.hooks.remove(*index);
                        }
                        Mode::Recipe(Some(name)) => {
                            draft.recipes.remove(name);
                        }
                        _ => return Action::None,
                    }
                    self.return_to_config();
                }
            }
            Target::Project(index) => {
                self.selected = index;
                return self.submit();
            }
            Target::Hook(index) => self.open_hook(Some(index)),
            Target::Recipe(name) => self.open_recipe(Some(name)),
            Target::Field(_) | Target::Context => {}
        }
        Action::None
    }

    fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        match &self.mode {
            Mode::List => {
                if self.projects.is_empty() {
                    items.push(Item::text("No saved projects. Return to Menu and use Save project on a session. Saving never creates a directory."));
                }
                for (index, project) in self.projects.iter().enumerate() {
                    items.push(Item::control(
                        format!(
                            "{}{}\n{}",
                            if index == self.selected { "> " } else { "  " },
                            text(project, "name"),
                            text(project, "context")
                        ),
                        Target::Project(index),
                    ));
                }
                items.push(Item::button("Open", Command::Submit));
                items.push(Item::button("Edit", Command::Edit));
                items.push(Item::button("Forget…", Command::Forget));
            }
            Mode::Save { context, .. } => {
                items.push(Item::field("Project name", 0));
                items.push(Item::control(
                    format!("Exact saved directory (read-only):\n{context}"),
                    Target::Context,
                ));
                items.push(Item::text("Bookmark this directory; no directory is created. If it changes before Save, nothing is saved."));
                items.push(Item::button("Save", Command::Submit));
            }
            Mode::Config => {
                let draft = self.draft.as_ref().expect("config has project draft");
                items.push(Item::field("Project name", 0));
                items.push(Item::control(
                    format!("Saved directory (read-only):\n{}", draft.context),
                    Target::Context,
                ));
                items.push(Item::text("Hooks — run in the exact order below"));
                if draft.hooks.is_empty() {
                    items.push(Item::text(
                        "No hooks. Add a hook to run setup when opening this project.",
                    ));
                }
                for (index, hook) in draft.hooks.iter().enumerate() {
                    items.push(Item::control(
                        format!(
                            "Edit hook {} · {} ms\n{}",
                            index + 1,
                            hook["timeout_ms"],
                            text(hook, "source")
                        ),
                        Target::Hook(index),
                    ));
                }
                items.push(Item::button("Add hook", Command::AddHook));
                items.push(Item::text("Recipes — named commands"));
                if draft.recipes.is_empty() {
                    items.push(Item::text(
                        "No recipes. Add a named command without editing JSON.",
                    ));
                }
                for (name, source) in &draft.recipes {
                    items.push(Item::control(
                        format!(
                            "Edit recipe: {name}\n{}",
                            source
                                .as_str()
                                .unwrap_or("Invalid non-text recipe; retained unchanged")
                        ),
                        Target::Recipe(name.clone()),
                    ));
                }
                items.push(Item::button("Add recipe", Command::AddRecipe));
                items.push(Item::text("Entry changes stay in this draft until Apply. Cancel leaves saved configuration unchanged."));
                items.push(Item::button("Apply", Command::Submit));
            }
            Mode::Hook(index) => {
                items.push(Item::text("Keep changes this draft only; Apply on the project form saves all changes together."));
                items.push(Item::field("Hook source (multiline)", 0));
                items.push(Item::field("Timeout in milliseconds (1–300000)", 1));
                items.push(Item::button("Keep hook", Command::Submit));
                if index.is_some() {
                    items.push(Item::button("Remove hook from draft", Command::RemoveEntry));
                }
            }
            Mode::Recipe(name) => {
                items.push(Item::text("Recipe names are unique. Commands are preserved exactly, including whitespace and newlines."));
                items.push(Item::field("Recipe name", 0));
                items.push(Item::field("Command (multiline)", 1));
                items.push(Item::button("Keep recipe", Command::Submit));
                if name.is_some() {
                    items.push(Item::button(
                        "Remove recipe from draft",
                        Command::RemoveEntry,
                    ));
                }
            }
            Mode::Remove { name, .. } => {
                items.push(Item::text(format!("Forget saved configuration for {name}?\n\nDirectories and files will NOT be deleted. Native/ACP sessions keep running. This only removes the saved bookmark.")));
                items.push(Item::button("Forget bookmark", Command::Submit));
            }
        }
        items.push(Item::button(
            if matches!(self.mode, Mode::List) {
                "Return"
            } else {
                "Cancel"
            },
            Command::Back,
        ));
        items.push(Item::button("Help", Command::Help));
        items
    }

    fn move_focus(&mut self, reverse: bool) {
        let targets: Vec<_> = self
            .items()
            .into_iter()
            .filter_map(|item| item.target)
            .collect();
        if targets.is_empty() {
            return;
        }
        let current = targets
            .iter()
            .position(|target| Some(target) == self.focus.as_ref());
        let next = match current {
            Some(i) if reverse => (i + targets.len() - 1) % targets.len(),
            Some(i) => (i + 1) % targets.len(),
            None if reverse => targets.len() - 1,
            None => 0,
        };
        self.focus = Some(targets[next].clone());
        if let Some(Target::Project(index)) = self.focus {
            self.selected = index;
        }
        self.reveal_focus = true;
    }

    fn scroll_by(&mut self, down: bool, amount: usize) {
        self.scroll = if down {
            self.scroll.saturating_add(amount).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(amount)
        };
        self.reveal_focus = false;
        self.reveal_error = false;
    }

    pub(super) fn event(&mut self, event: &Event) -> Action {
        if let Some(help) = self.help.as_mut() {
            if help.event(event) {
                self.help = None;
            }
            return Action::None;
        }
        if let Event::Mouse(mouse) = event {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.scroll_by(true, 3),
                MouseEventKind::ScrollUp => self.scroll_by(false, 3),
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((_, target)) = self
                        .hits
                        .iter()
                        .find(|(area, _)| area.contains((mouse.column, mouse.row).into()))
                    {
                        let target = target.clone();
                        self.focus = Some(target.clone());
                        self.reveal_focus = true;
                        if let Target::Project(index) = target {
                            self.selected = index;
                        }
                        if matches!(
                            target,
                            Target::Button(_) | Target::Hook(_) | Target::Recipe(_)
                        ) {
                            return self.activate(target);
                        }
                    }
                }
                _ => {}
            }
            return Action::None;
        }
        if let Event::Paste(paste) = event {
            if let Some(Target::Field(index)) = self.focus {
                if let Err(error) = self.fields[index].insert(paste) {
                    self.failure(format!("{error:#}"));
                } else {
                    self.error.clear();
                    self.reveal_focus = true;
                }
            }
            return Action::None;
        }
        let Event::Key(key) = event else {
            return Action::None;
        };
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return self.back(),
            KeyCode::F(1) => return self.activate(Target::Button(Command::Help)),
            KeyCode::Tab | KeyCode::BackTab => {
                self.move_focus(
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT),
                );
                return Action::None;
            }
            KeyCode::PageDown => {
                self.scroll_by(true, self.page.max(1));
                return Action::None;
            }
            KeyCode::PageUp => {
                self.scroll_by(false, self.page.max(1));
                return Action::None;
            }
            KeyCode::Char('s') if control => return self.submit(),
            _ => {}
        }
        if let Some(Target::Field(index)) = self.focus {
            let field = &mut self.fields[index];
            let mut addition = None;
            let mut changed = false;
            match key.code {
                KeyCode::Char('u') if control => {
                    field.value.clear();
                    field.cursor = 0;
                    changed = true;
                }
                KeyCode::Backspace => {
                    let previous = field.previous();
                    field.value.drain(previous..field.cursor);
                    field.cursor = previous;
                    changed = true;
                }
                KeyCode::Delete => {
                    field.value.drain(field.cursor..field.next());
                    changed = true;
                }
                KeyCode::Left => field.cursor = field.previous(),
                KeyCode::Right => field.cursor = field.next(),
                KeyCode::Home => field.cursor = 0,
                KeyCode::End => field.cursor = field.value.len(),
                KeyCode::Enter if field.multiline => addition = Some("\n".to_owned()),
                KeyCode::Enter if matches!(self.mode, Mode::Save { .. }) => return self.submit(),
                KeyCode::Enter => {
                    self.move_focus(false);
                    return Action::None;
                }
                KeyCode::Up => {
                    self.scroll_by(false, 1);
                    return Action::None;
                }
                KeyCode::Down => {
                    self.scroll_by(true, 1);
                    return Action::None;
                }
                KeyCode::Char(c) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                    addition = Some(c.to_string())
                }
                _ => return Action::None,
            }
            if changed {
                field.rejected = false;
                self.error.clear();
            }
            if let Some(addition) = addition {
                if let Err(error) = field.insert(&addition) {
                    self.failure(format!("{error:#}"));
                    return Action::None;
                }
                self.error.clear();
            }
            self.reveal_focus = true;
        } else {
            match key.code {
                KeyCode::Enter => {
                    if let Some(target) = self.focus.clone() {
                        return self.activate(target);
                    }
                }
                KeyCode::Char('?') => return self.activate(Target::Button(Command::Help)),
                KeyCode::Up | KeyCode::Char('k') => self.move_focus(true),
                KeyCode::Down | KeyCode::Char('j') => self.move_focus(false),
                KeyCode::Home | KeyCode::Char('g') => {
                    self.focus = None;
                    self.move_focus(false);
                }
                KeyCode::End | KeyCode::Char('G') => {
                    self.focus = None;
                    self.move_focus(true);
                }
                KeyCode::Char('e') if matches!(self.mode, Mode::List) => self.edit_config(),
                KeyCode::Delete | KeyCode::Char('r') if matches!(self.mode, Mode::List) => {
                    return self.activate(Target::Button(Command::Forget));
                }
                _ => {}
            }
        }
        Action::None
    }

    // Wrap the actual stored text, not a clipped preview. Each row retains its painted target.
    fn rows(&self, width: u16) -> Vec<Row> {
        let mut rows = Vec::new();
        for item in self.items() {
            Self::wrap_rows(
                &mut rows,
                &item.label,
                width,
                item.target.clone(),
                None,
                false,
            );
            if let Some(index) = item.field {
                let field = &self.fields[index];
                Self::wrap_rows(
                    &mut rows,
                    &field.value,
                    width,
                    item.target.clone(),
                    (self.focus == item.target).then_some(field.cursor),
                    false,
                );
            }
            rows.push(Row {
                text: String::new(),
                target: None,
                cursor: None,
                error: false,
            });
        }
        if !self.error.is_empty() {
            Self::wrap_rows(
                &mut rows,
                &format!("Error: {}", self.error),
                width,
                None,
                None,
                true,
            );
        }
        rows
    }

    fn wrap_rows(
        rows: &mut Vec<Row>,
        text: &str,
        width: u16,
        target: Option<Target>,
        cursor: Option<usize>,
        error: bool,
    ) {
        let width = width.max(1);
        let mut row = Row {
            text: String::new(),
            target: target.clone(),
            cursor: None,
            error,
        };
        let mut column = 0;
        for (offset, c) in text.char_indices() {
            let size = if c == '\t' {
                4
            } else if c == '\r' || c == '\n' {
                0
            } else {
                c.width().unwrap_or(0) as u16
            };
            if column + size > width || (column == width && cursor == Some(offset)) {
                rows.push(row);
                row = Row {
                    text: String::new(),
                    target: target.clone(),
                    cursor: None,
                    error,
                };
                column = 0;
            }
            if cursor == Some(offset) {
                row.cursor = Some(column.min(width - 1));
            }
            if c == '\n' || c == '\r' {
                rows.push(row);
                row = Row {
                    text: String::new(),
                    target: target.clone(),
                    cursor: None,
                    error,
                };
                column = 0;
            } else if c == '\t' {
                // Tabs stay exact in the stored value; expand only their presentation.
                for _ in 0..4 {
                    if column == width {
                        rows.push(row);
                        row = Row {
                            text: String::new(),
                            target: target.clone(),
                            cursor: None,
                            error,
                        };
                        column = 0;
                    }
                    row.text.push(' ');
                    column += 1;
                }
            } else {
                row.text.push(c);
                column += size;
            }
        }
        if cursor == Some(text.len()) {
            if column >= width {
                rows.push(row);
                row = Row {
                    text: String::new(),
                    target,
                    cursor: None,
                    error,
                };
                column = 0;
            }
            row.cursor = Some(column);
        }
        rows.push(row);
    }

    pub(super) fn draw(&mut self, frame: &mut Frame<'_>) {
        let mut screen = frame.area();
        let header = screen.height.min(super::HEADER);
        screen.y += header;
        screen.height -= header;
        let width = screen.width.min(88);
        let height = screen.height.min(28);
        let area = Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        );
        let title = match self.mode {
            Mode::List => " Projects ",
            Mode::Save { .. } => " Save project ",
            Mode::Config => " Edit project ",
            Mode::Hook(_) => " Hook draft ",
            Mode::Recipe(_) => " Recipe draft ",
            Mode::Remove { .. } => " Forget project? ",
        };
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(Style::default().fg(Color::White).bg(Color::Rgb(22, 29, 43)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.hits.clear();
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let footer_height = u16::from(inner.height >= 3);
        let content = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(footer_height),
        );
        let rows = self.rows(content.width);
        self.page = usize::from(content.height);
        self.max_scroll = rows.len().saturating_sub(self.page);
        self.scroll = self.scroll.min(self.max_scroll);
        let requested = if self.reveal_error {
            rows.iter().position(|row| row.error)
        } else if self.reveal_focus {
            rows.iter()
                .position(|row| row.cursor.is_some())
                .or_else(|| {
                    rows.iter()
                        .position(|row| row.target.is_some() && row.target == self.focus)
                })
        } else {
            None
        };
        if let Some(index) = requested {
            if index < self.scroll {
                self.scroll = index;
            } else if index >= self.scroll + self.page {
                self.scroll = index.saturating_sub(self.page.saturating_sub(1));
            }
            self.scroll = self.scroll.min(self.max_scroll);
        }
        self.reveal_error = false;
        self.reveal_focus = false;
        for (offset, row) in rows.iter().skip(self.scroll).take(self.page).enumerate() {
            let rect = Rect::new(content.x, content.y + offset as u16, content.width, 1);
            let focused = row.target.is_some() && row.target == self.focus;
            let style = if row.error {
                Style::default().fg(Color::Yellow)
            } else if focused {
                Style::default()
                    .fg(Color::Cyan)
                    .bg(Color::Rgb(39, 57, 76))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            frame.render_widget(Paragraph::new(row.text.as_str()).style(style), rect);
            if let Some(target) = &row.target {
                self.hits.push((rect, target.clone()));
            }
            if let Some(column) = row.cursor {
                frame.set_cursor_position((rect.x + column, rect.y));
            }
        }
        if footer_height > 0 {
            let hint = if self.error.is_empty() {
                "Tab: focus · Enter: action · F1: Help · Esc: back · PgUp/PgDn: scroll"
            } else {
                "Error retained below · PgDn: inspect · Tab: edit · F1: Help · Esc: back"
            };
            frame.render_widget(
                Paragraph::new(hint).style(Style::default().fg(Color::Gray)),
                Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
            );
        }
        if let Some(help) = self.help.as_mut() {
            help.draw(frame, screen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn snapshot() -> Value {
        json!({"project_save_terminal":true,
            "projects":[{"id":"p","name":"existing","saved":true,"context":"/tmp","hooks":[],"recipes":{}}],
            "terminals":[{"id":"t","project_id":"p","workspace":{"cwd":"/tmp/nested"}}]})
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn preview() -> Value {
        json!({"project":"p","context":"/tmp"})
    }

    #[test]
    fn rejected_name_paste_cannot_silently_submit_an_old_or_truncated_identity() {
        for invalid in ["different\nname".to_owned(), "x".repeat(257)] {
            let mut dialog = Dialog::save(&snapshot(), "t", preview()).unwrap();
            dialog.event(&Event::Paste(invalid));
            assert!(matches!(dialog.event(&key(KeyCode::Enter)), Action::None));
            dialog.event(&Event::Key(KeyEvent::new(
                KeyCode::Char('u'),
                KeyModifiers::CONTROL,
            )));
            dialog.event(&Event::Paste("corrected-δ".into()));
            let Action::Submit(request) = dialog.event(&key(KeyCode::Enter)) else {
                panic!("deliberately corrected project identity was not admitted");
            };
            assert_eq!(request["name"], "corrected-δ");
            assert_eq!(request["expected_context"], "/tmp");
        }
    }

    #[test]
    fn save_uses_authoritative_context_and_rejects_another_projects_preview() {
        assert!(
            Dialog::save(
                &snapshot(),
                "t",
                json!({"project":"other","context":"/tmp"})
            )
            .is_err()
        );
        assert!(Dialog::save(&snapshot(), "t", json!({"project":"p"})).is_err());
        let mut dialog = Dialog::save(&snapshot(), "t", preview()).unwrap();
        let Action::Submit(request) = dialog.submit() else {
            panic!("save unavailable");
        };
        assert_eq!(request["expected_context"], "/tmp");
        assert_eq!(request["terminal"], "t");
    }

    #[test]
    fn forgetting_requires_confirmation_and_cancel_does_not_retain_the_remove_action() {
        let mut dialog = Dialog::manage(&snapshot());
        assert!(matches!(
            dialog.event(&key(KeyCode::Char('r'))),
            Action::None
        ));
        assert!(matches!(dialog.event(&key(KeyCode::Esc)), Action::None));
        let Action::Submit(open) = dialog.event(&key(KeyCode::Enter)) else {
            panic!("cancelled removal captured the next list action");
        };
        assert_eq!(open["op"], "project-open");
        dialog.event(&key(KeyCode::Char('r')));
        // Confirmation starts on Cancel, not the destructive action.
        assert!(matches!(dialog.event(&key(KeyCode::Enter)), Action::None));
        dialog.event(&key(KeyCode::Char('r')));
        let Action::Submit(remove) = dialog.activate(Target::Button(Command::Submit)) else {
            panic!("explicit removal confirmation was not admitted");
        };
        assert_eq!(remove["op"], "project-remove");
        dialog.failure("write conflict".into());
        assert!(matches!(dialog.event(&key(KeyCode::Esc)), Action::None));
        assert!(matches!(dialog.mode, Mode::List));
    }

    #[test]
    fn structured_edits_preserve_hook_order_recipe_values_and_atomic_patch() {
        let mut snapshot = snapshot();
        snapshot["projects"][0]["hooks"] = json!([
            {"source":"first\n# exact", "timeout_ms":500},
            {"source":"second", "timeout_ms":1234}]);
        snapshot["projects"][0]["recipes"] = json!({"keep":"  original\n\tvalue  ", "edit":"old"});
        let mut dialog = Dialog::manage(&snapshot);
        dialog.edit_config();
        dialog.fields[0] = Field::new("renamed".into(), false);
        dialog.open_hook(Some(1));
        dialog.fields[0] = Field::new("updated\njk?".into(), true);
        assert!(matches!(dialog.submit(), Action::None));
        dialog.open_hook(None);
        dialog.fields[0] = Field::new("third".into(), true);
        dialog.submit();
        dialog.open_recipe(Some("edit".into()));
        dialog.fields[0] = Field::new("edited".into(), false);
        dialog.fields[1] = Field::new("new\n\tcommand".into(), true);
        dialog.submit();
        let Action::Submit(request) = dialog.submit() else {
            panic!("complete patch unavailable");
        };
        assert_eq!(request["op"], "project-config");
        assert_eq!(request["config"]["name"], "renamed");
        assert_eq!(
            request["config"]["hooks"],
            json!([
            {"source":"first\n# exact", "timeout_ms":500},
            {"source":"updated\njk?", "timeout_ms":1234},
            {"source":"third", "timeout_ms":30000}])
        );
        assert_eq!(
            request["config"]["recipes"],
            json!({"keep":"  original\n\tvalue  ", "edited":"new\n\tcommand"})
        );
        assert!(request["config"].get("context").is_none());
        dialog.failure("atomic write rejected".into());
        let Action::Submit(retry) = dialog.submit() else {
            panic!("rejected draft lost");
        };
        assert_eq!(retry, request);
    }

    #[test]
    fn entry_cancel_remove_and_rejected_fields_never_change_unrelated_values() {
        let mut snapshot = snapshot();
        snapshot["projects"][0]["hooks"] =
            json!([{"source":"one","timeout_ms":500},{"source":"two","timeout_ms":600}]);
        snapshot["projects"][0]["recipes"] = json!({"one":"first", "two":"second"});
        let mut dialog = Dialog::manage(&snapshot);
        dialog.edit_config();
        dialog.open_hook(Some(0));
        dialog.fields[0] = Field::new("cancelled".into(), true);
        dialog.back();
        assert_eq!(dialog.draft.as_ref().unwrap().hooks[0]["source"], "one");
        dialog.open_hook(Some(0));
        dialog.activate(Target::Button(Command::RemoveEntry));
        dialog.open_recipe(Some("one".into()));
        dialog.activate(Target::Button(Command::RemoveEntry));
        dialog.open_recipe(Some("two".into()));
        dialog.event(&Event::Paste("\0".into()));
        dialog.focus = Some(Target::Field(1));
        dialog.event(&Event::Paste("jk?".into()));
        assert!(matches!(dialog.submit(), Action::None));
        assert!(matches!(dialog.mode, Mode::Recipe(_)));
        dialog.back();
        let Action::Submit(request) = dialog.submit() else {
            panic!("draft unavailable");
        };
        assert_eq!(
            request["config"]["hooks"],
            json!([{"source":"two","timeout_ms":600}])
        );
        assert_eq!(request["config"]["recipes"], json!({"two":"second"}));
    }

    #[test]
    fn rejected_project_name_remains_rejected_across_entry_editors() {
        let mut dialog = Dialog::manage(&snapshot());
        dialog.edit_config();
        dialog.event(&Event::Paste("invalid\nname".into()));
        dialog.open_hook(None);
        dialog.fields[0] = Field::new("valid hook".into(), true);
        dialog.submit();
        assert!(matches!(dialog.submit(), Action::None));
        assert!(dialog.fields[0].rejected);
        dialog.event(&Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        dialog.event(&Event::Paste("corrected".into()));
        let Action::Submit(request) = dialog.submit() else {
            panic!("corrected name not admitted");
        };
        assert_eq!(request["config"]["name"], "corrected");
        assert_eq!(request["config"]["hooks"][0]["source"], "valid hook");
    }

    #[test]
    fn text_letters_are_literal_and_help_returns_to_the_same_draft() {
        let mut dialog = Dialog::manage(&snapshot());
        dialog.edit_config();
        dialog.open_hook(None);
        for c in "jk?".chars() {
            dialog.event(&key(KeyCode::Char(c)));
        }
        assert_eq!(dialog.fields[0].value, "jk?");
        assert!(dialog.help.is_none());
        dialog.activate(Target::Button(Command::Help));
        dialog.event(&key(KeyCode::Esc));
        assert!(dialog.help.is_none());
        assert!(matches!(dialog.mode, Mode::Hook(None)));
        assert_eq!(dialog.fields[0].value, "jk?");
    }
}
