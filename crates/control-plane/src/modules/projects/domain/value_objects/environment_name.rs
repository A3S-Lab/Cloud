use crate::modules::shared_kernel::domain::ResourceName;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnvironmentName(ResourceName);

impl EnvironmentName {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        ResourceName::parse(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn key(&self) -> &str {
        self.0.key()
    }
}
