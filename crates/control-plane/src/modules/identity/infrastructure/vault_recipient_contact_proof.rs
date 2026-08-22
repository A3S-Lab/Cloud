use super::recipient_contact_proof::{
    decode_claims, parse_proof, proof_signing_input, proof_with_authenticator,
};
use crate::infrastructure::{VaultClient, VaultClientError};
use crate::modules::identity::domain::entities::{
    RecipientContactVerification, RecipientContactVerificationClaims,
};
use crate::modules::identity::domain::services::{
    IRecipientContactProofService, RecipientContactProofError,
};
use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
use async_trait::async_trait;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const MAXIMUM_VAULT_HMAC_BYTES: usize = 1024;

pub struct VaultRecipientContactProofService {
    client: Arc<dyn VaultTransitHmacClient>,
    mount: String,
    key: String,
    key_id: RecipientContactSigningKeyId,
}

impl VaultRecipientContactProofService {
    pub fn new(
        address: &str,
        token: &str,
        mount: impl Into<String>,
        key: impl Into<String>,
        key_id: RecipientContactSigningKeyId,
        timeout: Duration,
    ) -> Result<Self, String> {
        let mount = validate_segment("Vault Transit HMAC mount", mount.into())?;
        let key = validate_segment("Vault Transit HMAC key", key.into())?;
        let client =
            VaultClient::new(address, token, timeout).map_err(vault_configuration_error)?;
        Ok(Self {
            client: Arc::new(VaultTransitHttpHmacClient { client }),
            mount,
            key,
            key_id,
        })
    }

    #[cfg(test)]
    fn with_client(
        client: Arc<dyn VaultTransitHmacClient>,
        mount: &str,
        key: &str,
        key_id: RecipientContactSigningKeyId,
    ) -> Result<Self, String> {
        Ok(Self {
            client,
            mount: validate_segment("Vault Transit HMAC mount", mount.into())?,
            key: validate_segment("Vault Transit HMAC key", key.into())?,
            key_id,
        })
    }
}

impl fmt::Debug for VaultRecipientContactProofService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultRecipientContactProofService")
            .field("provider", &"vault-transit-hmac-sha2-256")
            .field("key_id", &self.key_id)
            .finish()
    }
}

#[async_trait]
impl IRecipientContactProofService for VaultRecipientContactProofService {
    fn current_key_id(&self) -> &RecipientContactSigningKeyId {
        &self.key_id
    }

    async fn issue(
        &self,
        verification: &RecipientContactVerification,
    ) -> Result<Zeroizing<String>, RecipientContactProofError> {
        let signing_input = proof_signing_input(verification, &self.key_id)?;
        let response = self
            .client
            .generate(
                &format!("{}/hmac/{}/sha2-256", self.mount, self.key),
                TransitHmacRequest {
                    input: STANDARD.encode(signing_input.as_bytes()),
                },
            )
            .await?;
        parse_vault_hmac(&response.hmac)?;
        let authenticator = Zeroizing::new(URL_SAFE_NO_PAD.encode(response.hmac.as_bytes()));
        proof_with_authenticator(&signing_input, authenticator.as_str())
    }

    async fn verify(
        &self,
        proof: &str,
        now: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationClaims, RecipientContactProofError> {
        let parsed = parse_proof(proof)?;
        let authenticator = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(parsed.authenticator)
                .map_err(|_| RecipientContactProofError::Rejected)?,
        );
        let hmac = std::str::from_utf8(authenticator.as_slice())
            .map_err(|_| RecipientContactProofError::Rejected)?;
        parse_vault_hmac(hmac)?;
        let response = self
            .client
            .verify(
                &format!("{}/verify/{}/sha2-256", self.mount, self.key),
                TransitVerifyHmacRequest {
                    input: STANDARD.encode(parsed.signing_input.as_bytes()),
                    hmac,
                },
            )
            .await?;
        if !response.valid {
            return Err(RecipientContactProofError::Rejected);
        }
        decode_claims(parsed.payload, &self.key_id, now)
    }
}

#[derive(Clone)]
struct VaultTransitHttpHmacClient {
    client: VaultClient,
}

#[async_trait]
impl VaultTransitHmacClient for VaultTransitHttpHmacClient {
    async fn generate(
        &self,
        path: &str,
        request: TransitHmacRequest,
    ) -> Result<TransitHmacResponse, RecipientContactProofError> {
        self.client
            .post(path, &request)
            .await
            .map_err(map_vault_error)
    }

