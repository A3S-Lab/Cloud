use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationSubject {
    kind: String,
    id: Uuid,
}

impl OperationSubject {
    pub fn new(kind: impl Into<String>, id: Uuid) -> Result<Self, String> {
        let kind = kind.into();
        let mut characters = kind.chars();
        if kind.len() > 63
            || !characters
                .next()
                .is_some_and(|value| value.is_ascii_lowercase())
            || !characters
                .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '_')
        {
            return Err(
                "operation subject kind must be 1 to 63 lowercase letters, digits, or underscores and start with a letter"
                    .into(),
            );
        }
        Ok(Self { kind, id })
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub const fn id(&self) -> Uuid {
        self.id
    }
}
