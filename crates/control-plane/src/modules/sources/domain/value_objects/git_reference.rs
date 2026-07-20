use super::GitCommitSha;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum GitReference {
    Branch(String),
    Tag(String),
    Commit(GitCommitSha),
}

impl GitReference {
    pub fn parse(kind: &str, value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        match kind {
            "branch" => Ok(Self::Branch(parse_named_reference(value)?)),
            "tag" => Ok(Self::Tag(parse_named_reference(value)?)),
            "commit" => Ok(Self::Commit(GitCommitSha::parse(value)?)),
            _ => Err("Git reference kind must be branch, tag, or commit".into()),
        }
    }

    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Branch(_) => "branch",
            Self::Tag(_) => "tag",
            Self::Commit(_) => "commit",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Self::Branch(value) | Self::Tag(value) => value,
            Self::Commit(value) => value.as_str(),
        }
    }
}

fn parse_named_reference(value: String) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 255
        || value == "@"
        || value.starts_with("refs/")
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("..")
        || value.contains("//")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
        || value.split('/').any(|segment| {
            segment.is_empty()
                || segment.starts_with('.')
                || segment.ends_with('.')
                || segment.ends_with(".lock")
        })
    {
        return Err("Git branch or tag must be a bounded safe name without a refs/ prefix".into());
    }
    Ok(value)
}
