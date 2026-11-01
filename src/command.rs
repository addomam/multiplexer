use anyhow::{Result, bail};
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, Serializer};
use serde_json::{Map, Value};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    Shell,
    Command(String),
    Omp,
    Recipe(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Open {
    launch: Launch,
    fields: Map<String, Value>,
}

impl Open {
    fn from_fields(mut fields: Map<String, Value>) -> Result<Self, String> {
        let command = take_string(&mut fields, "command");
        let omp = match fields.remove("omp") {
            Some(Value::Bool(true)) => true,
            Some(value) => {
                fields.insert("omp".to_owned(), value);
                false
            }
            None => false,
        };
        let recipe = take_string(&mut fields, "recipe");
        let modes = u8::from(command.is_some()) + u8::from(omp) + u8::from(recipe.is_some());
        if modes > 1 {
            return Err("mixed-open: command, omp, and recipe are mutually exclusive".to_owned());
        }
        let launch = if let Some(command) = command {
            Launch::Command(command)
        } else if omp {
            Launch::Omp
        } else if let Some(recipe) = recipe {
            Launch::Recipe(recipe)
        } else {
            Launch::Shell
        };
        Ok(Self { launch, fields })
    }

    fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }

    fn fields_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.fields
    }

    fn to_fields(&self) -> Map<String, Value> {
        let mut fields = self.fields.clone();
        match &self.launch {
            Launch::Shell => {}
            Launch::Command(command) => {
                fields.insert("command".to_owned(), Value::String(command.clone()));
            }
            Launch::Omp => {
                fields.insert("omp".to_owned(), Value::Bool(true));
            }
            Launch::Recipe(recipe) => {
                fields.insert("recipe".to_owned(), Value::String(recipe.clone()));
            }
        }
        fields
    }
}

fn take_string(fields: &mut Map<String, Value>, key: &str) -> Option<String> {
    match fields.remove(key) {
        Some(Value::String(value)) => Some(value),
        Some(value) => {
            fields.insert(key.to_owned(), value);
            None
        }
        None => None,
    }
}

macro_rules! command_ops {
    ($($name:ident => $op:literal),* $(,)?) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum Command {
            Open(Open),
            $($name(Map<String, Value>),)*
        }

        impl Command {
            pub fn op(&self) -> &'static str {
                match self {
                    Self::Open(_) => "open",
                    $(Self::$name(_) => $op,)*
                }
            }

            fn body(&self) -> &Map<String, Value> {
                match self {
                    Self::Open(open) => open.fields(),
                    $(Self::$name(fields) => fields,)*
                }
            }

            pub fn get(&self, key: &str) -> Option<&Value> {
                self.body().get(key)
            }

            pub fn body_mut(&mut self) -> &mut Map<String, Value> {
                match self {
                    Self::Open(open) => open.fields_mut(),
                    $(Self::$name(fields) => fields,)*
                }
            }

            pub fn into_fields(self) -> Map<String, Value> {
                match self {
                    Self::Open(open) => open.to_fields(),
                    $(Self::$name(fields) => fields,)*
                }
            }

            pub fn open(launch: Launch, fields: Map<String, Value>) -> Self {
                Self::Open(Open { launch, fields })
            }

            pub fn to_value(&self) -> Value {
                let mut fields = match self {
                    Self::Open(open) => open.to_fields(),
                    $(Self::$name(fields) => fields.clone(),)*
                };
                fields.insert("op".to_owned(), Value::String(self.op().to_owned()));
                Value::Object(fields)
            }

            pub fn new(value: Value) -> Result<Self> {
                let Value::Object(object) = value else {
                    bail!("invalid-request: object required");
                };
                Self::from_map(object).map_err(anyhow::Error::msg)
            }

            fn from_map(mut object: Map<String, Value>) -> Result<Self, String> {
                object.remove("version");
                object.remove("persistent");
                let op = match object.remove("op") {
                    Some(Value::String(op)) => op,
                    _ => return Err("invalid-request: op required".to_owned()),
                };
                if op == "open" {
                    return Open::from_fields(object).map(Self::Open);
                }
                Ok(match op.as_str() {
                    $($op => Self::$name(object),)*
                    _ => return Err(format!("invalid-request: unknown operation {op}")),
                })
            }

            pub fn snapshot() -> Self {
                Self::Snapshot(Map::new())
            }

            pub fn screen(
                terminal: &str,
                offset: usize,
                limit: u16,
                since: Option<u64>,
                row_updates: bool,
            ) -> Self {
                let mut fields = Map::new();
                fields.insert("terminal".to_owned(), Value::String(terminal.to_owned()));
                fields.insert("offset".to_owned(), Value::from(offset));
                fields.insert("limit".to_owned(), Value::from(limit));
                fields.insert("since".to_owned(), json_option(since));
                fields.insert("row_updates".to_owned(), Value::Bool(row_updates));
                Self::Screen(fields)
            }

            pub fn input_bytes(terminal: &str, control: &Value, bytes: &[u8]) -> Self {
                let mut fields = Map::new();
                fields.insert("terminal".to_owned(), Value::String(terminal.to_owned()));
                fields.insert("control".to_owned(), control.clone());
                fields.insert(
                    "bytes".to_owned(),
                    Value::Array(bytes.iter().copied().map(Value::from).collect()),
                );
                Self::Input(fields)
            }

            pub fn input_paste(terminal: &str, control: &Value, text: &str) -> Self {
                let mut fields = Map::new();
                fields.insert("terminal".to_owned(), Value::String(terminal.to_owned()));
                fields.insert("control".to_owned(), control.clone());
                fields.insert("text".to_owned(), Value::String(text.to_owned()));
                fields.insert("paste".to_owned(), Value::Bool(true));
                Self::Input(fields)
            }
        }
    };
}

