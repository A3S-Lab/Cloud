use crate::modules::identity::domain::entities::{
    RecipientContactVerification, RecipientContactVerificationClaims,
    RecipientContactVerificationStatus,
};
use crate::modules::identity::domain::services::{
    IRecipientContactProofService, RecipientContactProofError,
};
use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
use crate::modules::shared_kernel::domain::canonical_timestamp;
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use fs2::FileExt;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

const PROOF_PREFIX: &str = "a3srcv1";
const MINIMUM_SECRET_BYTES: usize = 32;
const MAXIMUM_SECRET_BYTES: usize = 512;
const MAXIMUM_PROOF_BYTES: usize = 4096;
const LOCAL_SECRET_BYTES: usize = 32;

pub struct HmacRecipientContactProofService {
    key_id: RecipientContactSigningKeyId,
    secret: Zeroizing<Vec<u8>>,
}

impl HmacRecipientContactProofService {
    pub fn new(
        key_id: RecipientContactSigningKeyId,
        secret: Zeroizing<Vec<u8>>,
    ) -> Result<Self, String> {
        if !(MINIMUM_SECRET_BYTES..=MAXIMUM_SECRET_BYTES).contains(&secret.len()) {
            return Err("recipient contact proof secret must contain 32 to 512 bytes".into());
        }
        Ok(Self { key_id, secret })
    }

    pub fn load_or_create(
        key_id: RecipientContactSigningKeyId,
        key_path: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let secret = load_or_create_local_secret(key_path.as_ref())?;
        Self::new(key_id, secret)
    }

    fn mac(&self) -> Result<HmacSha256, RecipientContactProofError> {
        HmacSha256::new_from_slice(&self.secret)
            .map_err(|_| RecipientContactProofError::Unavailable)
    }
}

impl fmt::Debug for HmacRecipientContactProofService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HmacRecipientContactProofService")
            .field("key_id", &self.key_id)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

#[async_trait]
impl IRecipientContactProofService for HmacRecipientContactProofService {
    fn current_key_id(&self) -> &RecipientContactSigningKeyId {
        &self.key_id
    }

    async fn issue(
        &self,
        verification: &RecipientContactVerification,
    ) -> Result<Zeroizing<String>, RecipientContactProofError> {
        let signing_input = proof_signing_input(verification, &self.key_id)?;
        let mut mac = self.mac()?;
        mac.update(signing_input.as_bytes());
        let signature_bytes = Zeroizing::new(mac.finalize().into_bytes().to_vec());
        let signature = Zeroizing::new(URL_SAFE_NO_PAD.encode(signature_bytes.as_slice()));
        proof_with_authenticator(&signing_input, signature.as_str())
    }

    async fn verify(
        &self,
        proof: &str,
        now: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationClaims, RecipientContactProofError> {
        let parsed = parse_proof(proof)?;
        let signature = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(parsed.authenticator)
                .map_err(|_| RecipientContactProofError::Rejected)?,
        );
        let mut mac = self.mac()?;
        mac.update(parsed.signing_input.as_bytes());
        mac.verify_slice(signature.as_slice())
            .map_err(|_| RecipientContactProofError::Rejected)?;
        decode_claims(parsed.payload, &self.key_id, now)
    }
}

pub(super) struct ParsedRecipientContactProof<'a> {
    pub(super) payload: &'a str,
    pub(super) authenticator: &'a str,
    pub(super) signing_input: String,
}

pub(super) fn proof_signing_input(
    verification: &RecipientContactVerification,
    key_id: &RecipientContactSigningKeyId,
) -> Result<String, RecipientContactProofError> {
    verification
        .validate()
        .map_err(|_| RecipientContactProofError::Rejected)?;
    if &verification.signing_key_id != key_id
        || verification.status_at(verification.issued_at)
            != RecipientContactVerificationStatus::Pending
    {
        return Err(RecipientContactProofError::Rejected);
    }
    let payload = serde_json::to_vec(&verification.claims())
        .map_err(|_| RecipientContactProofError::Unavailable)?;
    Ok(format!(
        "{PROOF_PREFIX}.{}",
        URL_SAFE_NO_PAD.encode(payload)
    ))
}

pub(super) fn proof_with_authenticator(
    signing_input: &str,
    authenticator: &str,
) -> Result<Zeroizing<String>, RecipientContactProofError> {
    if authenticator.is_empty()
        || authenticator.len() > MAXIMUM_PROOF_BYTES
        || authenticator.contains(['\0', '\r', '\n', '.'])
    {
        return Err(RecipientContactProofError::Rejected);
    }
    let proof = Zeroizing::new(format!("{signing_input}.{authenticator}"));
    if proof.len() > MAXIMUM_PROOF_BYTES {
        return Err(RecipientContactProofError::Rejected);
    }
    Ok(proof)
}

