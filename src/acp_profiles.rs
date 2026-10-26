use crate::projects::AcpProfile;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const BUILTINS: [&str; 3] = ["omp", "codex", "grok"];
pub(crate) const OPTIONS_JSON_BYTES: usize = 64 * 1024;

/// Keep explicit option identities lossless and reject duplicate JSON keys.
pub(crate) fn parse_options_json(text: &str) -> Result<serde_json::Map<String, Value>> {
    ensure!(
        text.len() <= OPTIONS_JSON_BYTES,
        "Options JSON exceeds64KiB"
    );
    struct Options;
    impl<'de> serde::de::Visitor<'de> for Options {
        type Value = serde_json::Map<String, Value>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an object mapping option IDs to choice IDs")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut input: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut options = serde_json::Map::new();
            while let Some(id) = input.next_key::<String>()? {
                if id.is_empty() {
                    return Err(serde::de::Error::custom("Option ID cannot be empty"));
                }
                let serde_json::map::Entry::Vacant(entry) = options.entry(id) else {
                    return Err(serde::de::Error::custom("Duplicate option ID"));
                };
                let value = input.next_value::<String>()?;
                if value.is_empty() {
                    return Err(serde::de::Error::custom("Option value cannot be empty"));
                }
                entry.insert(Value::String(value));
            }
            Ok(options)
        }
    }
    let mut parser = serde_json::Deserializer::from_str(text);
    let options = serde::de::Deserializer::deserialize_map(&mut parser, Options)
        .context("Invalid Options JSON object")?;
    parser
        .end()
        .context("Invalid Options JSON trailing content")?;
    Ok(options)
}

fn builtin(id: &str) -> Option<AcpProfile> {
    let (label, command): (&str, &[&str]) = match id {
        "omp" => ("OMP", &["omp", "acp"]),
        "codex" => ("Codex", &["codex-acp"]),
        "grok" => ("Grok", &["grok", "--no-auto-update", "agent", "stdio"]),
        _ => return None,
    };
    Some(AcpProfile {
        label: label.into(),
        command: command.iter().map(|arg| (*arg).to_owned()).collect(),
        defaults: json!({}),
    })
}

pub(crate) fn validate_profiles(profiles: &BTreeMap<String, AcpProfile>) -> Result<()> {
    ensure!(
        profiles.len() <= 16,
        "at most16 ACP profiles may be configured"
    );
    for (id, profile) in profiles {
        ensure!(
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
            "ACP profile ID must contain1..64 ASCII letters, digits, hyphens or underscores"
        );
        ensure!(
            !profile.label.is_empty()
                && profile.label.len() <= 128
                && profile.label.chars().all(|c| !c.is_control()
                    && !matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}'
                        | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
            "ACP profile label must be a safe1..128 byte label"
        );
        ensure!(
            !profile.command.is_empty()
                && profile.command.len() <= 128
                && !profile.command[0].is_empty()
                && profile.command.iter().map(String::len).sum::<usize>() <= 64 * 1024
                && profile.command.iter().all(|arg| !arg.contains('\0')),
            "ACP profile command must be bounded nonempty argv"
        );
        let executable = Path::new(&profile.command[0]);
        ensure!(
            executable.is_absolute() || executable.components().count() == 1,
            "ACP profile executable must be absolute or a bare PATH command"
        );
        crate::acp::validate_requested_settings(&profile.defaults)?;
    }
    Ok(())
}

pub(crate) fn select(profiles: &BTreeMap<String, AcpProfile>, id: &str) -> Result<AcpProfile> {
    profiles
        .get(id)
        .cloned()
        .or_else(|| builtin(id))
        .context("invalid-request: unknown ACP agent profile")
}

fn executable(command: &str) -> Option<PathBuf> {
    let usable = |path: PathBuf| {
        let metadata = path.metadata().ok()?;
        (metadata.is_file()
            && metadata.permissions().mode() & 0o111 != 0
            && path.to_str().is_some())
        .then_some(path)
    };
    let path = Path::new(command);
    if path.is_absolute() {
        return usable(path.to_owned());
    }
    if path.components().count() != 1 {
        return None;
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|directory| directory.is_absolute())
        .find_map(|directory| usable(directory.join(command)))
}

pub(crate) fn list(mut profiles: BTreeMap<String, AcpProfile>) -> Value {
    for id in BUILTINS {
        profiles
            .entry(id.into())
            .or_insert_with(|| builtin(id).expect("known builtin"));
    }
    json!({"profiles": profiles.into_iter().map(|(id, profile)| {
        let available = executable(&profile.command[0]).is_some();
        json!({"id":id,"label":profile.label,"available":available,"defaults":profile.defaults})
    }).collect::<Vec<_>>()})
}

pub(crate) fn prepare(
    profile: AcpProfile,
    requested: Option<&Value>,
) -> Result<(Vec<String>, Value)> {
    let mut settings = profile.defaults;
    let defaults = settings
        .as_object_mut()
        .context("invalid-request: profile defaults must be an object")?;
    if let Some(requested) = requested {
        crate::acp::validate_requested_settings(requested)?;
        for (key, value) in requested.as_object().expect("validated settings") {
            if key == "options" {
                let options = defaults
                    .entry("options")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .context("invalid-request: profile options must be an object")?;
                options.extend(
                    value
                        .as_object()
                        .expect("validated options")
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            } else {
                defaults.insert(key.clone(), value.clone());
            }
        }
    }
    crate::acp::validate_requested_settings(&settings)?;
    let mut command = profile.command;
    let name = command
        .first()
        .context("invalid-request: ACP command argv required")?;
    let resolved =
        executable(name).context("invalid-request: ACP adapter executable is unavailable")?;
    command[0] = resolved
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("invalid-request: ACP adapter path is not UTF-8"))?;
    Ok((command, settings))
}
