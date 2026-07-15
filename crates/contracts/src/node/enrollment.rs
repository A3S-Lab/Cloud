use a3s_runtime::contract::RuntimeCapabilities;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{validate_single_line, validate_uuid};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeEnrollmentRequest {
    pub schema: String,
    pub enrollment_token: String,
    pub node_name: String,
    pub agent_instance_id: Uuid,
    pub agent_version: String,
    pub csr_pem: String,
    pub runtime_capabilities: RuntimeCapabilities,
}

impl std::fmt::Debug for NodeEnrollmentRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NodeEnrollmentRequest")
            .field("schema", &self.schema)
            .field("enrollment_token", &"[REDACTED]")
            .field("node_name", &self.node_name)
            .field("agent_instance_id", &self.agent_instance_id)
            .field("agent_version", &self.agent_version)
            .field("csr_pem", &"[REDACTED PEM]")
            .field("runtime_capabilities", &self.runtime_capabilities)
            .finish()
    }
}

impl NodeEnrollmentRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.node-enrollment-request.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node enrollment schema {:?}",
                self.schema
            ));
        }
        let Some(secret) = self.enrollment_token.strip_prefix("a3sn_") else {
            return Err("enrollment token must use the a3sn_ prefix".into());
        };
        if secret.len() != 64
            || !secret
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("enrollment token must contain 64 lowercase hexadecimal characters".into());
        }
        validate_single_line("node name", &self.node_name, 255)?;
        validate_uuid("agent_instance_id", self.agent_instance_id)?;
        validate_single_line("agent version", &self.agent_version, 255)?;
        validate_pem("certificate request", &self.csr_pem, "CERTIFICATE REQUEST")?;
        self.runtime_capabilities.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCertificate {
    pub certificate_id: Uuid,
    pub serial_number: String,
    pub certificate_pem: String,
    pub ca_bundle_pem: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl NodeCertificate {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("certificate_id", self.certificate_id)?;
        validate_single_line("certificate serial number", &self.serial_number, 255)?;
        validate_pem("node certificate", &self.certificate_pem, "CERTIFICATE")?;
        validate_pem("CA bundle", &self.ca_bundle_pem, "CERTIFICATE")?;
        if self.expires_at <= self.issued_at {
            return Err("node certificate must expire after it is issued".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeEnrollmentResponse {
    pub schema: String,
    pub node_id: Uuid,
    pub certificate: NodeCertificate,
    pub heartbeat_interval_ms: u64,
    pub command_long_poll_ms: u64,
    pub certificate_rotation_window_ms: u64,
}

impl NodeEnrollmentResponse {
    pub const SCHEMA: &'static str = "a3s.cloud.node-enrollment-response.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node enrollment response schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        self.certificate.validate()?;
        if self.heartbeat_interval_ms == 0
            || self.command_long_poll_ms == 0
            || self.command_long_poll_ms > 60_000
            || self.certificate_rotation_window_ms == 0
            || self.certificate_rotation_window_ms
                >= u64::try_from(
                    (self.certificate.expires_at - self.certificate.issued_at).num_milliseconds(),
                )
                .unwrap_or(0)
        {
            return Err("node polling intervals are invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCertificateRotationRequest {
    pub schema: String,
    pub node_id: Uuid,
    pub current_certificate_id: Uuid,
    pub csr_pem: String,
}

impl std::fmt::Debug for NodeCertificateRotationRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NodeCertificateRotationRequest")
            .field("schema", &self.schema)
            .field("node_id", &self.node_id)
            .field("current_certificate_id", &self.current_certificate_id)
            .field("csr_pem", &"[REDACTED PEM]")
            .finish()
    }
}

impl NodeCertificateRotationRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.node-certificate-rotation-request.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node certificate rotation schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("current_certificate_id", self.current_certificate_id)?;
        validate_pem("certificate request", &self.csr_pem, "CERTIFICATE REQUEST")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCertificateRotationResponse {
    pub schema: String,
    pub node_id: Uuid,
    pub previous_certificate_id: Uuid,
    pub certificate: NodeCertificate,
    pub replayed: bool,
}

impl NodeCertificateRotationResponse {
    pub const SCHEMA: &'static str = "a3s.cloud.node-certificate-rotation-response.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node certificate rotation response schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("previous_certificate_id", self.previous_certificate_id)?;
        self.certificate.validate()?;
        if self.certificate.certificate_id == self.previous_certificate_id {
            return Err("certificate rotation did not change certificate identity".into());
        }
        Ok(())
    }
}

fn validate_pem(label: &str, value: &str, kind: &str) -> Result<(), String> {
    if value.len() > 128 * 1024
        || !value.starts_with(&format!("-----BEGIN {kind}-----"))
        || !value.trim_end().ends_with(&format!("-----END {kind}-----"))
        || value.contains('\0')
    {
        return Err(format!("{label} is not a bounded PEM document"));
    }
    Ok(())
}
