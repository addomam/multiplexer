//! Capability-backed startup settings on the original ACP connection.
use super::*;

const CONFIG_CHOICES: usize = 4096;
type Parsed<T> = std::result::Result<T, &'static str>;

pub(super) fn validate_requested(requested: &Value) -> Result<()> {
    closed(requested, &["model", "mode", "effort", "options"], &[])?;
    ensure!(
        value_bytes(requested) <= LOCAL_CONTENT,
        "invalid-request: ACP settings budget"
    );
    for key in ["model", "mode", "effort"] {
        if let Some(value) = requested.get(key) {
            ensure!(
                value.as_str().is_some_and(valid_id),
                "invalid-request: ACP setting identity"
            );
        }
    }
    if let Some(options) = requested.get("options") {
        let options = options
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("invalid-request: ACP setting options object"))?;
        ensure!(
            options.len() <= FIELDS,
            "invalid-request: ACP setting option budget"
        );
        for (id, value) in options {
            ensure!(
                valid_id(id) && value.as_str().is_some_and(valid_id),
                "invalid-request: ACP setting option identity or value"
            );
        }
    }
    Ok(())
}

#[derive(Default)]
struct Choices {
    current: Option<String>,
    offered: Vec<(String, String)>,
}
impl Choices {
    fn projection(&self) -> Value {
        json!({"current":self.current,"offered":self.offered.iter().map(|(id,name)| json!({"id":id,"name":name})).collect::<Vec<_>>(),
            "state":if self.current.is_some() {"reported"} else {"unreported"}})
    }
    fn offers(&self, value: &str) -> bool {
        self.offered.iter().any(|(id, _)| id == value)
    }
}

struct ConfigOption {
    id: String,
    name: String,
    category: Option<String>,
    kind: String,
    current: Value,
    offered: Vec<(String, String)>,
}
impl ConfigOption {
    fn projection(&self) -> Value {
        json!({"id":self.id,"name":self.name,"category":self.category,"type":self.kind,"current":self.current,
            "offered":self.offered.iter().map(|(id,name)| json!({"id":id,"name":name})).collect::<Vec<_>>()})
    }
    fn choice_projection(&self) -> Value {
        json!({"current":self.current.as_str(),"offered":self.offered.iter().map(|(id,name)| json!({"id":id,"name":name})).collect::<Vec<_>>(),
            "state":if self.current.is_string() {"reported"} else {"unreported"}})
    }
}