    async fn verify(
        &self,
        path: &str,
        request: TransitVerifyHmacRequest<'_>,
    ) -> Result<TransitVerifyHmacResponse, RecipientContactProofError> {
        self.client
            .post(path, &request)
            .await
            .map_err(map_vault_error)
    }
}

#[async_trait]
trait VaultTransitHmacClient: Send + Sync {
    async fn generate(
        &self,
        path: &str,
        request: TransitHmacRequest,
    ) -> Result<TransitHmacResponse, RecipientContactProofError>;

    async fn verify(
        &self,
        path: &str,
        request: TransitVerifyHmacRequest<'_>,
    ) -> Result<TransitVerifyHmacResponse, RecipientContactProofError>;
}

#[derive(Clone, Serialize)]
struct TransitHmacRequest {
    input: String,
}

#[derive(Deserialize)]
struct TransitHmacResponse {
    hmac: String,
}

impl Drop for TransitHmacResponse {
    fn drop(&mut self) {
        self.hmac.zeroize();
    }
}

#[derive(Serialize)]
struct TransitVerifyHmacRequest<'a> {
    input: String,
    hmac: &'a str,
}

#[derive(Deserialize)]
struct TransitVerifyHmacResponse {
    valid: bool,
}

fn parse_vault_hmac(value: &str) -> Result<(), RecipientContactProofError> {
    if value.is_empty()
        || value.len() > MAXIMUM_VAULT_HMAC_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(RecipientContactProofError::Rejected);
    }
    let mut parts = value.split(':');
    if parts.next() != Some("vault") {
        return Err(RecipientContactProofError::Rejected);
    }
    let version = parts
        .next()
        .and_then(|value| value.strip_prefix('v'))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|version| *version > 0)
        .ok_or(RecipientContactProofError::Rejected)?;
    let encoded = parts.next().ok_or(RecipientContactProofError::Rejected)?;
    if parts.next().is_some() {
        return Err(RecipientContactProofError::Rejected);
    }
    let bytes = Zeroizing::new(
        STANDARD
            .decode(encoded)
            .map_err(|_| RecipientContactProofError::Rejected)?,
    );
    if bytes.len() != 32
        || format!("vault:v{version}:{}", STANDARD.encode(bytes.as_slice())) != value
    {
        return Err(RecipientContactProofError::Rejected);
    }
    Ok(())
}

fn validate_segment(label: &str, value: String) -> Result<String, String> {
    if value.is_empty()
        || value.len() > 255
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(value)
}

fn vault_configuration_error(error: VaultClientError) -> String {
    match error {
        VaultClientError::Configuration(message)
        | VaultClientError::Rejected(message)
        | VaultClientError::Unavailable(message) => message,
    }
}

