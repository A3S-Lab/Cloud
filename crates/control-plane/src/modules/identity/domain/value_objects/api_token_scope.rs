use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiTokenScope(String);

impl ApiTokenScope {
    pub const PLATFORM_WRITE: &'static str = "platform:write";
    pub const TOKEN_WRITE: &'static str = "token:write";
    pub const PROJECT_WRITE: &'static str = "project:write";
    pub const ENVIRONMENT_WRITE: &'static str = "environment:write";

    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 63
            && value.split_once(':').is_some_and(|(domain, action)| {
                !domain.is_empty()
                    && !action.is_empty()
                    && domain
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
                    && action
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
            });
        if !valid {
            return Err("API token scope must use bounded lowercase domain:action syntax".into());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn bootstrap_scopes() -> std::collections::BTreeSet<Self> {
        [
            Self::PLATFORM_WRITE,
            Self::TOKEN_WRITE,
            Self::PROJECT_WRITE,
            Self::ENVIRONMENT_WRITE,
        ]
        .into_iter()
        .map(|scope| Self(scope.to_owned()))
        .collect()
    }
}
