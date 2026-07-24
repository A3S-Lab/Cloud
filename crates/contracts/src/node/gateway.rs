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
    if !value.starts_with('/') || value.split('/').any(|component| component == "..") {
        return Err(format!("{label} must be an absolute normalized path"));
    }
    Ok(())
}

fn validate_pem(label: &str, value: &str, kind: &str, maximum: usize) -> Result<(), String> {
    if value.len() > maximum
        || !value.starts_with(&format!("-----BEGIN {kind}-----\n"))
        || !value.ends_with(&format!("-----END {kind}-----\n"))
        || value.contains('\0')
    {
        return Err(format!("{label} must be a bounded PEM value"));
    }
    Ok(())
}