pub(super) fn parse_proof(
    proof: &str,
) -> Result<ParsedRecipientContactProof<'_>, RecipientContactProofError> {
    if proof.is_empty() || proof.len() > MAXIMUM_PROOF_BYTES || proof.contains(['\0', '\r', '\n']) {
        return Err(RecipientContactProofError::Rejected);
    }
    let mut parts = proof.split('.');
    let prefix = parts.next();
    let payload = parts.next().filter(|value| !value.is_empty());
    let authenticator = parts.next().filter(|value| !value.is_empty());
    if prefix != Some(PROOF_PREFIX) || parts.next().is_some() {
        return Err(RecipientContactProofError::Rejected);
    }
    let payload = payload.ok_or(RecipientContactProofError::Rejected)?;
    let authenticator = authenticator.ok_or(RecipientContactProofError::Rejected)?;
    Ok(ParsedRecipientContactProof {
        payload,
        authenticator,
        signing_input: format!("{PROOF_PREFIX}.{payload}"),
    })
}

pub(super) fn decode_claims(
    payload: &str,
    key_id: &RecipientContactSigningKeyId,
    now: DateTime<Utc>,
) -> Result<RecipientContactVerificationClaims, RecipientContactProofError> {
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| RecipientContactProofError::Rejected)?;
    let claims = serde_json::from_slice::<RecipientContactVerificationClaims>(&payload)
        .map_err(|_| RecipientContactProofError::Rejected)?;
    claims
        .validate()
        .map_err(|_| RecipientContactProofError::Rejected)?;
    let now = canonical_timestamp(now);
    if &claims.signing_key_id != key_id || now < claims.issued_at || now >= claims.expires_at {
        return Err(RecipientContactProofError::Rejected);
    }
    Ok(claims)
}

fn load_or_create_local_secret(path: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    validate_local_key_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| "recipient contact proof key path has no parent directory".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| {
        format!("could not create recipient contact proof key directory: {error}")
    })?;
    secure_local_directory(parent)?;
    reject_local_key_symlink(path)?;

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("could not open recipient contact proof key: {error}"))?;
    FileExt::lock_exclusive(&file)
        .map_err(|error| format!("could not lock recipient contact proof key: {error}"))?;
    let result = validate_local_key_metadata(path)
        .and_then(|()| load_or_initialize_locked_secret(&mut file, parent));
    let unlock = FileExt::unlock(&file)
        .map_err(|error| format!("could not unlock recipient contact proof key: {error}"));
    match (result, unlock) {
        (Ok(secret), Ok(())) => Ok(secret),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn validate_local_key_path(path: &Path) -> Result<(), String> {
    let value = path
        .to_str()
        .ok_or_else(|| "recipient contact proof key path must be valid UTF-8".to_owned())?;
    if value.trim().is_empty()
        || value.len() > 4096
        || value.contains(['\0', '\r', '\n'])
        || path.file_name().is_none()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("recipient contact proof key path is invalid".into());
    }
    Ok(())
}

fn reject_local_key_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("recipient contact proof key must not be a symbolic link".into())
        }
        Ok(metadata) if !metadata.is_file() => {
            Err("recipient contact proof key is not a regular file".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "could not inspect recipient contact proof key: {error}"
        )),
    }
}

fn load_or_initialize_locked_secret(
    file: &mut File,
    parent: &Path,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let metadata = file
        .metadata()
        .map_err(|error| format!("could not inspect recipient contact proof key: {error}"))?;
    if !metadata.is_file() {
        return Err("recipient contact proof key is not a regular file".into());
    }
    if metadata.len() == 0 {
        let mut secret = Zeroizing::new(vec![0_u8; LOCAL_SECRET_BYTES]);
        getrandom::fill(secret.as_mut_slice())
            .map_err(|error| format!("could not generate recipient contact proof key: {error}"))?;
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.write_all(secret.as_slice()))
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("could not persist recipient contact proof key: {error}"))?;
        sync_local_key_directory(parent)?;
        return Ok(secret);
    }
    if metadata.len() != LOCAL_SECRET_BYTES as u64 {
        return Err("recipient contact proof key must contain exactly 32 bytes".into());
    }
    let mut secret = Zeroizing::new(Vec::with_capacity(LOCAL_SECRET_BYTES));
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("could not seek recipient contact proof key: {error}"))?;
    {
        let mut bounded = (&mut *file).take(LOCAL_SECRET_BYTES as u64 + 1);
        bounded
            .read_to_end(&mut secret)
            .map_err(|error| format!("could not read recipient contact proof key: {error}"))?;
    }
    if secret.len() != LOCAL_SECRET_BYTES {
        return Err("recipient contact proof key must contain exactly 32 bytes".into());
    }
    Ok(secret)
}

