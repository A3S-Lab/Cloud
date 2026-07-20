use crate::infrastructure::{VaultClient, VaultClientError};
use crate::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, IGatewayCertificateAuthority,
};
use crate::modules::edge::domain::{GatewayCertificate, GatewayCertificateMaterial};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use rcgen::{CertificateParams, ExtendedKeyUsagePurpose, IsCa, SanType};
use rustls::pki_types::CertificateDer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

const MAX_PROVIDER_CLOCK_SKEW_SECONDS: i64 = 300;

pub struct VaultGatewayCertificateAuthority {
    client: Arc<dyn GatewayPkiClient>,
    mount: String,
    role: String,
}

impl VaultGatewayCertificateAuthority {
    pub fn new(
        address: &str,
        token: &str,
        mount: impl Into<String>,
        role: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, GatewayCertificateAuthorityError> {
        let client = VaultClient::new(address, token, timeout).map_err(map_client_error)?;
        Self::with_client(Arc::new(HttpGatewayPkiClient { client }), mount, role)
    }

    fn with_client(
        client: Arc<dyn GatewayPkiClient>,
        mount: impl Into<String>,
        role: impl Into<String>,
    ) -> Result<Self, GatewayCertificateAuthorityError> {
        Ok(Self {
            client,
            mount: validate_segment("Vault Gateway PKI mount", mount.into())?,
            role: validate_segment("Vault Gateway PKI role", role.into())?,
        })
    }

    fn sign_path(&self) -> String {
        format!("{}/sign/{}", self.mount, self.role)
    }

    fn revoke_path(&self) -> String {
        format!("{}/revoke", self.mount)
    }
}

#[async_trait]
impl IGatewayCertificateAuthority for VaultGatewayCertificateAuthority {
    async fn issue(
        &self,
        request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        request
            .validate()
            .map_err(GatewayCertificateAuthorityError::InvalidRequest)?;
        let ttl_seconds = (request.expires_at - request.issued_at).num_seconds();
        if ttl_seconds <= 0 {
            return Err(GatewayCertificateAuthorityError::InvalidRequest(
                "Gateway certificate lifetime must include at least one second".into(),
            ));
        }
        let common_name = request.dns_names.first().cloned().ok_or_else(|| {
            GatewayCertificateAuthorityError::InvalidRequest(
                "Gateway certificate requires at least one DNS name".into(),
            )
        })?;
        let response = self
            .client
            .sign(
                &self.sign_path(),
                SignRequest {
                    csr: request.csr_pem.clone(),
                    common_name,
                    alt_names: request.dns_names.join(","),
                    ttl: format!("{ttl_seconds}s"),
                    format: "pem",
                    exclude_cn_from_sans: true,
                },
            )
            .await
            .map_err(map_client_error)?;
        material_from_response(&request, response)
    }

    async fn revoke(
        &self,
        certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        let serial_number = certificate
            .material
            .as_ref()
            .map(|material| material.serial_number.as_str())
            .ok_or_else(|| {
                GatewayCertificateAuthorityError::InvalidRequest(
                    "Gateway certificate has no issued material".into(),
                )
            })?;
        if parse_serial(serial_number).is_none() {
            return Err(GatewayCertificateAuthorityError::InvalidRequest(
                "Gateway certificate serial number is invalid".into(),
            ));
        }
        self.client
            .revoke(&self.revoke_path(), serial_number)
            .await
            .map_err(map_client_error)
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        self.client.health().await.map_err(map_client_error)
    }
}

#[async_trait]
trait GatewayPkiClient: Send + Sync {
    async fn sign(
        &self,
        path: &str,
        request: SignRequest,
    ) -> Result<SignResponse, VaultClientError>;

    async fn revoke(&self, path: &str, serial_number: &str) -> Result<(), VaultClientError>;

    async fn health(&self) -> Result<bool, VaultClientError>;
}

struct HttpGatewayPkiClient {
    client: VaultClient,
}

#[async_trait]
impl GatewayPkiClient for HttpGatewayPkiClient {
    async fn sign(
        &self,
        path: &str,
        request: SignRequest,
    ) -> Result<SignResponse, VaultClientError> {
        self.client.post(path, &request).await
    }

    async fn revoke(&self, path: &str, serial_number: &str) -> Result<(), VaultClientError> {
        let _: serde_json::Value = self
            .client
            .post(path, &RevokeRequest { serial_number })
            .await?;
        Ok(())
    }

    async fn health(&self) -> Result<bool, VaultClientError> {
        self.client.health().await
    }
}

#[derive(Serialize)]
struct SignRequest {
    csr: String,
    common_name: String,
    alt_names: String,
    ttl: String,
    format: &'static str,
    exclude_cn_from_sans: bool,
}

#[derive(Clone, Deserialize)]
struct SignResponse {
    certificate: String,
    #[serde(default)]
    issuing_ca: String,
    #[serde(default)]
    ca_chain: Vec<String>,
    serial_number: String,
}

#[derive(Serialize)]
struct RevokeRequest<'a> {
    serial_number: &'a str,
}