pub fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn json_option(since: Option<u64>) -> Value {
    match since {
        Some(since) => Value::from(since),
        None => Value::Null,
    }
}

command_ops! {
    AcpProfiles => "acp-profiles",
    AcpOpen => "acp-open",
    AcpInspect => "acp-inspect",
    AcpPrompt => "acp-prompt",
    AcpAuthenticate => "acp-authenticate",
    AcpAnswer => "acp-answer",
    AcpPermission => "acp-permission",
    AcpInterrupt => "acp-interrupt",
    AcpClose => "acp-close",
    ControllerCreate => "controller-create",
    OwnershipClaim => "ownership-claim",
    ControlHandle => "control-handle",
    Snapshot => "snapshot",
    List => "list",
    AgentOpen => "agent-open",
    AgentInspect => "agent-inspect",
    AgentPrompt => "agent-prompt",
    AgentAnswer => "agent-answer",
    AgentPermission => "agent-permission",
    AgentInterrupt => "agent-interrupt",
    State => "state",
    Screen => "screen",
    Resize => "resize",
    Input => "input",
    Execute => "execute",
    Report => "report",
    Name => "name",
    Reparent => "reparent",
    Demote => "demote",
    Promote => "promote",
    Close => "close",
    Remove => "remove",
    Events => "events",
    Wait => "wait",
    ProjectList => "project-list",
    ProjectRemove => "project-remove",
    ProjectSave => "project-save",
    ProjectSavePreview => "project-save-preview",
    ProjectNew => "project-new",
    ProjectOpen => "project-open",
    ProjectConfig => "project-config",
    Config => "config",
    Detach => "detach",
    Shutdown => "shutdown",
}

impl std::ops::Index<&str> for Command {
    type Output = Value;

    fn index(&self, index: &str) -> &Value {
        self.get(index)
            .unwrap_or_else(|| panic!("command {} has no {index}", self.op()))
    }
}

impl Serialize for Command {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Command {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CommandVisitor;

        impl<'de> Visitor<'de> for CommandVisitor {
            type Value = Command;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a version-1 command object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut object = Map::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    object.insert(key, value);
                }
                Command::from_map(object).map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_map(CommandVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_omp_together_with_recipe() {
        for body in [
            r#"{"op":"open","omp":true,"recipe":"check"}"#,
            r#"{"op":"open","omp":true,"command":"printf one"}"#,
            r#"{"op":"open","recipe":"check","command":"printf one"}"#,
            r#"{"op":"open","omp":true,"recipe":"check","command":"printf one"}"#,
            r#"{"op":"open","omp":true,"recipe":""}"#,
            r#"{"op":"open","command":"","recipe":"check"}"#,
        ] {
            let error = serde_json::from_str::<Command>(body)
                .expect_err(body)
                .to_string();
            assert!(
                error.starts_with("mixed-open:"),
                "{body} deserialized as {error}"
            );
        }
    }

    #[test]
    fn open_accepts_a_blank_shell_and_one_launch_mode() {
        for body in [
            r#"{"op":"open"}"#,
            r#"{"op":"open","omp":false}"#,
            r#"{"op":"open","omp":true}"#,
            r#"{"op":"open","recipe":"check"}"#,
            r#"{"op":"open","command":"printf one"}"#,
            r#"{"op":"open","command":"","omp":false}"#,
        ] {
            serde_json::from_str::<Command>(body).unwrap_or_else(|error| panic!("{body}: {error}"));
        }
        let shell = serde_json::from_str::<Command>(r#"{"op":"open"}"#).unwrap();
        assert_eq!(shell.to_value()["op"], "open");
        assert!(shell.to_value().get("command").is_none());
        assert!(shell.to_value().get("omp").is_none());
        assert!(shell.to_value().get("recipe").is_none());
    }

    #[test]
    fn unknown_op_fails_decode() {
        let error = serde_json::from_str::<Command>(r#"{"version":1,"op":"nope"}"#)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("invalid-request:"), "{error}");
    }
}
