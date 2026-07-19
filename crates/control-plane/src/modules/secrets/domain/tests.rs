use super::*;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, ResourceName, SecretId,
};
use chrono::{Duration, Utc};

fn encrypted(marker: &str) -> EncryptedSecretValue {
    EncryptedSecretValue::new("local:test-key", format!("v1.nonce.{marker}"))
        .expect("encrypted Secret value")
}

fn secret() -> (Secret, SecretVersion) {
    Secret::create(
        SecretId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("Database Password").expect("Secret name"),
        encrypted("one"),
        Utc::now(),
    )
    .expect("Secret")
}

#[test]
fn rotations_are_immutable_monotonic_and_context_bound() {
    let (mut secret, first) = secret();
    let second = secret
        .rotate(encrypted("two"), secret.created_at + Duration::seconds(1))
        .expect("rotated Secret");
    assert_eq!(first.version, 1);
    assert_eq!(first.encrypted_value.ciphertext(), "v1.nonce.one");
    assert_eq!(second.version, 2);
    assert_eq!(secret.current_version, 2);
    assert_eq!(secret.aggregate_version, 2);
    assert!(first.is_materializable(&secret));
    assert!(second.is_materializable(&secret));

    let first_context =
        secret_encryption_context(secret.organization_id, secret.id, 1).expect("first context");
    let second_context =
        secret_encryption_context(secret.organization_id, secret.id, 2).expect("second context");
    assert_ne!(first_context, second_context);
}

#[test]
fn revocation_is_idempotent_and_blocks_materialization() {
    let (mut secret, mut version) = secret();
    let revoked_at = secret.created_at + Duration::seconds(1);
    version.revoke(revoked_at).expect("revoke version");
    let aggregate_version = version.aggregate_version;
    version
        .revoke(revoked_at)
        .expect("replay version revocation");
    assert_eq!(version.aggregate_version, aggregate_version);
    assert!(!version.is_materializable(&secret));

    secret.revoke(revoked_at).expect("revoke Secret");
    let aggregate_version = secret.aggregate_version;
    secret.revoke(revoked_at).expect("replay Secret revocation");
    assert_eq!(secret.aggregate_version, aggregate_version);
    assert!(secret.rotate(encrypted("rejected"), revoked_at).is_err());
}

#[test]
fn debug_output_never_contains_ciphertext() {
    let (_, version) = secret();
    let debug = format!("{version:?} {:?}", version.encrypted_value);
    assert!(!debug.contains("v1.nonce.one"));
    assert!(debug.contains("<redacted-ciphertext>"));
}
