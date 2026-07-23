use crate::infrastructure::{VaultClient, VaultClientError};
use crate::modules::artifacts::domain::{
    sha256_digest, BuildEvidenceSigningError, BuildEvidenceSigningKey, IBuildEvidenceSigner,
    VerifiedBuildEvidenceSignature,
};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ring::signature::{UnparsedPublicKey, ED25519};
use rustls_pemfile::Item;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

const MAX_PAE_BYTES: usize = 64 * 1024 * 1024 + 1024;
const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

pub struct VaultBuildEvidenceSigner {
    client: Arc<dyn VaultTransitSigningClient>,
    mount: String,
    key: String,
}

impl VaultBuildEvidenceSigner {
    pub fn new(
        address: &str,
        token: &str,
        mount: impl Into<String>,
        key: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, BuildEvidenceSigningError> {
        let mount = validate_segment("Vault Transit signing mount", mount.into())?;
        let key = validate_segment("Vault Transit signing key", key.into())?;
        let client = VaultClient::new(address, token, timeout).map_err(map_vault_error)?;
        Ok(Self {
            client: Arc::new(VaultTransitHttpClient { client }),
            mount,
            key,
        })
    }

    #[cfg(test)]
    fn with_client(
        client: Arc<dyn VaultTransitSigningClient>,
        mount: &str,
        key: &str,
    ) -> Result<Self, BuildEvidenceSigningError> {
        Ok(Self {
            client,
            mount: validate_segment("Vault Transit signing mount", mount.into())?,
            key: validate_segment("Vault Transit signing key", key.into())?,
        })
    }
}

#[async_trait]
impl IBuildEvidenceSigner for VaultBuildEvidenceSigner {
    async fn sign(
        &self,
        pae: &[u8],
    ) -> Result<VerifiedBuildEvidenceSignature, BuildEvidenceSigningError> {
        if pae.is_empty() || pae.len() > MAX_PAE_BYTES {
            return Err(BuildEvidenceSigningError::Invalid(
                "DSSE PAE exceeds the Vault signing bound".into(),
            ));
        }
        let response = self
            .client
            .sign(
                &format!("{}/sign/{}", self.mount, self.key),
                TransitSignRequest {
                    input: STANDARD.encode(pae),
                    prehashed: false,
                },
            )
            .await?;
        let (key_version, signature) = parse_vault_signature(&response.signature)?;
        if response
            .key_version
            .is_some_and(|version| version != key_version)
        {
            return Err(BuildEvidenceSigningError::Rejected(
                "Vault signature version disagrees with its response metadata".into(),
            ));
        }
        let key = self
            .client
            .read_key(&format!("{}/keys/{}", self.mount, self.key))
            .await?;
        if key.key_type != "ed25519" {
            return Err(BuildEvidenceSigningError::Rejected(
                "Vault Transit build evidence key must be Ed25519".into(),
            ));
        }
        let version = key.keys.get(&key_version.to_string()).ok_or_else(|| {
            BuildEvidenceSigningError::Rejected(format!(
                "Vault omitted public key version {key_version}"
            ))
        })?;
        let public_key = parse_ed25519_public_key(&version.public_key)?;
        UnparsedPublicKey::new(&ED25519, &public_key)
            .verify(pae, &signature)
            .map_err(|_| {
                BuildEvidenceSigningError::Rejected(format!(
                    "Vault signature from key version {key_version} failed local Ed25519 verification"
                ))
            })?;
        VerifiedBuildEvidenceSignature::new(
            BuildEvidenceSigningKey {
                algorithm: "ed25519".into(),
                key_id: sha256_digest(&public_key),
                public_key: STANDARD.encode(public_key),
                key_version: Some(key_version),
            },
            signature,
        )
    }
}

#[derive(Clone)]
struct VaultTransitHttpClient {
    client: VaultClient,
}

#[async_trait]
impl VaultTransitSigningClient for VaultTransitHttpClient {
    async fn sign(
        &self,
        path: &str,
        request: TransitSignRequest,
    ) -> Result<TransitSignResponse, BuildEvidenceSigningError> {
        self.client
            .post(path, &request)
            .await
            .map_err(map_vault_error)
    }

    async fn read_key(&self, path: &str) -> Result<TransitKeyResponse, BuildEvidenceSigningError> {
        self.client.get(path).await.map_err(map_vault_error)
    }
}

#[async_trait]
trait VaultTransitSigningClient: Send + Sync {
    async fn sign(
        &self,
        path: &str,
        request: TransitSignRequest,
    ) -> Result<TransitSignResponse, BuildEvidenceSigningError>;

