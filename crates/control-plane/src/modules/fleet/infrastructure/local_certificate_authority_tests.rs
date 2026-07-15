use super::LocalCertificateAuthority;
use crate::modules::fleet::domain::services::{ICertificateAuthority, NodeCertificateRequest};
use crate::modules::shared_kernel::domain::{NodeCertificateId, NodeId};
use chrono::{Duration, Utc};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};

fn csr() -> String {
    let key = KeyPair::generate().expect("node private key");
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "untrusted-request-name");
    params.distinguished_name = name;
    params
        .serialize_request(&key)
        .expect("CSR")
        .pem()
        .expect("CSR PEM")
}

#[tokio::test]
async fn local_ca_persists_identity_issues_client_certificates_and_records_revocation() {
    let directory = tempfile::tempdir().expect("temp CA directory");
    let authority = LocalCertificateAuthority::load_or_create(directory.path()).expect("local CA");
    assert!(authority.health().await.expect("CA health"));
    let node_id = NodeId::new();
    let issued_at = Utc::now();
    let certificate = authority
        .issue(NodeCertificateRequest {
            certificate_id: NodeCertificateId::new(),
            node_id,
            csr_pem: csr(),
            issued_at,
            expires_at: issued_at + Duration::minutes(30),
        })
        .await
        .expect("issue node certificate");
    assert_eq!(certificate.node_id, node_id);
    assert!(certificate
        .certificate_pem
        .starts_with("-----BEGIN CERTIFICATE-----"));
    assert!(certificate.fingerprint.starts_with("sha256:"));

    let reopened = LocalCertificateAuthority::load_or_create(directory.path()).expect("reopen CA");
    let second = reopened
        .issue(NodeCertificateRequest {
            certificate_id: NodeCertificateId::new(),
            node_id,
            csr_pem: csr(),
            issued_at,
            expires_at: issued_at + Duration::minutes(30),
        })
        .await
        .expect("issue from reopened CA");
    assert_eq!(certificate.ca_bundle_pem, second.ca_bundle_pem);

    reopened.revoke(&certificate).await.expect("revoke");
    reopened
        .revoke(&certificate)
        .await
        .expect("idempotent revoke");
    let revocations =
        std::fs::read_to_string(directory.path().join("revoked-serials")).expect("revocations");
    assert_eq!(revocations.lines().count(), 1);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(directory.path().join("ca-key.pem"))
            .expect("key metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[tokio::test]
async fn local_ca_rejects_malformed_or_expired_requests() {
    let directory = tempfile::tempdir().expect("temp CA directory");
    let authority = LocalCertificateAuthority::load_or_create(directory.path()).expect("local CA");
    let now = Utc::now();
    assert!(authority
        .issue(NodeCertificateRequest {
            certificate_id: NodeCertificateId::new(),
            node_id: NodeId::new(),
            csr_pem: "not a CSR".into(),
            issued_at: now,
            expires_at: now + Duration::minutes(1),
        })
        .await
        .is_err());
    assert!(authority
        .issue(NodeCertificateRequest {
            certificate_id: NodeCertificateId::new(),
            node_id: NodeId::new(),
            csr_pem: csr(),
            issued_at: now,
            expires_at: now,
        })
        .await
        .is_err());
}

#[test]
fn local_ca_materializes_an_idempotent_node_control_identity() {
    let directory = tempfile::tempdir().expect("temp CA directory");
    let authority = LocalCertificateAuthority::load_or_create(directory.path().join("authority"))
        .expect("local CA");
    let tls = directory.path().join("tls");
    let certificate_path = tls.join("server.pem");
    let key_path = tls.join("server-key.pem");
    let bundle_path = tls.join("client-ca.pem");

    authority
        .ensure_ca_bundle(&bundle_path)
        .expect("client CA bundle");
    authority
        .ensure_server_identity("localhost", &certificate_path, &key_path)
        .expect("server identity");
    let first_certificate = std::fs::read_to_string(&certificate_path).expect("certificate");
    let first_key = std::fs::read_to_string(&key_path).expect("private key");
    assert!(first_certificate.starts_with("-----BEGIN CERTIFICATE-----"));
    assert!(first_key.starts_with("-----BEGIN PRIVATE KEY-----"));

    authority
        .ensure_ca_bundle(&bundle_path)
        .expect("idempotent client CA bundle");
    authority
        .ensure_server_identity("localhost", &certificate_path, &key_path)
        .expect("idempotent server identity");
    assert_eq!(
        std::fs::read_to_string(&certificate_path).expect("certificate replay"),
        first_certificate
    );
    assert_eq!(
        std::fs::read_to_string(&key_path).expect("private key replay"),
        first_key
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let key_mode = std::fs::metadata(&key_path)
            .expect("key metadata")
            .permissions()
            .mode()
            & 0o777;
        let certificate_mode = std::fs::metadata(&certificate_path)
            .expect("certificate metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(key_mode, 0o600);
        assert_eq!(certificate_mode, 0o644);
    }
}

#[test]
fn local_ca_rejects_a_conflicting_node_control_bundle_or_partial_identity() {
    let directory = tempfile::tempdir().expect("temp CA directory");
    let authority = LocalCertificateAuthority::load_or_create(directory.path().join("authority"))
        .expect("local CA");
    let bundle_path = directory.path().join("client-ca.pem");
    std::fs::write(&bundle_path, "different CA").expect("conflicting bundle");
    assert!(authority.ensure_ca_bundle(&bundle_path).is_err());

    let certificate_path = directory.path().join("server.pem");
    let key_path = directory.path().join("server-key.pem");
    std::fs::write(&key_path, "partial key").expect("partial identity");
    assert!(authority
        .ensure_server_identity("localhost", &certificate_path, &key_path)
        .is_err());
}
