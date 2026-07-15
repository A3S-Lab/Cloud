use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UpstreamEndpoint(String);

impl UpstreamEndpoint {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, String> {
        let url = Url::parse(value.as_ref())
            .map_err(|error| format!("route upstream endpoint is invalid: {error}"))?;
        let loopback = url
            .host_str()
            .and_then(|host| host.parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        if url.scheme() != "http"
            || !loopback
            || url.port().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(
                "route upstream endpoint must be an explicit node-local HTTP origin".into(),
            );
        }
        Ok(Self(url.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
