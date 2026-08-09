use a3s_form_core::{canonicalize_value, CanonicalValue};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormDraftRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub document: CanonicalValue,
}

impl FormDraftRequest {
    pub fn into_parts(self) -> Result<(String, String, String), String> {
        let document = canonicalize_value(&self.document)
            .map_err(|error| format!("Form document could not be canonicalized: {error}"))?;
        let document = String::from_utf8(document)
            .map_err(|_| "Form document canonical JSON is not UTF-8".to_owned())?;
        Ok((self.name, self.description, document))
    }
}