    async fn read_key(&self, path: &str) -> Result<TransitKeyResponse, BuildEvidenceSigningError>;
}

#[derive(Debug, Clone, Serialize)]
struct TransitSignRequest {
    input: String,
    prehashed: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct TransitSignResponse {
    signature: String,
    #[serde(default)]
    key_version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct TransitKeyResponse {
    #[serde(rename = "type")]
    key_type: String,
    keys: BTreeMap<String, TransitKeyVersion>,
}

#[derive(Debug, Clone, Deserialize)]
struct TransitKeyVersion {
    public_key: String,
}

fn parse_vault_signature(value: &str) -> Result<(u32, Vec<u8>), BuildEvidenceSigningError> {
    let mut parts = value.split(':');
    if parts.next() != Some("vault") {
        return Err(BuildEvidenceSigningError::Rejected(
            "Vault returned an invalid Transit signature prefix".into(),
        ));
    }
    let version = parts
        .next()
        .and_then(|value| value.strip_prefix('v'))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|version| *version > 0)
        .ok_or_else(|| {
            BuildEvidenceSigningError::Rejected(
                "Vault returned an invalid Transit signature version".into(),
            )
        })?;
    let encoded = parts.next().ok_or_else(|| {
        BuildEvidenceSigningError::Rejected(
            "Vault returned a Transit signature without bytes".into(),
        )
    })?;
    if parts.next().is_some() {
        return Err(BuildEvidenceSigningError::Rejected(
            "Vault returned a malformed Transit signature".into(),
        ));
    }
    let signature = STANDARD.decode(encoded).map_err(|_| {
        BuildEvidenceSigningError::Rejected(
            "Vault returned a Transit signature with invalid base64".into(),
        )
    })?;
    if signature.len() != 64 || STANDARD.encode(&signature) != encoded {
        return Err(BuildEvidenceSigningError::Rejected(
            "Vault returned an invalid Ed25519 signature length or encoding".into(),
        ));
    }
    Ok((version, signature))
}

fn parse_ed25519_public_key(pem: &str) -> Result<[u8; 32], BuildEvidenceSigningError> {
    if pem.len() > 16 * 1024 || pem.contains('\0') {
        return Err(BuildEvidenceSigningError::Rejected(
            "Vault Ed25519 public key exceeds its protocol bound".into(),
        ));
    }
    let mut public_key = None;
    for item in rustls_pemfile::read_all(&mut BufReader::new(pem.as_bytes())) {
        let Item::SubjectPublicKeyInfo(value) = item.map_err(|_| {
            BuildEvidenceSigningError::Rejected(
                "Vault returned an invalid Ed25519 public key PEM".into(),
            )
        })?
        else {
            return Err(BuildEvidenceSigningError::Rejected(
                "Vault returned a non-public-key PEM block".into(),
            ));
        };
        if public_key.is_some() {
            return Err(BuildEvidenceSigningError::Rejected(
                "Vault returned multiple Ed25519 public keys".into(),
            ));
        }
        let der = value.as_ref();
        if der.len() != ED25519_SPKI_PREFIX.len() + 32 || !der.starts_with(&ED25519_SPKI_PREFIX) {
            return Err(BuildEvidenceSigningError::Rejected(
                "Vault returned a public key that is not Ed25519 SPKI".into(),
            ));
        }
        let mut raw = [0_u8; 32];
        raw.copy_from_slice(&der[ED25519_SPKI_PREFIX.len()..]);
        public_key = Some(raw);
    }
    public_key.ok_or_else(|| {
        BuildEvidenceSigningError::Rejected("Vault omitted its Ed25519 public key".into())
    })
}

fn validate_segment(label: &str, value: String) -> Result<String, BuildEvidenceSigningError> {
    if value.is_empty()
        || value.len() > 255
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    {
        return Err(BuildEvidenceSigningError::Invalid(format!(
            "{label} is invalid"
        )));
    }
    Ok(value)
}

fn map_vault_error(error: VaultClientError) -> BuildEvidenceSigningError {
    match error {
        VaultClientError::Configuration(message) => BuildEvidenceSigningError::Invalid(message),
        VaultClientError::Rejected(message) => BuildEvidenceSigningError::Rejected(message),
        VaultClientError::Unavailable(message) => BuildEvidenceSigningError::Unavailable(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::artifacts::domain::dsse_pae;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    struct RotatingVaultClient {
        signing_key: Ed25519KeyPair,
        other_key: Ed25519KeyPair,
        version: u32,
    }

    #[async_trait]
    impl VaultTransitSigningClient for RotatingVaultClient {
        async fn sign(
            &self,
            path: &str,
            request: TransitSignRequest,
        ) -> Result<TransitSignResponse, BuildEvidenceSigningError> {
            assert_eq!(path, "transit/sign/a3s-cloud-build-evidence");
            assert!(!request.prehashed);
            let pae = STANDARD
                .decode(request.input)
                .expect("Vault sign input base64");
            Ok(TransitSignResponse {
                signature: format!(
                    "vault:v{}:{}",
                    self.version,
                    STANDARD.encode(self.signing_key.sign(&pae).as_ref())
                ),
                key_version: Some(self.version),
            })
        }

        async fn read_key(
            &self,
            path: &str,
        ) -> Result<TransitKeyResponse, BuildEvidenceSigningError> {
            assert_eq!(path, "transit/keys/a3s-cloud-build-evidence");
            Ok(TransitKeyResponse {
                key_type: "ed25519".into(),
                keys: BTreeMap::from([
                    (
                        "1".into(),
                        TransitKeyVersion {
                            public_key: public_key_pem(self.other_key.public_key().as_ref()),
                        },
                    ),
                    (
                        self.version.to_string(),
                        TransitKeyVersion {
                            public_key: public_key_pem(self.signing_key.public_key().as_ref()),
                        },
                    ),
                    (
                        (self.version + 1).to_string(),
                        TransitKeyVersion {
                            public_key: public_key_pem(self.other_key.public_key().as_ref()),
                        },
                    ),
                ]),
            })
        }
    }

    #[tokio::test]
    async fn vault_signer_verifies_the_exact_returned_key_version_locally() {
        let signing_key = key_pair();
        let expected_key_id = sha256_digest(signing_key.public_key().as_ref());
        let expected_public_key = STANDARD.encode(signing_key.public_key().as_ref());
        let signer = VaultBuildEvidenceSigner::with_client(
            Arc::new(RotatingVaultClient {
                signing_key,
                other_key: key_pair(),
                version: 2,
            }),
            "transit",
            "a3s-cloud-build-evidence",
        )
        .expect("Vault signer");
        let pae = dsse_pae("application/vnd.in-toto+json", b"{}").expect("DSSE PAE");

        let signature = signer.sign(&pae).await.expect("verified signature");

        assert_eq!(signature.key.algorithm, "ed25519");
        assert_eq!(signature.key.key_id, expected_key_id);
        assert_eq!(signature.key.public_key, expected_public_key);
        assert_eq!(signature.key.key_version, Some(2));
        assert_eq!(signature.signature.len(), 64);
    }

    #[tokio::test]
    async fn vault_signer_rejects_a_signature_that_does_not_match_its_versioned_key() {
        struct MismatchedVaultClient {
            signer: Ed25519KeyPair,
            advertised: Ed25519KeyPair,
        }

        #[async_trait]
        impl VaultTransitSigningClient for MismatchedVaultClient {
            async fn sign(
                &self,
                _path: &str,
                request: TransitSignRequest,
            ) -> Result<TransitSignResponse, BuildEvidenceSigningError> {
                let pae = STANDARD.decode(request.input).expect("Vault input");
                Ok(TransitSignResponse {
                    signature: format!(
                        "vault:v7:{}",
                        STANDARD.encode(self.signer.sign(&pae).as_ref())
                    ),
                    key_version: Some(7),
                })
            }

            async fn read_key(
                &self,
                _path: &str,
            ) -> Result<TransitKeyResponse, BuildEvidenceSigningError> {
                Ok(TransitKeyResponse {
                    key_type: "ed25519".into(),
                    keys: BTreeMap::from([(
                        "7".into(),
                        TransitKeyVersion {
                            public_key: public_key_pem(self.advertised.public_key().as_ref()),
                        },
                    )]),
                })
            }
        }

        let signer = VaultBuildEvidenceSigner::with_client(
            Arc::new(MismatchedVaultClient {
                signer: key_pair(),
                advertised: key_pair(),
            }),
            "transit",
            "a3s-cloud-build-evidence",
        )
        .expect("Vault signer");

        assert!(matches!(
            signer.sign(b"DSSEv1 1 x 1 y").await,
            Err(BuildEvidenceSigningError::Rejected(_))
        ));
    }

    fn key_pair() -> Ed25519KeyPair {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("Ed25519 PKCS#8");
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("Ed25519 key")
    }

    fn public_key_pem(raw: &[u8]) -> String {
        let mut der = ED25519_SPKI_PREFIX.to_vec();
        der.extend_from_slice(raw);
        let encoded = STANDARD.encode(der);
        format!("-----BEGIN PUBLIC KEY-----\n{encoded}\n-----END PUBLIC KEY-----\n")
    }
}
