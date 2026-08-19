use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::net::IpAddr;
use uuid::Uuid;

use super::{validate_sha256, validate_single_line, validate_uuid};

const MAX_GATEWAY_ACL_BYTES: usize = 1024 * 1024;
const MAX_GATEWAY_CERTIFICATE_DNS_NAMES: usize = 100;
const MAX_GATEWAY_SNAPSHOT_VALIDITY_HOURS: i64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayManagementProtocolDiscovery {
    Advertised,
    LegacyVersionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayManagementProtocol {
    pub protocol: String,
    pub snapshot_request_schema: String,
    pub snapshot_status_schema: String,
    pub discovery: GatewayManagementProtocolDiscovery,
}

impl GatewayManagementProtocol {
    pub const V1: &'static str = "a3s.gateway.management-protocol.v1";
    pub const SNAPSHOT_REQUEST_V1: &'static str = "a3s.gateway.managed-snapshot.v1";
    pub const SNAPSHOT_STATUS_V1: &'static str = "a3s.gateway.managed-snapshot-status.v1";

    pub fn v1(discovery: GatewayManagementProtocolDiscovery) -> Self {
        Self {
            protocol: Self::V1.into(),
            snapshot_request_schema: Self::SNAPSHOT_REQUEST_V1.into(),
            snapshot_status_schema: Self::SNAPSHOT_STATUS_V1.into(),
            discovery,
        }
    }

    pub fn advertised_v1() -> Self {
        Self::v1(GatewayManagementProtocolDiscovery::Advertised)
    }

    pub fn legacy_v1() -> Self {
        Self::v1(GatewayManagementProtocolDiscovery::LegacyVersionV1)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != Self::V1
            || self.snapshot_request_schema != Self::SNAPSHOT_REQUEST_V1
            || self.snapshot_status_schema != Self::SNAPSHOT_STATUS_V1
        {
            return Err(format!(
                "unsupported Gateway management protocol {:?}",
                self.protocol
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewaySnapshotObservationRequest {
    pub schema: String,
    pub gateway_id: Uuid,
    pub revision: u64,
    pub snapshot_digest: String,
}

impl GatewaySnapshotObservationRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.gateway-snapshot-observation-request.v1";

    pub fn new(
        gateway_id: Uuid,
        revision: u64,
        snapshot_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            gateway_id,
            revision,
            snapshot_digest: snapshot_digest.into(),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Gateway snapshot observation request schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Gateway snapshot observation Gateway ID", self.gateway_id)?;
        if self.revision == 0 {
            return Err("Gateway snapshot observation revision must be positive".into());
        }
        validate_sha256("Gateway snapshot observation digest", &self.snapshot_digest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewaySnapshotObservationState {
    Applying,
    Applied,
    Rejected,
    Expired,
    NotApplied,
    Uninitialized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedGatewaySnapshot {
    pub gateway_id: Uuid,
    pub revision: u64,
    pub expected_revision: Option<u64>,
    pub snapshot_digest: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub applied_at: DateTime<Utc>,
}

impl AppliedGatewaySnapshot {
    pub fn validate(&self) -> Result<(), String> {
        validate_uuid("applied Gateway snapshot Gateway ID", self.gateway_id)?;
        if self.revision == 0
            || self
                .expected_revision
                .is_some_and(|revision| revision == 0 || revision >= self.revision)
        {
            return Err("applied Gateway snapshot revision chain is invalid".into());
        }
        validate_sha256("applied Gateway snapshot digest", &self.snapshot_digest)?;
        if self.expires_at <= self.issued_at
            || self.applied_at < self.issued_at
            || self.applied_at >= self.expires_at
        {
            return Err("applied Gateway snapshot timestamps are invalid".into());
        }
        Ok(())
    }

    pub fn matches(&self, request: &GatewaySnapshotObservationRequest) -> bool {
        self.gateway_id == request.gateway_id
            && self.revision == request.revision
            && self.snapshot_digest == request.snapshot_digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeGatewaySnapshotObservation {
    pub schema: String,
    pub observation_id: Uuid,
    pub command_id: Uuid,
    pub node_id: Uuid,
    pub gateway_id: Uuid,
    pub revision: u64,
    pub snapshot_digest: String,
    pub state: GatewaySnapshotObservationState,
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<AppliedGatewaySnapshot>,
    pub observed_at: DateTime<Utc>,
    pub management_protocol: GatewayManagementProtocol,
}

impl NodeGatewaySnapshotObservation {
    pub const SCHEMA: &'static str = "a3s.cloud.node-gateway-snapshot-observation.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node Gateway snapshot observation schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Gateway snapshot observation ID", self.observation_id)?;
        validate_uuid("Gateway snapshot observation command ID", self.command_id)?;
        validate_uuid("Gateway snapshot observation node ID", self.node_id)?;
        validate_uuid("Gateway snapshot observation Gateway ID", self.gateway_id)?;
        if self.node_id != self.gateway_id || self.revision == 0 {
            return Err("Gateway snapshot observation identity is inconsistent".into());
        }
        validate_sha256("Gateway snapshot observation digest", &self.snapshot_digest)?;
        self.management_protocol.validate()?;
        if let Some(applied) = &self.applied {
            applied.validate()?;
            if applied.gateway_id != self.gateway_id || applied.applied_at > self.observed_at {
                return Err(
                    "Gateway snapshot observation applied state identity or time is inconsistent"
                        .into(),
                );
            }
        }
        let requested_is_applied = self.applied.as_ref().is_some_and(|applied| {
            applied.gateway_id == self.gateway_id
                && applied.revision == self.revision
                && applied.snapshot_digest == self.snapshot_digest
        });
        match self.state {
            GatewaySnapshotObservationState::Applied => {
                if !self.ready
                    || !requested_is_applied
                    || self
                        .applied
                        .as_ref()
                        .is_some_and(|applied| applied.expires_at <= self.observed_at)
                {
                    return Err(
                        "applied Gateway snapshot observation is not exact and ready".into(),
                    );
                }
            }
            GatewaySnapshotObservationState::Expired => {
                if self.ready
                    || !requested_is_applied
                    || self
                        .applied
                        .as_ref()
                        .is_none_or(|applied| applied.expires_at > self.observed_at)
                {
                    return Err("expired Gateway snapshot observation is inconsistent".into());
                }
            }
            GatewaySnapshotObservationState::Rejected
            | GatewaySnapshotObservationState::NotApplied => {
                if self.ready || requested_is_applied {
                    return Err(
                        "unapplied Gateway snapshot observation retained the requested state"
                            .into(),
                    );
                }
            }
            GatewaySnapshotObservationState::Uninitialized => {
                if self.ready || self.applied.is_some() {
                    return Err("uninitialized Gateway snapshot observation is inconsistent".into());
                }
            }
            GatewaySnapshotObservationState::Applying => {
                if self.ready {
                    return Err("applying Gateway snapshot observation cannot be ready".into());
                }
            }
        }
        Ok(())
    }

    pub fn validate_for(
        &self,
        command_id: Uuid,
        node_id: Uuid,
        request: &GatewaySnapshotObservationRequest,
    ) -> Result<(), String> {
        self.validate()?;
        request.validate()?;
        if self.command_id != command_id
            || self.node_id != node_id
            || self.gateway_id != request.gateway_id
            || self.revision != request.revision
            || self.snapshot_digest != request.snapshot_digest
        {
            return Err(
                "Gateway snapshot observation does not match its exact node command".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayCertificateRequest {
    pub schema: String,
    pub certificate_id: Uuid,
    pub dns_names: Vec<String>,
    pub certificate_file: String,
    pub private_key_file: String,
}

impl GatewayCertificateRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.gateway-certificate-request.v1";

    pub fn new(
        certificate_id: Uuid,
        dns_names: Vec<String>,
        certificate_file: impl Into<String>,
        private_key_file: impl Into<String>,
    ) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            certificate_id,
            dns_names,
            certificate_file: certificate_file.into(),
            private_key_file: private_key_file.into(),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Gateway certificate request schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Gateway certificate ID", self.certificate_id)?;
        validate_dns_names(&self.dns_names)?;
        validate_file_reference("Gateway certificate file", &self.certificate_file)?;
        validate_file_reference("Gateway private key file", &self.private_key_file)?;
        if self.certificate_file == self.private_key_file {
            return Err("Gateway certificate and private key files must differ".into());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayCertificateSigningRequest {
    pub schema: String,
    pub certificate_id: Uuid,
    pub node_id: Uuid,
    pub csr_pem: String,
    pub requested_at: DateTime<Utc>,
}

impl GatewayCertificateSigningRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.gateway-certificate-signing-request.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Gateway certificate signing request schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Gateway certificate ID", self.certificate_id)?;
        validate_uuid("Gateway certificate node ID", self.node_id)?;
        validate_pem(
            "Gateway certificate signing request",
            &self.csr_pem,
            "CERTIFICATE REQUEST",
            64 * 1024,
        )?;
        if self.csr_pem.contains("PRIVATE KEY") {
            return Err(
                "Gateway certificate signing request must not contain a private key".into(),
            );
        }
        Ok(())
    }
}

impl fmt::Debug for GatewayCertificateSigningRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayCertificateSigningRequest")
            .field("schema", &self.schema)
            .field("certificate_id", &self.certificate_id)
            .field("node_id", &self.node_id)
            .field("csr_pem", &"<redacted-csr>")
            .field("requested_at", &self.requested_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayCertificateSigningResponse {
    pub schema: String,
    pub certificate_id: Uuid,
    pub node_id: Uuid,
    pub dns_names: Vec<String>,
    pub serial_number: String,
    pub fingerprint: String,
    pub certificate_pem: String,
    pub ca_bundle_pem: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl GatewayCertificateSigningResponse {
    pub const SCHEMA: &'static str = "a3s.cloud.gateway-certificate-signing-response.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Gateway certificate signing response schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Gateway certificate ID", self.certificate_id)?;
        validate_uuid("Gateway certificate node ID", self.node_id)?;
        validate_dns_names(&self.dns_names)?;
        validate_single_line(
            "Gateway certificate serial number",
            &self.serial_number,
            512,
        )?;
        validate_sha256("Gateway certificate fingerprint", &self.fingerprint)?;
        validate_pem(
            "Gateway certificate",
            &self.certificate_pem,
            "CERTIFICATE",
            256 * 1024,
        )?;
        validate_pem(
            "Gateway certificate CA bundle",
            &self.ca_bundle_pem,
            "CERTIFICATE",
            256 * 1024,
        )?;
        if self.expires_at <= self.issued_at {
            return Err("Gateway certificate expiry must follow its issue time".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewaySnapshot {
    pub schema: String,
    pub gateway_id: Uuid,
    pub revision: u64,
    pub expected_revision: Option<u64>,
    pub snapshot_digest: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub acl: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_request: Option<GatewayCertificateRequest>,
}

impl GatewaySnapshot {
    pub const SCHEMA: &'static str = "a3s.cloud.gateway-snapshot.v3";

    pub fn new(
        gateway_id: Uuid,
        revision: u64,
        expected_revision: Option<u64>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        acl: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new_with_certificate(
            gateway_id,
            revision,
            expected_revision,
            issued_at,
            expires_at,
            acl,
            None,
        )
    }

    pub fn new_with_certificate(
        gateway_id: Uuid,
        revision: u64,
        expected_revision: Option<u64>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        acl: impl Into<String>,
        certificate_request: Option<GatewayCertificateRequest>,
    ) -> Result<Self, String> {
        let acl = acl.into();
        let snapshot_digest = digest_acl(&acl);
        let snapshot = Self {
            schema: Self::SCHEMA.into(),
            gateway_id,
            revision,
            expected_revision,
            snapshot_digest,
            issued_at,
            expires_at,
            acl,
            certificate_request,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Gateway snapshot schema {:?}",
                self.schema
            ));
        }
        validate_uuid("Gateway snapshot Gateway ID", self.gateway_id)?;
        if self.revision == 0 {
            return Err("Gateway snapshot revision must be positive".into());
        }
        if self
            .expected_revision
            .is_some_and(|expected| expected == 0 || expected >= self.revision)
        {
            return Err(
                "Gateway snapshot expected revision must be positive and precede its revision"
                    .into(),
            );
        }
        if self.acl.trim().is_empty()
            || self.acl.len() > MAX_GATEWAY_ACL_BYTES
            || self.acl.contains('\0')
        {
            return Err("Gateway snapshot ACL must contain 1 byte to 1 MiB without NUL".into());
        }
        if let Some(certificate) = &self.certificate_request {
            certificate.validate()?;
            if !self.acl.contains(&certificate.certificate_file)
                || !self.acl.contains(&certificate.private_key_file)
            {
                return Err(
                    "Gateway snapshot ACL does not reference its certificate and private key files"
                        .into(),
                );
            }
        }
        if self.expires_at <= self.issued_at {
            return Err("Gateway snapshot expiry must follow its issue time".into());
        }
        if self.expires_at - self.issued_at > Duration::hours(MAX_GATEWAY_SNAPSHOT_VALIDITY_HOURS) {
            return Err(format!(
                "Gateway snapshot validity must not exceed {MAX_GATEWAY_SNAPSHOT_VALIDITY_HOURS} hours"
            ));
        }
        validate_sha256("Gateway snapshot digest", &self.snapshot_digest)?;
        let expected_digest = digest_acl(&self.acl);
        if self.snapshot_digest != expected_digest {
            return Err("Gateway snapshot digest does not match its exact ACL bytes".into());
        }
        Ok(())
    }
}

fn digest_acl(acl: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(acl.as_bytes()))
}

fn validate_dns_name(value: &str) -> Result<(), String> {
    validate_single_line("Gateway certificate DNS name", value, 253)?;
    let suffix = value.strip_prefix("*.").unwrap_or(value);
    if suffix.parse::<IpAddr>().is_ok()
        || suffix.ends_with('.')
        || suffix.split('.').count() < 2
        || suffix.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        || value.contains('*') && !value.starts_with("*.")
    {
        return Err("Gateway certificate DNS name must be canonical exact or wildcard DNS".into());
    }
    Ok(())
}

fn validate_dns_names(values: &[String]) -> Result<(), String> {
    if values.is_empty() || values.len() > MAX_GATEWAY_CERTIFICATE_DNS_NAMES {
        return Err("Gateway certificate must contain 1 to 100 DNS names".into());
    }
    let mut previous: Option<&str> = None;
    for dns_name in values {
        validate_dns_name(dns_name)?;
        if previous.is_some_and(|value| value >= dns_name.as_str()) {
            return Err(
                "Gateway certificate DNS names must be sorted and contain no duplicates".into(),
            );
        }
        previous = Some(dns_name);
    }
    Ok(())
}

fn validate_file_reference(label: &str, value: &str) -> Result<(), String> {
    validate_single_line(label, value, 4096)?;
    let bytes = value.as_bytes();
    let posix_absolute = value.starts_with('/');
    let windows_drive_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if !(posix_absolute || windows_drive_absolute)
        || value.split(['/', '\\']).any(|component| component == "..")
    {
        return Err(format!("{label} must be an absolute normalized path"));
    }
    Ok(())
}

fn validate_pem(label: &str, value: &str, kind: &str, maximum: usize) -> Result<(), String> {
    let lf_header = format!("-----BEGIN {kind}-----\n");
    let crlf_header = format!("-----BEGIN {kind}-----\r\n");
    let lf_footer = format!("-----END {kind}-----\n");
    let crlf_footer = format!("-----END {kind}-----\r\n");
    if value.len() > maximum
        || !(value.starts_with(&lf_header) || value.starts_with(&crlf_header))
        || !(value.ends_with(&lf_footer) || value.ends_with(&crlf_footer))
        || value.contains('\0')
    {
        return Err(format!("{label} must be a bounded PEM value"));
    }
    Ok(())
}
