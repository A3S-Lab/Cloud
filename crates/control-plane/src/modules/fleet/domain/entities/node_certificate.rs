use crate::modules::shared_kernel::domain::{NodeCertificateId, NodeId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCertificate {
    pub id: NodeCertificateId,
    pub node_id: NodeId,
    pub serial_number: String,
    pub fingerprint: String,
    pub certificate_pem: String,
    pub ca_bundle_pem: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCertificateMaterial {
    pub serial_number: String,
    pub fingerprint: String,
    pub certificate_pem: String,
    pub ca_bundle_pem: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl NodeCertificate {
    pub fn new(
        id: NodeCertificateId,
        node_id: NodeId,
        material: NodeCertificateMaterial,
    ) -> Result<Self, String> {
        if material.serial_number.is_empty() || material.serial_number.len() > 255 {
            return Err("certificate serial number is invalid".into());
        }
        validate_digest(&material.fingerprint)?;
        if material.certificate_pem.len() > 128 * 1024 || material.ca_bundle_pem.len() > 512 * 1024
        {
            return Err("node certificate material exceeds size limits".into());
        }
        if material.expires_at <= material.issued_at {
            return Err("node certificate must expire after issue time".into());
        }
        Ok(Self {
            id,
            node_id,
            serial_number: material.serial_number,
            fingerprint: material.fingerprint,
            certificate_pem: material.certificate_pem,
            ca_bundle_pem: material.ca_bundle_pem,
            issued_at: material.issued_at,
            expires_at: material.expires_at,
            revoked_at: None,
        })
    }

    pub fn is_valid_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && now >= self.issued_at && now < self.expires_at
    }
}

fn validate_digest(value: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err("certificate fingerprint must use sha256".into());
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("certificate fingerprint must contain 64 hexadecimal characters".into());
    }
    Ok(())
}
