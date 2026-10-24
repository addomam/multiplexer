//! The published Grok ACP question extension, projected into the existing flat
//! form ledger. It does not create a second responder or reinterpret prose.
use super::{FIELDS, FormAnswer, OPTIONS, schema_keys};
use anyhow::{Result, bail, ensure};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

pub(super) fn form(mut params: Value) -> Result<Value> {
    schema_keys(
        &params,
        &["sessionId", "toolCallId", "questions", "mode", "_meta"],
    )
    .map_err(anyhow::Error::msg)?;
    ensure!(
        matches!(params["mode"].as_str(), Some("default" | "plan")),
        "unsupported: Grok question mode"
    );
    ensure!(
        params["toolCallId"].as_str().is_some_and(super::valid_id),
        "invalid-request: Grok question tool identity"
    );
    let questions = params["questions"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("invalid-request: Grok questions"))?;
    ensure!(
        !questions.is_empty() && questions.len() <= FIELDS / 2,
        "unsupported: Grok question count"
    );
    let mut names = BTreeSet::new();
    let mut options = 0;
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (index, question) in questions.iter().enumerate() {
        schema_keys(
            question,
            &[
                "question",
                "options",
                "multiSelect",
                "multi_select",
                "id",
                "_meta",
            ],
        )
        .map_err(anyhow::Error::msg)?;
        let text = question["question"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("invalid-request: Grok question text"))?;
        ensure!(
            names.insert(text),
            "unsupported: duplicate Grok question text"
        );
        ensure!(
            question.get("multiSelect").is_none() || question.get("multi_select").is_none(),
            "invalid-request: ambiguous Grok multiselect"
        );
        let multiple = match question
            .get("multiSelect")
            .or_else(|| question.get("multi_select"))
            .filter(|value| !value.is_null())
        {
            Some(value) => value
                .as_bool()
                .ok_or_else(|| anyhow::anyhow!("invalid-request: Grok multiselect"))?,
            None => false,
        };
        if let Some(id) = question.get("id") {
            ensure!(
                id.as_str().is_some_and(super::valid_id),
                "invalid-request: Grok question id"
            );
        }
        let offered = question["options"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("invalid-request: Grok options"))?;
        ensure!(offered.len() <= OPTIONS, "unsupported: Grok option count");
        let mut labels = BTreeSet::new();
        let mut choices = Vec::new();
        for option in offered {
            schema_keys(option, &["label", "description", "preview", "id", "_meta"])
                .map_err(anyhow::Error::msg)?;
            let label = option["label"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("invalid-request: Grok option label"))?;
            ensure!(
                labels.insert(label),
                "unsupported: duplicate Grok option label"
            );
            let description = option["description"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("invalid-request: Grok option description"))?;
            if let Some(id) = option.get("id") {
                ensure!(
                    id.as_str().is_some_and(super::valid_id),
                    "invalid-request: Grok option id"
                );
            }
            let mut choice = json!({"const":label,"title":label,"description":description});
            if let Some(preview) = option.get("preview").filter(|value| !value.is_null()) {
                ensure!(preview.is_string(), "invalid-request: Grok option preview");
                choice["_meta"] = json!({"preview":preview});
            }
            choices.push(choice);
        }
        if !labels.contains("Other") {
            choices.push(json!({"const":"Other","title":"Other","description":"Supply the free-form answer in the notes field."}));
        }
        options += choices.len();
        ensure!(options <= OPTIONS, "unsupported: Grok total options");
        let key = format!("q{index}");
        let notes = format!("q{index}_notes");
        let mut field = if multiple {
            json!({"type":"array","items":{"type":"string","anyOf":choices},"minItems":1})
        } else {
            json!({"type":"string","oneOf":choices})
        };
        field["title"] = json!(text);
        field["_meta"] = json!({"grok_notes_key":notes});
        properties.insert(key.clone(), field);
        properties.insert(notes, json!({"type":"string","title":format!("Notes for question {}", index + 1),"description":"Optional answer notes; required when selecting Other."}));
        required.push(key);
    }
    Ok(
        json!({"mode":"form","sessionId":params["sessionId"].take(),"toolCallId":params["toolCallId"].take(),
        "message":"Grok question: choose deliberately; Other requires free-form notes. Plan chat/skip are not accept/decline actions.",
        "requestedSchema":{"type":"object","properties":properties,"required":required}}),
    )
}

pub(super) fn response(schema: &Value, answer: &FormAnswer) -> Result<Value> {
    let values = match answer {
        FormAnswer::Accept(values) => values,
        FormAnswer::Cancel => return Ok(json!({"outcome":"cancelled"})),
        FormAnswer::Decline => {
            bail!("unsupported: Grok questions offer accept/cancel, not decline")
        }
    };
    let mut answers = Map::new();
    let mut annotations = Map::new();
    for (key, field) in schema["properties"]
        .as_object()
        .expect("validated Grok form")
    {
        let Some(notes_key) = field["_meta"]["grok_notes_key"].as_str() else {
            continue;
        };
        let text = field["title"].as_str().expect("validated Grok question");
        let selected: Vec<&str> = if let Some(value) = values[key].as_str() {
            vec![value]
        } else {
            values[key]
                .as_array()
                .expect("validated Grok choices")
                .iter()
                .map(|v| v.as_str().expect("validated choice"))
                .collect()
        };
        let notes = values[notes_key].as_str().filter(|s| !s.is_empty());
        ensure!(
            !selected.contains(&"Other") || notes.is_some(),
            "invalid-request: Other requires answer notes"
        );
        answers.insert(text.to_owned(), json!(selected));
        let mut annotation = Map::new();
        if let Some(notes) = notes {
            annotation.insert("notes".into(), json!(notes));
        }
        if selected.len() == 1 {
            let choices = if field["type"] == "array" {
                &field["items"]["anyOf"]
            } else {
                &field["oneOf"]
            };
            if let Some(preview) = choices
                .as_array()
                .expect("validated Grok options")
                .iter()
                .find(|option| option["const"] == selected[0])
                .and_then(|option| option["_meta"].get("preview"))
            {
                annotation.insert("preview".into(), preview.clone());
            }
        }
        if !annotation.is_empty() {
            annotations.insert(text.to_owned(), Value::Object(annotation));
        }
    }
    let mut response = json!({"outcome":"accepted","answers":answers});
    if !annotations.is_empty() {
        response["annotations"] = Value::Object(annotations);
    }
    Ok(response)
}