fn map_vault_error(error: VaultClientError) -> RecipientContactProofError {
    match error {
        VaultClientError::Unavailable(_) => RecipientContactProofError::Unavailable,
        VaultClientError::Configuration(_) | VaultClientError::Rejected(_) => {
            RecipientContactProofError::Rejected
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::RecipientContactVerification;
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, PrincipalId, RecipientContactId, RecipientContactVerificationId,
        Sha256Digest,
    };
    use chrono::Duration as ChronoDuration;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::sync::Mutex;

    struct FixtureClient {
        secret: [u8; 32],
        calls: Mutex<Vec<(String, String)>>,
        available: bool,
    }

    #[async_trait]
    impl VaultTransitHmacClient for FixtureClient {
        async fn generate(
            &self,
            path: &str,
            request: TransitHmacRequest,
        ) -> Result<TransitHmacResponse, RecipientContactProofError> {
            if !self.available {
                return Err(RecipientContactProofError::Unavailable);
            }
            self.calls
                .lock()
                .expect("calls")
                .push((path.into(), request.input.clone()));
            let input = STANDARD
                .decode(request.input)
                .map_err(|_| RecipientContactProofError::Rejected)?;
            Ok(TransitHmacResponse {
                hmac: vault_hmac(&self.secret, &input),
            })
        }

        async fn verify(
            &self,
            path: &str,
            request: TransitVerifyHmacRequest<'_>,
        ) -> Result<TransitVerifyHmacResponse, RecipientContactProofError> {
            if !self.available {
                return Err(RecipientContactProofError::Unavailable);
            }
            self.calls
                .lock()
                .expect("calls")
                .push((path.into(), request.input.clone()));
            let input = STANDARD
                .decode(request.input)
                .map_err(|_| RecipientContactProofError::Rejected)?;
            Ok(TransitVerifyHmacResponse {
                valid: request.hmac == vault_hmac(&self.secret, &input),
            })
        }
    }

    #[tokio::test]
    async fn vault_provider_binds_exact_paths_input_version_and_verification() {
        let client = Arc::new(FixtureClient {
            secret: [0x5a; 32],
            calls: Mutex::new(Vec::new()),
            available: true,
        });
        let key_id =
            RecipientContactSigningKeyId::parse("recipient-contact-v1").expect("logical key ID");
        let service = VaultRecipientContactProofService::with_client(
            client.clone(),
            "transit",
            "a3s-cloud-recipient-contact-proof",
            key_id.clone(),
        )
        .expect("Vault proof service");
        let now = canonical_timestamp(Utc::now());
        let verification = verification(now, key_id);

        let proof = service.issue(&verification).await.expect("Vault proof");
        assert_eq!(
            service
                .verify(&proof, now + ChronoDuration::minutes(1))
                .await
                .expect("verified Vault proof"),
            verification.claims()
        );
        assert!(!format!("{service:?}").contains("5a5a"));
        let calls = client.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].0,
            "transit/hmac/a3s-cloud-recipient-contact-proof/sha2-256"
        );
        assert_eq!(
            calls[1].0,
            "transit/verify/a3s-cloud-recipient-contact-proof/sha2-256"
        );
        assert_eq!(calls[0].1, calls[1].1);
    }

    #[tokio::test]
    async fn vault_provider_rejects_tampering_and_preserves_unavailable_failures() {
        let key_id =
            RecipientContactSigningKeyId::parse("recipient-contact-v1").expect("logical key ID");
        let now = canonical_timestamp(Utc::now());
        let verification = verification(now, key_id.clone());
        let available = VaultRecipientContactProofService::with_client(
            Arc::new(FixtureClient {
                secret: [0x21; 32],
                calls: Mutex::new(Vec::new()),
                available: true,
            }),
            "transit",
            "recipient-contact-proof",
            key_id.clone(),
        )
        .expect("available service");
        let proof = available.issue(&verification).await.expect("proof");
        let mut tampered = proof.to_string();
        tampered.push('a');
        assert_eq!(
            available
                .verify(&tampered, now + ChronoDuration::seconds(1))
                .await,
            Err(RecipientContactProofError::Rejected)
        );

        let unavailable = VaultRecipientContactProofService::with_client(
            Arc::new(FixtureClient {
                secret: [0x21; 32],
                calls: Mutex::new(Vec::new()),
                available: false,
            }),
            "transit",
            "recipient-contact-proof",
            key_id,
        )
        .expect("unavailable service");
        assert_eq!(
            unavailable.issue(&verification).await,
            Err(RecipientContactProofError::Unavailable)
        );
    }

    #[test]
    fn vault_hmac_parser_requires_versioned_canonical_sha2_256_material() {
        let valid = format!("vault:v3:{}", STANDARD.encode([7_u8; 32]));
        parse_vault_hmac(&valid).expect("valid HMAC");
        for invalid in [
            format!("vault:v0:{}", STANDARD.encode([7_u8; 32])),
            format!("vault:v01:{}", STANDARD.encode([7_u8; 32])),
            format!("vault:v3:{}", STANDARD.encode([7_u8; 31])),
            STANDARD.encode([7_u8; 32]),
        ] {
            assert!(parse_vault_hmac(&invalid).is_err(), "accepted {invalid}");
        }
    }

    fn verification(
        now: DateTime<Utc>,
        key_id: RecipientContactSigningKeyId,
    ) -> RecipientContactVerification {
        RecipientContactVerification::create(
            RecipientContactVerificationId::new(),
            RecipientContactId::new(),
            PrincipalId::new(),
            Sha256Digest::from_bytes(b"mailbox"),
            4,
            key_id,
            now,
            now + ChronoDuration::minutes(10),
        )
        .expect("verification")
    }

    fn vault_hmac(secret: &[u8], input: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC secret");
        mac.update(input);
        format!("vault:v7:{}", STANDARD.encode(mac.finalize().into_bytes()))
    }
}
