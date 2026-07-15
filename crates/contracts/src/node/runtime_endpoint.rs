use a3s_runtime::contract::RuntimeObservation;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use url::Url;

const CLAIM_PREFIX: &str = "a3s.cloud.service-endpoint.";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuntimeServiceEndpoint {
    pub port_name: String,
    pub origin: String,
}

impl RuntimeServiceEndpoint {
    pub fn new(port_name: impl Into<String>, origin: impl AsRef<str>) -> Result<Self, String> {
        let endpoint = Self {
            port_name: port_name.into(),
            origin: canonical_origin(origin.as_ref())?,
        };
        endpoint.validate()?;
        Ok(endpoint)
    }

    pub fn node_local_http(port_name: impl Into<String>, port: u16) -> Result<Self, String> {
        if port == 0 {
            return Err("Runtime service endpoint port must be positive".into());
        }
        Self::new(port_name, format!("http://127.0.0.1:{port}"))
    }

    pub fn validate(&self) -> Result<(), String> {
        if !valid_port_name(&self.port_name) || canonical_origin(&self.origin)? != self.origin {
            return Err("Runtime service endpoint is invalid".into());
        }
        Ok(())
    }

    pub fn claim_key(&self) -> String {
        format!("{CLAIM_PREFIX}{}", self.port_name)
    }

    pub fn from_observation(
        observation: &RuntimeObservation,
        port_name: &str,
    ) -> Result<Self, String> {
        if !valid_port_name(port_name) {
            return Err("Runtime service endpoint port name is invalid".into());
        }
        let value = observation
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.claims.get(&format!("{CLAIM_PREFIX}{port_name}")))
            .ok_or_else(|| {
                format!("Runtime observation has no node-local endpoint for port {port_name:?}")
            })?;
        Self::new(port_name, value)
    }
}

fn canonical_origin(value: &str) -> Result<String, String> {
    let url = Url::parse(value)
        .map_err(|error| format!("Runtime service endpoint origin is invalid: {error}"))?;
    let loopback = url
        .host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|address| address.is_loopback());
    if url.scheme() != "http"
        || !loopback
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Runtime service endpoint must be an explicit node-local HTTP origin".into());
    }
    Ok(url.to_string())
}

fn valid_port_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_runtime::contract::{
        RuntimeEvidence, RuntimeHealthObservation, RuntimeHealthState, RuntimeUnitClass,
        RuntimeUnitState,
    };
    use std::collections::BTreeMap;

    #[test]
    fn extracts_only_typed_node_local_origins() {
        let endpoint =
            RuntimeServiceEndpoint::node_local_http("http", 49152).expect("node-local endpoint");
        let mut observation = RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.into(),
            unit_id: "workload:test".into(),
            generation: 1,
            spec_digest: format!("sha256:{}", "a".repeat(64)),
            class: RuntimeUnitClass::Service,
            state: RuntimeUnitState::Running,
            provider_resource_id: Some("container".into()),
            provider_build: Some("docker/test".into()),
            observed_at_ms: 1,
            started_at_ms: Some(1),
            finished_at_ms: None,
            health: Some(RuntimeHealthObservation {
                state: RuntimeHealthState::Healthy,
                checked_at_ms: 1,
                message: None,
            }),
            outputs: Vec::new(),
            usage: None,
            evidence: Some(RuntimeEvidence {
                provider_build: "docker/test".into(),
                spec_digest: format!("sha256:{}", "a".repeat(64)),
                semantics_profile_digest: None,
                claims: BTreeMap::from([(endpoint.claim_key(), endpoint.origin.clone())]),
            }),
            provider_attestation: None,
            failure: None,
        };
        assert_eq!(
            RuntimeServiceEndpoint::from_observation(&observation, "http").expect("endpoint"),
            endpoint
        );
        observation
            .evidence
            .as_mut()
            .expect("evidence")
            .claims
            .insert(endpoint.claim_key(), "http://10.0.0.8:8080/".into());
        assert!(RuntimeServiceEndpoint::from_observation(&observation, "http").is_err());
    }
}