fn material_from_response(
    request: &GatewayCertificateIssueRequest,
    response: SignResponse,
) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
    let certificate_pem = normalize_pem(&response.certificate)?;
    let certificates = parse_certificates(&certificate_pem, "leaf certificate")?;
    if certificates.len() != 1 {
        return Err(invalid_response(
            "an unexpected Gateway leaf certificate chain",
        ));
    }
    let leaf = &certificates[0];
    let params = CertificateParams::from_ca_cert_der(leaf)
        .map_err(|_| invalid_response("an invalid Gateway leaf certificate"))?;
    if !matches!(params.is_ca, IsCa::NoCa | IsCa::ExplicitNoCa)
        || params.extended_key_usages != vec![ExtendedKeyUsagePurpose::ServerAuth]
        || !has_exact_dns_names(&params.subject_alt_names, &request.dns_names)
    {
        return Err(invalid_response(
            "a Gateway certificate with the wrong SAN or usage policy",
        ));
    }
    let serial = params
        .serial_number
        .ok_or_else(|| invalid_response("a Gateway certificate without a serial number"))?;
    if parse_serial(&response.serial_number).as_deref() != Some(serial.as_ref()) {
        return Err(invalid_response(
            "a serial number that does not match the Gateway certificate",
        ));
    }
    let issued_at = timestamp(params.not_before.unix_timestamp())?;
    let expires_at = timestamp(params.not_after.unix_timestamp())?;
    let provider_skew = ChronoDuration::seconds(MAX_PROVIDER_CLOCK_SKEW_SECONDS);
    if issued_at > request.issued_at
        || issued_at < request.issued_at - provider_skew
        || expires_at <= request.issued_at
        || expires_at > request.expires_at + provider_skew
    {
        return Err(invalid_response(
            "Gateway certificate validity outside the requested policy",
        ));
    }

    let ca_bundle_pem = ca_bundle(&response)?;
    let authorities = parse_certificates(&ca_bundle_pem, "CA bundle")?;
    if authorities.is_empty()
        || authorities.iter().any(|certificate| {
            CertificateParams::from_ca_cert_der(certificate)
                .map(|params| !matches!(params.is_ca, IsCa::Ca(_)))
                .unwrap_or(true)
        })
    {
        return Err(invalid_response("an invalid Gateway CA bundle"));
    }
    let material = GatewayCertificateMaterial {
        serial_number: serial.to_string(),
        fingerprint: format!("sha256:{:x}", Sha256::digest(leaf.as_ref())),
        certificate_pem,
        ca_bundle_pem,
        issued_at,
        expires_at,
    };
    material
        .validate()
        .map_err(|_| invalid_response("invalid Gateway certificate material"))?;
    Ok(material)
}

fn ca_bundle(response: &SignResponse) -> Result<String, GatewayCertificateAuthorityError> {
    let certificates = if response.ca_chain.is_empty() {
        std::slice::from_ref(&response.issuing_ca)
    } else {
        response.ca_chain.as_slice()
    };
    let mut bundle = String::new();
    for certificate in certificates {
        bundle.push_str(&normalize_pem(certificate)?);
    }
    Ok(bundle)
}

fn normalize_pem(value: &str) -> Result<String, GatewayCertificateAuthorityError> {
    if value.is_empty() || value.len() > 256 * 1024 || value.contains('\0') {
        return Err(invalid_response("an invalid PEM value"));
    }
    let value = value.replace("\r\n", "\n");
    Ok(format!("{}\n", value.trim()))
}

fn parse_certificates(
    pem: &str,
    label: &str,
) -> Result<Vec<CertificateDer<'static>>, GatewayCertificateAuthorityError> {
    rustls_pemfile::certs(&mut BufReader::new(pem.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid_response(&format!("an invalid Gateway {label}")))
}

fn has_exact_dns_names(subject_alt_names: &[SanType], expected: &[String]) -> bool {
    if subject_alt_names.len() != expected.len() {
        return false;
    }
    let mut actual = Vec::with_capacity(subject_alt_names.len());
    for subject_alt_name in subject_alt_names {
        let SanType::DnsName(dns_name) = subject_alt_name else {
            return false;
        };
        actual.push(dns_name.as_str());
    }
    actual.sort_unstable();
    actual
        .iter()
        .copied()
        .zip(expected.iter().map(String::as_str))
        .all(|(actual, expected)| actual == expected)
}

fn parse_serial(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || value.len() > 512 {
        return None;
    }
    let hex = value
        .bytes()
        .filter(|byte| *byte != b':')
        .collect::<Vec<_>>();
    if hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }
    hex.chunks_exact(2)
        .map(|pair| {
            let high = hex_value(pair[0])?;
            let low = hex_value(pair[1])?;
            Some((high << 4) | low)
        })
        .collect()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn timestamp(value: i64) -> Result<chrono::DateTime<Utc>, GatewayCertificateAuthorityError> {
    Utc.timestamp_opt(value, 0)
        .single()
        .ok_or_else(|| invalid_response("an out-of-range Gateway certificate timestamp"))
}

fn validate_segment(
    label: &str,
    value: String,
) -> Result<String, GatewayCertificateAuthorityError> {
    if value.is_empty()
        || value.len() > 255
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    {
        return Err(GatewayCertificateAuthorityError::InvalidRequest(format!(
            "{label} is invalid"
        )));
    }
    Ok(value)
}

fn map_client_error(error: VaultClientError) -> GatewayCertificateAuthorityError {
    match error {
        VaultClientError::Configuration(_) => GatewayCertificateAuthorityError::InvalidRequest(
            "Vault Gateway PKI configuration is invalid".into(),
        ),
        VaultClientError::Rejected(_) => GatewayCertificateAuthorityError::Rejected(
            "Vault Gateway PKI rejected the request".into(),
        ),
        VaultClientError::Unavailable(_) => GatewayCertificateAuthorityError::Unavailable(
            "Vault Gateway PKI is temporarily unavailable".into(),
        ),
    }
}

fn invalid_response(reason: &str) -> GatewayCertificateAuthorityError {
    GatewayCertificateAuthorityError::Rejected(format!("Vault Gateway PKI returned {reason}"))
}

#[cfg(test)]
#[path = "vault_gateway_certificate_authority_tests.rs"]
mod tests;
