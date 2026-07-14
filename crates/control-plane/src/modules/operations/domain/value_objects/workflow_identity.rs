use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowIdentity {
    name: String,
    version: String,
}

impl WorkflowIdentity {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        let version = version.into();
        if !bounded_text(&name, 255) {
            return Err("workflow name must contain 1 to 255 characters without controls".into());
        }
        if !bounded_text(&version, 63) {
            return Err("workflow version must contain 1 to 63 characters without controls".into());
        }
        Ok(Self { name, version })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

fn bounded_text(value: &str, max_length: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= max_length
        && !value.chars().any(char::is_control)
}