enum Requested {
    Model(String),
    Mode(String),
    Effort(String),
    Option(String, String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyModelSelection<'a> {
    session_id: &'a str,
    model_id: &'a str,
}

enum Setting {
    Config {
        id: String,
        value: String,
        key: String,
    },
    Mode {
        value: String,
        key: String,
    },
    Model(String),
}
impl Setting {
    fn key(&self) -> &str {
        match self {
            Self::Config { key, .. } | Self::Mode { key, .. } => key,
            Self::Model(_) => "category:model",
        }
    }
    fn value(&self) -> &str {
        match self {
            Self::Config { value, .. } | Self::Mode { value, .. } | Self::Model(value) => value,
        }
    }
    fn request(&self, session: &str) -> Result<acp::ClientRequest> {
        Ok(match self {
            Self::Config { id, value, .. } => acp::ClientRequest::SetSessionConfigOptionRequest(
                acp::SetSessionConfigOptionRequest::new(
                    session.to_owned(),
                    id.clone(),
                    value.as_str(),
                ),
            ),
            Self::Mode { value, .. } => acp::ClientRequest::SetSessionModeRequest(
                acp::SetSessionModeRequest::new(session.to_owned(), value.clone()),
            ),
            // Only advertised legacy models use this original method. Never
            // retry a rejected modern configuration RPC through another path.
            Self::Model(value) => acp::ClientRequest::ExtMethodRequest(acp::ExtRequest::new(
                "session/set_model",
                serde_json::value::to_raw_value(&LegacyModelSelection {
                    session_id: session,
                    model_id: value,
                })?
                .into(),
            )),
        })
    }
}

pub(super) struct Configuration {
    requested: Value,
    models: Choices,
    modes: Choices,
    options: Vec<ConfigOption>,
    pending: VecDeque<Requested>,
    active: Option<Setting>,
    applied: BTreeMap<String, String>,
    phase: &'static str,
    failure: Option<&'static str>,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            requested: json!({}),
            models: Choices::default(),
            modes: Choices::default(),
            options: Vec::new(),
            pending: VecDeque::new(),
            active: None,
            applied: BTreeMap::new(),
            phase: "pending",
            failure: None,
        }
    }
}
impl Configuration {
    pub(super) fn new(requested: Value) -> Result<Self> {
        validate_requested(&requested)?;
        let mut pending = VecDeque::new();
        for key in ["model", "mode", "effort"] {
            if let Some(value) = requested.get(key) {
                let value = value.as_str().expect("validated setting").to_owned();
                pending.push_back(match key {
                    "model" => Requested::Model(value),
                    "mode" => Requested::Mode(value),
                    _ => Requested::Effort(value),
                });
            }
        }
        if let Some(options) = requested.get("options") {
            for (id, value) in options.as_object().expect("validated options") {
                pending.push_back(Requested::Option(
                    id.clone(),
                    value.as_str().expect("validated option").to_owned(),
                ));
            }
        }
        Ok(Self {
            requested,
            pending,
            models: Choices::default(),
            modes: Choices::default(),
            options: Vec::new(),
            active: None,
            applied: BTreeMap::new(),
            phase: "pending",
            failure: None,
        })
    }
    fn category(&self, category: &str) -> Parsed<Option<&ConfigOption>> {
        let mut options = self
            .options
            .iter()
            .filter(|option| option.category.as_deref() == Some(category));
        let option = options.next();
        if options.next().is_some() {
            return Err(match category {
                "model" => "ambiguous-model",
                "mode" => "ambiguous-mode",
                _ => "ambiguous-effort",
            });
        }
        Ok(option)
    }
    pub(super) fn projection(&self) -> Value {
        let model = self.category("model").ok().flatten();
        let mode = self.category("mode").ok().flatten();
        let effort = self.category("thought_level").ok().flatten();
        let mut effort_projection = effort.map_or_else(
            || Choices::default().projection(),
            ConfigOption::choice_projection,
        );
        effort_projection["option_id"] = effort.map_or(Value::Null, |option| json!(option.id));
        json!({"requested":self.requested,"reported":{
            "model":model.map_or_else(|| self.models.projection(), ConfigOption::choice_projection),
            "mode":if self.modes.current.is_some() { self.modes.projection() } else { mode.map_or_else(|| Choices::default().projection(), ConfigOption::choice_projection) },
            "effort":effort_projection,"options":self.options.iter().map(ConfigOption::projection).collect::<Vec<_>>()},
            "state":self.phase,"failure":self.failure})
    }
    pub(super) fn fail(&mut self, code: &'static str) {
        self.phase = "failed";
        self.failure = Some(code);
        self.pending.clear();
        self.active = None;
    }
    fn error(&mut self, code: &'static str) -> anyhow::Error {
        self.fail(code);
        anyhow::anyhow!("unsupported: ACP startup settings: {code}")
    }
    pub(super) fn ended(&mut self) {
        if matches!(self.phase, "pending" | "applying") {
            self.fail("startup-ended");
        }
    }
    pub(super) fn session(&mut self, value: &Value) -> Result<()> {
        let parsed = (|| {
            let modes = optional(value, "modes")
                .map(|value| parse_choices(value, "currentModeId", "availableModes", "id"))
                .transpose()?;
            let models = optional(value, "models")
                .map(|value| parse_choices(value, "currentModelId", "availableModels", "modelId"))
                .transpose()?;
            let options = optional(value, "configOptions")
                .map(parse_options)
                .transpose()?;
            Ok::<_, &'static str>((modes, models, options))
        })();
        let (modes, models, options) = parsed.map_err(|code| self.error(code))?;
        self.modes = modes.unwrap_or_default();
        self.models = models.unwrap_or_default();
        self.options = options.unwrap_or_default();
        Ok(())
    }
    pub(super) fn update(&mut self, update: &Value) -> Result<()> {
        match update["sessionUpdate"].as_str() {
            Some("config_option_update") => {
                self.options =
                    parse_options(&update["configOptions"]).map_err(|code| self.error(code))?;
                if let Ok(Some(mode)) = self.category("mode") {
                    self.modes.current = mode.current.as_str().map(str::to_owned);
                }
            }
            Some("current_mode_update") => {
                let current =
                    setting_id(&update["currentModeId"]).map_err(|code| self.error(code))?;
                self.mode_current(current);
            }
            _ => {}
        }
        if matches!(self.phase, "applied" | "changed") {
            self.phase = if self
                .applied
                .iter()
                .any(|(key, expected)| self.current(key) != Some(expected.as_str()))
            {
                "changed"
            } else {
                "applied"
            };
        }
        Ok(())
    }
    fn config(
        &self,
        option: &ConfigOption,
        value: String,
        unsupported: &'static str,
    ) -> Parsed<Setting> {
        if option.kind != "select" || !option.offered.iter().any(|(id, _)| id == &value) {
            return Err(unsupported);
        }
        let key = format!("option:{}", option.id);
        Ok(Setting::Config {
            id: option.id.clone(),
            value,
            key,
        })
    }
    fn resolve(&self, requested: Requested) -> Parsed<Setting> {
        match requested {
            Requested::Model(value) => {
                if let Some(option) = self.category("model")? {
                    self.config(option, value, "unsupported-model")
                } else if self.models.offers(&value) {
                    Ok(Setting::Model(value))
                } else {
                    Err("unsupported-model")
                }
            }
            Requested::Mode(value) => {
                if self.modes.offers(&value) {
                    let key = self.category("mode").ok().flatten().map_or_else(
                        || "category:mode".to_owned(),
                        |option| format!("option:{}", option.id),
                    );
                    Ok(Setting::Mode { value, key })
                } else if let Some(option) = self.category("mode")? {
                    self.config(option, value, "unsupported-mode")
                } else {
                    Err("unsupported-mode")
                }
            }
            Requested::Effort(value) => self.config(
                self.category("thought_level")?
                    .ok_or("unsupported-effort")?,
                value,
                "unsupported-effort",
            ),
            Requested::Option(id, value) => self.config(
                self.options
                    .iter()
                    .find(|option| option.id == id)
                    .ok_or("unsupported-option")?,
                value,
                "unsupported-option",
            ),
        }
    }
    pub(super) fn next(&mut self, session: &str) -> Result<Option<acp::ClientRequest>> {
        ensure!(
            self.active.is_none(),
            "transport-failed: ACP setting reply pending"
        );
        while let Some(requested) = self.pending.pop_front() {
            let setting = self.resolve(requested).map_err(|code| self.error(code))?;
            if let Some(previous) = self.applied.get(setting.key()) {
                if previous != setting.value() {
                    return Err(self.error("conflicting-settings"));
                }
                continue;
            }
            let request = setting.request(session)?;
            self.phase = "applying";
            self.active = Some(setting);
            return Ok(Some(request));
        }
        let mismatched = self
            .applied
            .iter()
            .any(|(key, expected)| self.current(key) != Some(expected.as_str()));
        if mismatched {
            return Err(self.error("setting-mismatch"));
        }
        self.phase = if self.applied.is_empty() {
            "agent-default"
        } else {
            "applied"
        };
        Ok(None)
    }
    fn current(&self, key: &str) -> Option<&str> {
        match key {
            "category:model" => self
                .category("model")
                .ok()
                .flatten()
                .and_then(|option| option.current.as_str())
                .or(self.models.current.as_deref()),
            "category:mode" => self.modes.current.as_deref().or_else(|| {
                self.category("mode")
                    .ok()
                    .flatten()
                    .and_then(|option| option.current.as_str())
            }),
            _ => key
                .strip_prefix("option:")
                .and_then(|id| self.options.iter().find(|option| option.id == id))
                .and_then(|option| option.current.as_str()),
        }
    }
    pub(super) fn acknowledge(&mut self, reply: &Value) -> Result<()> {
        let setting = self
            .active
            .take()
            .ok_or_else(|| anyhow::anyhow!("transport-failed: ACP setting not retained"))?;
        // The pinned legacy mode response permits an empty null acknowledgement;
        // config setters still require their full value-confirming response.
        if !(reply.is_object() || reply.is_null() && matches!(&setting, Setting::Mode { .. })) {
            return Err(self.error("setting-unconfirmed"));
        }
        match &setting {
            Setting::Config { id, value, .. } => {
                let Some(raw) = reply.get("configOptions") else {
                    return Err(self.error("setting-unconfirmed"));
                };
                self.options = parse_options(raw).map_err(|code| self.error(code))?;
                let Some(option) = self.options.iter().find(|option| &option.id == id) else {
                    return Err(self.error("setting-unconfirmed"));
                };
                if option.current.as_str() != Some(value.as_str()) {
                    return Err(self.error("setting-mismatch"));
                }
                if let Ok(Some(mode)) = self.category("mode") {
                    self.modes.current = mode.current.as_str().map(str::to_owned);
                }
            }
            // These setters return empty success acknowledgements of the exact
            // selected identity; optional duplicate notifications are not needed.
            Setting::Mode { value, .. } => self.mode_current(value.clone()),
            Setting::Model(value) => self.models.current = Some(value.clone()),
        }
        self.applied
            .insert(setting.key().to_owned(), setting.value().to_owned());
        Ok(())
    }
    fn mode_current(&mut self, current: String) {
        let mut options = self
            .options
            .iter_mut()
            .filter(|option| option.category.as_deref() == Some("mode"));
        if let Some(option) = options.next()
            && options.next().is_none()
        {
            option.current = json!(current);
        }
        self.modes.current = Some(current);
    }
}

