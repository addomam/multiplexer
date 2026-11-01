use serde::Deserialize;

/// Wire operation. This shape still stores `omp` and `recipe` together.
/// The serde test expects that pair to be rejected.
#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
pub enum Command {
    #[serde(rename = "open")]
    Open {
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        omp: bool,
        #[serde(default)]
        recipe: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rejects_omp_together_with_recipe() {
        let parsed =
            serde_json::from_str::<Command>(r#"{"op":"open","omp":true,"recipe":"check"}"#);
        assert!(
            parsed.is_err(),
            "omp together with recipe must not deserialize, got {parsed:?}"
        );
    }
}
