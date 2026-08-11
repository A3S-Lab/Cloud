use serde::{Deserialize, Serialize};
use url::Url;

const MAX_PLUGIN_REGISTRY_ENDPOINT_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PluginRegistryEndpoint(String);

impl PluginRegistryEndpoint {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, String> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > MAX_PLUGIN_REGISTRY_ENDPOINT_BYTES
            || value.trim() != value
        {
            return Err("plugin registry endpoint must be a bounded canonical HTTPS URL".into());
        }
        let mut url = Url::parse(value)
            .map_err(|error| format!("plugin registry endpoint is invalid: {error}"))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "plugin registry endpoint requires HTTPS and cannot contain credentials, query parameters, or fragments"
                    .into(),
            );
        }
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        let normalized = url.to_string();
        if normalized.len() > MAX_PLUGIN_REGISTRY_ENDPOINT_BYTES {
            return Err("plugin registry endpoint exceeds its canonical size bound".into());
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::PluginRegistryEndpoint;

    #[test]
    fn normalizes_one_https_directory_endpoint() {
        assert_eq!(
            PluginRegistryEndpoint::parse("https://registry.example/a3s")
                .expect("endpoint")
                .as_str(),
            "https://registry.example/a3s/"
        );
    }

    #[test]
    fn rejects_non_https_credentials_query_and_fragment() {
        for endpoint in [
            "http://registry.example/",
            "https://user@registry.example/",
            "https://registry.example/?tenant=one",
            "https://registry.example/#catalog",
        ] {
            assert!(
                PluginRegistryEndpoint::parse(endpoint).is_err(),
                "accepted {endpoint}"
            );
        }
    }
}
