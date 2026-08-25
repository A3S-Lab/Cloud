use serde::{Deserialize, Serialize};

/// Provider-normalized pull-request lifecycle observation retained inside the
/// Sources bounded context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestChangeKind {
    Opened,
    Synchronized,
    Reopened,
    Closed,
}

impl PullRequestChangeKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "opened" => Ok(Self::Opened),
            "synchronize" => Ok(Self::Synchronized),
            "reopened" => Ok(Self::Reopened),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("unsupported pull-request change kind `{value}`")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Synchronized => "synchronize",
            Self::Reopened => "reopened",
            Self::Closed => "closed",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed)
    }
}
