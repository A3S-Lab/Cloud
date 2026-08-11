use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginRegistryState {
    Active,
    Disabled,
}

impl PluginRegistryState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            _ => Err("stored plugin registry state is invalid".into()),
        }
    }
}
