use super::{LocalKeyEncryptionService, VaultCertificateAuthority, VaultKeyEncryptionService};
use crate::modules::fleet::domain::services::{EncryptedValue, IKeyEncryptionService};
use std::time::Duration;

#[tokio::test]
async fn local_key_encryption_is_persistent_authenticated_and_context_bound() {
    let directory = tempfile::tempdir().expect("key directory");
    let path = directory.path().join("master.key");
    let service = LocalKeyEncryptionService::load_or_create(&path).expect("local encryption");
    let encrypted = service
        .encrypt(b"node credential", b"node:one")
        .await
        .expect("encrypt");
    assert!(!encrypted.ciphertext.contains("node credential"));
    assert_eq!(
        service
            .decrypt(&encrypted, b"node:one")
            .await
            .expect("decrypt"),
        b"node credential"
    );
    assert!(service.decrypt(&encrypted, b"node:two").await.is_err());

    let reopened = LocalKeyEncryptionService::load_or_create(&path).expect("reopen key");
    assert_eq!(
        reopened
            .decrypt(&encrypted, b"node:one")
            .await
            .expect("decrypt after reopen"),
        b"node credential"
    );
    let mut tampered = encrypted;
    tampered.ciphertext.push('x');
    assert!(reopened.decrypt(&tampered, b"node:one").await.is_err());
    assert!(reopened
        .decrypt(
            &EncryptedValue {
                key_id: "different".into(),
                ciphertext: tampered.ciphertext,
            },
            b"node:one"
        )
        .await
        .is_err());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .expect("key metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn vault_integrations_require_https_and_closed_provider_names() {
    let timeout = Duration::from_secs(1);
    assert!(
        VaultCertificateAuthority::new("http://vault:8200", "token", "pki", "node", timeout)
            .is_err()
    );
    assert!(VaultCertificateAuthority::new(
        "https://vault.example",
        "token",
        "../pki",
        "node",
        timeout
    )
    .is_err());
    assert!(VaultKeyEncryptionService::new(
        "https://vault.example",
        "token",
        "transit",
        "node-key",
        timeout
    )
    .is_ok());
}
