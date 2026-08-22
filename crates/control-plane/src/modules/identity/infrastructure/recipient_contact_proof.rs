use crate::modules::identity::domain::entities::{
    RecipientContactVerification, RecipientContactVerificationClaims,
    RecipientContactVerificationStatus,
};
use crate::modules::identity::domain::services::{
    IRecipientContactProofService, RecipientContactProofError,
};
use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
use crate::modules::shared_kernel::domain::canonical_timestamp;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fmt;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

const PROOF_PREFIX: &str = "a3srcv1";
const MINIMUM_SECRET_BYTES: usize = 32;
const MAXIMUM_SECRET_BYTES: usize = 512;
const MAXIMUM_PROOF_BYTES: usize = 4096;

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

impl IRecipientContactProofService for HmacRecipientContactProofService {
    fn current_key_id(&self) -> &RecipientContactSigningKeyId {
        &self.key_id
    }

    fn issue(
        &self,
        verification: &RecipientContactVerification,
    ) -> Result<Zeroizing<String>, RecipientContactProofError> {
        verification
            .validate()
            .map_err(|_| RecipientContactProofError::Rejected)?;
        if verification.signing_key_id != self.key_id
            || verification.status_at(verification.issued_at)
                != RecipientContactVerificationStatus::Pending
        {
            return Err(RecipientContactProofError::Rejected);
        }
        let payload = serde_json::to_vec(&verification.claims())
            .map_err(|_| RecipientContactProofError::Unavailable)?;
        let payload = URL_SAFE_NO_PAD.encode(payload);
        let signing_input = format!("{PROOF_PREFIX}.{payload}");
        let mut mac = self.mac()?;
        mac.update(signing_input.as_bytes());
        let signature_bytes = Zeroizing::new(mac.finalize().into_bytes().to_vec());
        let signature = Zeroizing::new(URL_SAFE_NO_PAD.encode(signature_bytes.as_slice()));
        Ok(Zeroizing::new(format!(
            "{signing_input}.{}",
            signature.as_str()
        )))
    }

    fn verify(
        &self,
        proof: &str,
        now: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationClaims, RecipientContactProofError> {
        if proof.is_empty()
            || proof.len() > MAXIMUM_PROOF_BYTES
            || proof.contains(['\0', '\r', '\n'])
        {
            return Err(RecipientContactProofError::Rejected);
        }
        let mut parts = proof.split('.');
        let prefix = parts.next();
        let payload = parts.next();
        let signature = parts.next();
        if prefix != Some(PROOF_PREFIX) || parts.next().is_some() {
            return Err(RecipientContactProofError::Rejected);
        }
        let payload = payload.ok_or(RecipientContactProofError::Rejected)?;
        let signature = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(signature.ok_or(RecipientContactProofError::Rejected)?)
                .map_err(|_| RecipientContactProofError::Rejected)?,
        );
        let signing_input = format!("{PROOF_PREFIX}.{payload}");
        let mut mac = self.mac()?;
        mac.update(signing_input.as_bytes());
        mac.verify_slice(signature.as_slice())
            .map_err(|_| RecipientContactProofError::Rejected)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| RecipientContactProofError::Rejected)?;
        let claims = serde_json::from_slice::<RecipientContactVerificationClaims>(&payload)
            .map_err(|_| RecipientContactProofError::Rejected)?;
        claims
            .validate()
            .map_err(|_| RecipientContactProofError::Rejected)?;
        let now = canonical_timestamp(now);
        if claims.signing_key_id != self.key_id
            || now < claims.issued_at
            || now >= claims.expires_at
        {
            return Err(RecipientContactProofError::Rejected);
        }
        Ok(claims)
    }
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

    #[test]
    fn proof_binds_every_claim_and_rejects_tampering_expiry_and_stale_keys() {
        let now = canonical_timestamp(Utc::now());
        let current = service("contact-v1", 7);
        let verification = verification(now);
        let proof = current.issue(&verification).expect("proof");
        assert_eq!(
            current
                .verify(&proof, now + Duration::minutes(1))
                .expect("verified proof"),
            verification.claims()
        );

        let mut tampered = proof.to_string();
        tampered.push('x');
        assert_eq!(
            current.verify(&tampered, now + Duration::minutes(1)),
            Err(RecipientContactProofError::Rejected)
        );
        assert_eq!(
            current.verify(&proof, verification.expires_at),
            Err(RecipientContactProofError::Rejected)
        );
        assert_eq!(
            service("contact-v2", 9).verify(&proof, now + Duration::minutes(1)),
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
}
