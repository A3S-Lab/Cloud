use a3s_use_core::PluginReleaseChannel;
use a3s_use_extension::{PluginCatalogHost, PluginCatalogSearch};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogSearchRequest {
    pub host: PluginCatalogHost,
    pub search: PluginCatalogSearch,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogInspectRequest {
    pub host: PluginCatalogHost,
    pub package_id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub channel: Option<PluginReleaseChannel>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_bodies_decode_into_exact_use_contracts() {
        let search: PluginCatalogSearchRequest = serde_json::from_value(serde_json::json!({
            "host": {
                "target": "x86_64-unknown-linux-gnu",
                "useVersion": "0.3.0"
            },
            "search": {
                "query": "a3s",
                "channel": "stable",
                "limit": 20
            }
        }))
        .expect("catalog search request");
        assert_eq!(search.host.target, "x86_64-unknown-linux-gnu");
        assert_eq!(search.search.query, "a3s");
        assert_eq!(search.search.limit, 20);
        assert_eq!(search.search.channel, Some(PluginReleaseChannel::Stable));

        let inspect: PluginCatalogInspectRequest = serde_json::from_value(serde_json::json!({
            "host": {
                "target": "x86_64-unknown-linux-gnu",
                "useVersion": "0.3.0"
            },
            "packageId": "a3s/example",
            "version": "1.2.3",
            "channel": "beta"
        }))
        .expect("catalog inspection request");
        assert_eq!(inspect.host.use_version, "0.3.0");
        assert_eq!(inspect.package_id, "a3s/example");
        assert_eq!(inspect.version.as_deref(), Some("1.2.3"));
        assert_eq!(inspect.channel, Some(PluginReleaseChannel::Beta));
    }
}