fn sync_local_key_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                format!("could not sync recipient contact proof key directory: {error}")
            })?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn secure_local_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!("could not inspect recipient contact proof key directory: {error}")
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("recipient contact proof key directory is not an owned directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!("could not secure recipient contact proof key directory: {error}")
        })?;
    }
    Ok(())
}

fn validate_local_key_metadata(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect recipient contact proof key: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("recipient contact proof key is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("recipient contact proof key permissions must be 0600 or stricter".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
    use crate::modules::shared_kernel::domain::{
        PrincipalId, RecipientContactId, RecipientContactVerificationId, Sha256Digest,
    };
    use chrono::Duration;

    fn service(key: &str, secret: u8) -> HmacRecipientContactProofService {
        HmacRecipientContactProofService::new(
            RecipientContactSigningKeyId::parse(key).expect("key ID"),
            Zeroizing::new(vec![secret; 32]),
        )
        .expect("proof service")
    }

    fn verification(now: DateTime<Utc>) -> RecipientContactVerification {
        RecipientContactVerification::create(
            RecipientContactVerificationId::new(),
            RecipientContactId::new(),
            PrincipalId::new(),
            Sha256Digest::from_bytes(b"mailbox"),
            3,
            RecipientContactSigningKeyId::parse("contact-v1").expect("key ID"),
            now,
            now + Duration::minutes(10),
        )
        .expect("verification")
    }

    #[tokio::test]
    async fn proof_binds_every_claim_and_rejects_tampering_expiry_and_stale_keys() {
        let now = canonical_timestamp(Utc::now());
        let current = service("contact-v1", 7);
        let verification = verification(now);
        let proof = current.issue(&verification).await.expect("proof");
        assert_eq!(
            current
                .verify(&proof, now + Duration::minutes(1))
                .await
                .expect("verified proof"),
            verification.claims()
        );

        let mut tampered = proof.to_string();
        tampered.push('x');
        assert_eq!(
            current.verify(&tampered, now + Duration::minutes(1)).await,
            Err(RecipientContactProofError::Rejected)
        );
        assert_eq!(
            current.verify(&proof, verification.expires_at).await,
            Err(RecipientContactProofError::Rejected)
        );
        assert_eq!(
            service("contact-v2", 9)
                .verify(&proof, now + Duration::minutes(1))
                .await,
            Err(RecipientContactProofError::Rejected)
        );
    }

    #[test]
    fn proof_material_is_redacted_and_secret_length_is_bounded() {
        let service = service("contact-v1", 5);
        assert!(!format!("{service:?}").contains(&"5".repeat(32)));
        assert!(HmacRecipientContactProofService::new(
            RecipientContactSigningKeyId::parse("contact-v1").expect("key ID"),
            Zeroizing::new(vec![0; 31]),
        )
        .is_err());
    }

    #[tokio::test]
    async fn local_key_is_private_restart_stable_and_never_rendered() {
        let root = tempfile::tempdir().expect("temporary directory");
        let key_path = root.path().join("recipient-contact/proof-hmac.key");
        let key_id = RecipientContactSigningKeyId::parse("contact-v1").expect("key ID");
        let first = HmacRecipientContactProofService::load_or_create(key_id.clone(), &key_path)
            .expect("first local proof service");
        let now = canonical_timestamp(Utc::now());
        let proof = first.issue(&verification(now)).await.expect("issued proof");
        let second = HmacRecipientContactProofService::load_or_create(key_id, &key_path)
            .expect("restarted local proof service");
        second
            .verify(&proof, now + Duration::minutes(1))
            .await
            .expect("proof survives restart");
        assert_eq!(fs::metadata(&key_path).expect("key metadata").len(), 32);
        assert!(!format!("{second:?}").contains(
            &fs::read(&key_path)
                .expect("key bytes")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&key_path)
                    .expect("key metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn concurrent_local_startup_converges_on_one_complete_key() {
        let root = tempfile::tempdir().expect("temporary directory");
        let key_path = root.path().join("recipient-contact/proof-hmac.key");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let services = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..16 {
                let barrier = barrier.clone();
                let key_path = key_path.clone();
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    HmacRecipientContactProofService::load_or_create(
                        RecipientContactSigningKeyId::parse("contact-v1").expect("key ID"),
                        key_path,
                    )
                    .expect("concurrent local proof service")
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join().expect("startup thread"))
                .collect::<Vec<_>>()
        });
        let now = canonical_timestamp(Utc::now());
        let proof = services[0]
            .issue(&verification(now))
            .await
            .expect("issued proof");
        for service in services.iter().skip(1) {
            service
                .verify(&proof, now + Duration::minutes(1))
                .await
                .expect("all startups share one complete key");
        }
        assert_eq!(fs::metadata(key_path).expect("key metadata").len(), 32);
    }
}
