use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitProvider {
    Github,
}

impl GitProvider {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "github" => Ok(Self::Github),
            _ => Err("Git provider is not supported".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
        }
    }
}
