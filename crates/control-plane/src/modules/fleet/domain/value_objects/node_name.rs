use crate::modules::shared_kernel::domain::ResourceName;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeName(ResourceName);

impl Serialize for NodeName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.value())
    }
}

impl<'de> Deserialize<'de> for NodeName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl NodeName {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        ResourceName::parse(value).map(Self)
    }

    pub fn value(&self) -> &str {
        self.0.as_str()
    }

    pub fn uniqueness_key(&self) -> &str {
        self.0.key()
    }
}