fn setting_id(value: &Value) -> Parsed<String> {
    value
        .as_str()
        .filter(|value| valid_id(value))
        .map(str::to_owned)
        .ok_or("invalid-settings-metadata")
}
fn parse_choices(value: &Value, current: &str, offered: &str, key: &str) -> Parsed<Choices> {
    let current = setting_id(&value[current])?;
    let values = value[offered]
        .as_array()
        .ok_or("invalid-settings-metadata")?;
    if values.len() > CONFIG_CHOICES {
        return Err("settings-metadata-budget");
    }
    let mut seen = BTreeSet::new();
    let mut offered = Vec::with_capacity(values.len());
    for value in values {
        let id = setting_id(&value[key])?;
        if !seen.insert(id.clone()) {
            return Err("invalid-settings-metadata");
        }
        let name = value["name"].as_str().ok_or("invalid-settings-metadata")?;
        offered.push((id, safe_label(name)));
    }
    Ok(Choices {
        current: Some(current),
        offered,
    })
}
fn parse_options(value: &Value) -> Parsed<Vec<ConfigOption>> {
    let values = value.as_array().ok_or("invalid-settings-metadata")?;
    if values.len() > FIELDS || value_bytes(value) > FRAME {
        return Err("settings-metadata-budget");
    }
    let mut seen = BTreeSet::new();
    let mut total = 0;
    let mut options = Vec::with_capacity(values.len());
    for value in values {
        let id = setting_id(&value["id"])?;
        if !seen.insert(id.clone()) {
            return Err("invalid-settings-metadata");
        }
        let name = safe_label(value["name"].as_str().ok_or("invalid-settings-metadata")?);
        let category = optional(value, "category")
            .map(|value| {
                value
                    .as_str()
                    .filter(|value| valid_id(value))
                    .map(str::to_owned)
                    .ok_or("invalid-settings-metadata")
            })
            .transpose()?;
        let kind = value["type"].as_str().ok_or("invalid-settings-metadata")?;
        let mut offered = Vec::new();
        let current = match kind {
            "select" => {
                let current = setting_id(&value["currentValue"])?;
                let groups = value["options"]
                    .as_array()
                    .ok_or("invalid-settings-metadata")?;
                if groups.len() > CONFIG_CHOICES {
                    return Err("settings-metadata-budget");
                }
                let mut ids = BTreeSet::new();
                for group in groups {
                    let choices = if group.get("group").is_some() {
                        let _ = setting_id(&group["group"])?;
                        group["options"]
                            .as_array()
                            .ok_or("invalid-settings-metadata")?
                            .as_slice()
                    } else {
                        std::slice::from_ref(group)
                    };
                    total += choices.len();
                    if total > CONFIG_CHOICES {
                        return Err("settings-metadata-budget");
                    }
                    for choice in choices {
                        let choice_id = setting_id(&choice["value"])?;
                        if !ids.insert(choice_id.clone()) {
                            return Err("invalid-settings-metadata");
                        }
                        let name =
                            safe_label(choice["name"].as_str().ok_or("invalid-settings-metadata")?);
                        offered.push((choice_id, name));
                    }
                }
                json!(current)
            }
            "boolean" => json!(
                value["currentValue"]
                    .as_bool()
                    .ok_or("invalid-settings-metadata")?
            ),
            // Future kinds are not coerced into selectable string IDs.
            _ => {
                if !valid_id(kind) {
                    return Err("invalid-settings-metadata");
                }
                Value::Null
            }
        };
        options.push(ConfigOption {
            id,
            name,
            category,
            kind: kind.to_owned(),
            current,
            offered,
        });
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(model: &str, effort: &str) -> Value {
        json!([
            {"id":"engine","name":"Model","category":"model","type":"select","currentValue":model,
                "options":[{"value":"fixture/cheap","name":"Cheap"},{"value":"fixture/other","name":"Other"}]},
            {"id":"depth","name":"Thinking","category":"thought_level","type":"select","currentValue":effort,
                "options":[{"group":"levels","name":"Levels","options":[
                    {"value":"off","name":"Off"},{"value":"low","name":"Low"}]}]}
        ])
    }

    #[test]
    fn e06s01_legacy_models_use_only_the_advertised_original_setter() {
        let mut configuration = Configuration::new(json!({"model":"fixture/cheap"})).unwrap();
        configuration.session(&json!({"models":{"currentModelId":"fixture/other","availableModels":[
            {"modelId":"fixture/cheap","name":"Cheap"},{"modelId":"fixture/other","name":"Other"}]}})).unwrap();
        let request = configuration.next("original-session").unwrap().unwrap();
        assert_eq!(request.method(), "session/set_model");
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({"sessionId":"original-session","modelId":"fixture/cheap"})
        );
        configuration.acknowledge(&json!({})).unwrap();
        assert!(configuration.next("original-session").unwrap().is_none());
        assert_eq!(
            configuration.projection()["reported"]["model"]["current"],
            "fixture/cheap"
        );

        let mut unavailable = Configuration::new(json!({"model":"fixture/not-offered"})).unwrap();
        unavailable
            .session(
                &json!({"models":{"currentModelId":"fixture/cheap","availableModels":[
            {"modelId":"fixture/cheap","name":"Cheap"}]}}),
            )
            .unwrap();
        assert!(unavailable.next("original-session").is_err());
        assert_eq!(unavailable.projection()["failure"], "unsupported-model");
    }

    #[test]
    fn e06s01_grouped_choices_preserve_exact_category_and_option_ids() {
        let mut configuration = Configuration::new(json!({"effort":"low"})).unwrap();
        configuration
            .session(&json!({"configOptions":catalog("fixture/cheap", "off")}))
            .unwrap();
        let request = configuration.next("original-session").unwrap().unwrap();
        assert_eq!(request.method(), "session/set_config_option");
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({"sessionId":"original-session","configId":"depth","value":"low"})
        );
        configuration
            .acknowledge(&json!({"configOptions":catalog("fixture/cheap", "low")}))
            .unwrap();
        assert!(configuration.next("original-session").unwrap().is_none());
        assert_eq!(
            configuration.projection()["reported"]["effort"]["option_id"],
            "depth"
        );
    }

    #[test]
    fn e06s01_later_startup_setting_cannot_silently_reset_an_acknowledged_model() {
        let mut configuration =
            Configuration::new(json!({"model":"fixture/cheap","effort":"low"})).unwrap();
        configuration
            .session(&json!({"configOptions":catalog("fixture/other", "off")}))
            .unwrap();
        assert!(configuration.next("original-session").unwrap().is_some());
        configuration
            .acknowledge(&json!({"configOptions":catalog("fixture/cheap", "off")}))
            .unwrap();
        assert!(configuration.next("original-session").unwrap().is_some());
        configuration
            .acknowledge(&json!({"configOptions":catalog("fixture/other", "low")}))
            .unwrap();
        assert!(configuration.next("original-session").is_err());
        assert_eq!(configuration.projection()["failure"], "setting-mismatch");
        assert_eq!(
            configuration.projection()["reported"]["model"]["current"],
            "fixture/other"
        );
    }

    #[test]
    fn e06s01_duplicate_advertised_ids_are_not_silently_dropped_by_typed_defaults() {
        let mut options = catalog("fixture/cheap", "off");
        options[1]["id"] = json!("engine");
        let mut configuration = Configuration::new(json!({"model":"fixture/cheap"})).unwrap();
        assert!(
            configuration
                .session(&json!({"configOptions":options}))
                .is_err()
        );
        assert_eq!(
            configuration.projection()["failure"],
            "invalid-settings-metadata"
        );
    }
}
