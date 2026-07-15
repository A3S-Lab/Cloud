use super::vault_client::{VaultClient, VaultClientError};
use crate::modules::fleet::domain::entities::{NodeCertificate, NodeCertificateMaterial};
use crate::modules::fleet::domain::services::{
    CertificateAuthorityError, ICertificateAuthority, NodeCertificateRequest,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::time::Duration;

pub struct VaultCertificateAuthority {
    client: VaultClient,
    mount: String,
    role: String,
}

impl VaultCertificateAuthority {
    pub fn new(
        address: &str,
        token: &str,
        mount: impl Into<String>,
        role: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, CertificateAuthorityError> {
        let mount = validate_segment("Vault PKI mount", mount.into())?;
        let role = validate_segment("Vault PKI role", role.into())?;
        Ok(Self {
            client: VaultClient::new(address, token, timeout).map_err(map_error)?,
            mount,
            role,
        })
    }
}

#[async_trait]
impl ICertificateAuthority for VaultCertificateAuthority {
    async fn issue(
        &self,
        request: NodeCertificateRequest,
    ) -> Result<NodeCertificate, CertificateAuthorityError> {
        if request.expires_at <= request.issued_at {
            return Err(CertificateAuthorityError::InvalidRequest(
                "certificate expiry must follow issue time".into(),
            ));
        }
        let ttl_seconds = (request.expires_at - request.issued_at).num_seconds();
        let data: SignResponse = self
            .client
            .post(
                &format!("{}/sign/{}", self.mount, self.role),
                &SignRequest {
                    csr: &request.csr_pem,
                    common_name: format!("a3s-node:{}", request.node_id),
                    ttl: format!("{ttl_seconds}s"),
                    format: "pem",
                },
            )
            .await
            .map_err(map_error)?;
        let mut input = Cursor::new(data.certificate.as_bytes());
        let certificates = rustls_pemfile::certs(&mut input)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                CertificateAuthorityError::Rejected(format!(
                    "Vault returned an invalid certificate: {error}"
                ))
            })?;
        if certificates.len() != 1 {
            return Err(CertificateAuthorityError::Rejected(
                "Vault returned an unexpected certificate chain".into(),
            ));
        }
        let ca_bundle_pem = if data.ca_chain.is_empty() {
            data.issuing_ca
        } else {
            format!("{}\n", data.ca_chain.join("\n"))
        };
        NodeCertificate::new(
            request.certificate_id,
            request.node_id,
            NodeCertificateMaterial {
                serial_number: data.serial_number,
                fingerprint: format!("sha256:{:x}", Sha256::digest(&certificates[0])),
                certificate_pem: data.certificate,
                ca_bundle_pem,
                issued_at: request.issued_at,
                expires_at: request.expires_at,
            },
        )
        .map_err(CertificateAuthorityError::InvalidRequest)
    }

    async fn revoke(&self, certificate: &NodeCertificate) -> Result<(), CertificateAuthorityError> {
        let _: RevokeResponse = self
            .client
            .post(
                &format!("{}/revoke", self.mount),
                &RevokeRequest {
                    serial_number: &certificate.serial_number,
                },
            )
            .await
            .map_err(map_error)?;
        Ok(())
    }

    async fn health(&self) -> Result<bool, CertificateAuthorityError> {
        self.client.health().await.map_err(map_error)
    }
}

#[derive(Serialize)]
struct SignRequest<'a> {
    csr: &'a str,
    common_name: String,
    ttl: String,
    format: &'static str,
}

#[derive(Deserialize)]
struct SignResponse {
    certificate: String,
    issuing_ca: String,
    #[serde(default)]
    ca_chain: Vec<String>,
    serial_number: String,
}

#[derive(Serialize)]
struct RevokeRequest<'a> {
    serial_number: &'a str,
}

#[derive(Deserialize)]
struct RevokeResponse {}

fn validate_segment(label: &str, value: String) -> Result<String, CertificateAuthorityError> {
    if value.is_empty()
        || value.len() > 255
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    {
        return Err(CertificateAuthorityError::InvalidRequest(format!(
            "{label} is invalid"
        )));
    }
    Ok(value)
}

fn map_error(error: VaultClientError) -> CertificateAuthorityError {
    match error {
        VaultClientError::Configuration(message) => {
            CertificateAuthorityError::InvalidRequest(message)
        }
        VaultClientError::Rejected(message) => CertificateAuthorityError::Rejected(message),
        VaultClientError::Unavailable(message) => CertificateAuthorityError::Unavailable(message),
    }
}
