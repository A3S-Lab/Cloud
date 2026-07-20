use super::*;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, GatewayCertificateId, NodeCommandId, NodeId, OrganizationId,
};
use a3s_cloud_contracts::GatewayCertificateRequest;
use chrono::Duration;
use rcgen::{
    BasicConstraints, CertificateSigningRequestParams, DistinguishedName, DnType, KeyPair,
    KeyUsagePurpose, SerialNumber,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct ObservedSign {
    path: String,
    common_name: String,
    alt_names: String,
    ttl: String,
    format: &'static str,
    exclude_cn_from_sans: bool,
    csr_contains_private_key: bool,
}

struct RecordingGatewayPkiClient {
    response: SignResponse,
    reject_signing: AtomicBool,
    observed_sign: Mutex<Option<ObservedSign>>,
    observed_revocation: Mutex<Option<(String, String)>>,
}

#[async_trait]
impl GatewayPkiClient for RecordingGatewayPkiClient {
    async fn sign(
        &self,
        path: &str,
        request: SignRequest,
    ) -> Result<SignResponse, VaultClientError> {
        *self.observed_sign.lock().expect("sign observation") = Some(ObservedSign {
            path: path.into(),
            common_name: request.common_name,
            alt_names: request.alt_names,
            ttl: request.ttl,
            format: request.format,
            exclude_cn_from_sans: request.exclude_cn_from_sans,
            csr_contains_private_key: request.csr.contains("PRIVATE KEY"),
        });
        if self.reject_signing.load(Ordering::SeqCst) {
            return Err(VaultClientError::Rejected(
                "provider-internal policy detail must not escape".into(),
            ));
        }
        Ok(self.response.clone())
    }

    async fn revoke(&self, path: &str, serial_number: &str) -> Result<(), VaultClientError> {
        *self
            .observed_revocation
            .lock()
            .expect("revocation observation") = Some((path.into(), serial_number.into()));
        Ok(())
    }

    async fn health(&self) -> Result<bool, VaultClientError> {
        Ok(true)
    }
}

#[tokio::test]
async fn vault_gateway_ca_issues_exact_server_identity_with_provider_serial_and_validity() {
    let (request, response) = fixture(vec!["api.example.net".into(), "*.example.com".into()]);
    let client = Arc::new(RecordingGatewayPkiClient {
        response,
        reject_signing: AtomicBool::new(false),
        observed_sign: Mutex::new(None),
        observed_revocation: Mutex::new(None),
    });
    let authority = VaultGatewayCertificateAuthority::with_client(
        client.clone(),
        "gateway-pki",
        "a3s-cloud-gateway",
    )
    .expect("Vault Gateway CA");

    let material = authority
        .issue(request.clone())
        .await
        .expect("Vault Gateway certificate");
    assert_eq!(material.serial_number, "01:23:45:67:89");
    assert_eq!(
        material.issued_at,
        request.issued_at - Duration::seconds(30)
    );
    assert_eq!(
        material.expires_at,
        request.expires_at + Duration::seconds(30)
    );
    assert!(!material.certificate_pem.contains("PRIVATE KEY"));
    assert!(!material.ca_bundle_pem.contains("PRIVATE KEY"));

    let observed = client
        .observed_sign
        .lock()
        .expect("sign observation")
        .take()
        .expect("sign request");
    assert_eq!(observed.path, "gateway-pki/sign/a3s-cloud-gateway");
    assert_eq!(observed.common_name, "*.example.com");
    assert_eq!(observed.alt_names, "*.example.com,api.example.net");
    assert_eq!(observed.ttl, "2592000s");
    assert_eq!(observed.format, "pem");
    assert!(observed.exclude_cn_from_sans);
    assert!(!observed.csr_contains_private_key);
    assert!(authority.health().await.expect("Vault health"));

    let mut certificate = GatewayCertificate::provision(
        request.certificate_id,
        OrganizationId::new(),
        request.node_id,
        vec![DomainClaimId::new()],
        1,
        NodeCommandId::new(),
        format!("sha256:{}", "a".repeat(64)),
        GatewayCertificateRequest::new(
            request.certificate_id.as_uuid(),
            request.dns_names,
            "/managed/certificate.pem",
            "/managed/private-key.pem",
        )
        .expect("certificate request"),
        request.issued_at,
    )
    .expect("Gateway certificate");
    certificate
        .record_issued(
            format!("sha256:{}", "b".repeat(64)),
            material.clone(),
            request.issued_at,
        )
        .expect("issued material");
    authority
        .revoke(&certificate)
        .await
        .expect("revoke Gateway certificate");
    assert_eq!(
        client
            .observed_revocation
            .lock()
            .expect("revocation observation")
            .as_ref(),
        Some(&("gateway-pki/revoke".into(), material.serial_number.clone()))
    );
}

#[tokio::test]
async fn vault_gateway_ca_rejects_changed_identity_and_sanitizes_provider_failures() {
    let (request, response) = fixture(vec!["attacker.example.com".into()]);
    let client = Arc::new(RecordingGatewayPkiClient {
        response,
        reject_signing: AtomicBool::new(false),
        observed_sign: Mutex::new(None),
        observed_revocation: Mutex::new(None),
    });
    let authority = VaultGatewayCertificateAuthority::with_client(
        client.clone(),
        "gateway-pki",
        "a3s-cloud-gateway",
    )
    .expect("Vault Gateway CA");

    assert!(matches!(
        authority
            .issue(request.clone())
            .await
            .expect_err("changed certificate identity"),
        GatewayCertificateAuthorityError::Rejected(_)
    ));

    client.reject_signing.store(true, Ordering::SeqCst);
    let error = authority
        .issue(request)
        .await
        .expect_err("provider rejection");
    assert!(matches!(
        error,
        GatewayCertificateAuthorityError::Rejected(ref message)
            if !message.contains("provider-internal")
    ));
}

#[test]
fn vault_gateway_ca_rejects_mismatched_serial_and_non_ca_bundle() {
    let (request, mut wrong_serial) =
        fixture(vec!["api.example.net".into(), "*.example.com".into()]);
    wrong_serial.serial_number = "00:00".into();
    assert!(matches!(
        material_from_response(&request, wrong_serial).expect_err("mismatched serial"),
        GatewayCertificateAuthorityError::Rejected(_)
    ));

    let (request, mut wrong_bundle) =
        fixture(vec!["api.example.net".into(), "*.example.com".into()]);
    wrong_bundle.ca_chain = vec![wrong_bundle.certificate.clone()];
    assert!(matches!(
        material_from_response(&request, wrong_bundle).expect_err("non-CA bundle"),
        GatewayCertificateAuthorityError::Rejected(_)
    ));
}

#[test]
fn vault_gateway_ca_requires_https_without_url_credentials_and_closed_names() {
    let timeout = std::time::Duration::from_secs(1);
    assert!(VaultGatewayCertificateAuthority::new(
        "http://vault:8200",
        "token",
        "gateway-pki",
        "gateway",
        timeout
    )
    .is_err());
    assert!(VaultGatewayCertificateAuthority::new(
        "https://user:password@vault.example",
        "token",
        "gateway-pki",
        "gateway",
        timeout
    )
    .is_err());
    assert!(VaultGatewayCertificateAuthority::new(
        "https://vault.example",
        "token",
        "../gateway-pki",
        "gateway",
        timeout
    )
    .is_err());
}

fn fixture(actual_dns_names: Vec<String>) -> (GatewayCertificateIssueRequest, SignResponse) {
    let issued_at = Utc
        .with_ymd_and_hms(2026, 7, 20, 12, 0, 0)
        .single()
        .expect("issue timestamp");
    let expires_at = issued_at + Duration::days(30);
    let requested_dns_names = vec!["*.example.com".into(), "api.example.net".into()];
    let gateway_key = KeyPair::generate().expect("Gateway key");
    let mut request_params =
        CertificateParams::new(requested_dns_names.clone()).expect("request parameters");
    let mut request_name = DistinguishedName::new();
    request_name.push(DnType::CommonName, "a3s-gateway:test");
    request_params.distinguished_name = request_name;
    let csr_pem = request_params
        .serialize_request(&gateway_key)
        .expect("Gateway CSR")
        .pem()
        .expect("Gateway CSR PEM");
    let request = GatewayCertificateIssueRequest {
        certificate_id: GatewayCertificateId::new(),
        node_id: NodeId::new(),
        dns_names: requested_dns_names,
        csr_pem: csr_pem.clone(),
        issued_at,
        expires_at,
    };

    let ca_key = KeyPair::generate().expect("Vault fixture CA key");
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let ca = ca_params.self_signed(&ca_key).expect("Vault fixture CA");
    let mut certificate =
        CertificateSigningRequestParams::from_pem(&csr_pem).expect("parse Gateway CSR");
    let serial = SerialNumber::from_slice(&[0x01, 0x23, 0x45, 0x67, 0x89]);
    certificate.params.serial_number = Some(serial.clone());
    certificate.params.is_ca = IsCa::ExplicitNoCa;
    certificate.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    certificate.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    certificate.params.subject_alt_names = actual_dns_names
        .into_iter()
        .map(|dns_name| {
            dns_name
                .try_into()
                .map(SanType::DnsName)
                .expect("fixture DNS name")
        })
        .collect();
    certificate.params.not_before = offset_time(issued_at - Duration::seconds(30));
    certificate.params.not_after = offset_time(expires_at + Duration::seconds(30));
    let leaf = certificate
        .signed_by(&ca, &ca_key)
        .expect("Vault fixture certificate");
    (
        request,
        SignResponse {
            certificate: leaf.pem(),
            issuing_ca: String::new(),
            ca_chain: vec![ca.pem()],
            serial_number: serial.to_string().to_uppercase(),
        },
    )
}

fn offset_time(value: chrono::DateTime<Utc>) -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(value.timestamp()).expect("fixture timestamp")
}
