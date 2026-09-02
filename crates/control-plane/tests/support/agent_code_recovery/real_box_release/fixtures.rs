use super::*;
use std::collections::BTreeMap;
use std::sync::Mutex;

pub(super) struct FixtureSecretEncryption;

#[async_trait]
impl ISecretEncryptionService for FixtureSecretEncryption {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        context: &[u8],
    ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
        let mut bound = Vec::with_capacity(context.len() + plaintext.len());
        bound.extend_from_slice(context);
        bound.extend_from_slice(plaintext);
        EncryptedSecretValue::new("test:a0-4", Sha256Digest::from_bytes(&bound).to_string())
            .map_err(SecretEncryptionError::Rejected)
    }

    async fn decrypt(
        &self,
        _value: &EncryptedSecretValue,
        _context: &[u8],
    ) -> Result<Vec<u8>, SecretEncryptionError> {
        Err(SecretEncryptionError::Rejected(
            "A0.4 fixture ciphertext is admission-only".into(),
        ))
    }

    async fn health(&self) -> Result<bool, SecretEncryptionError> {
        Ok(true)
    }
}

pub(super) struct PublishedAgentSecretTransport {
    expected_revision: Mutex<Option<Uuid>>,
    materials: BTreeMap<(Uuid, u64), Vec<u8>>,
    calls: Mutex<BTreeMap<Uuid, usize>>,
}

impl PublishedAgentSecretTransport {
    pub(super) fn new(materials: impl IntoIterator<Item = (SecretId, Vec<u8>)>) -> Self {
        let materials = materials
            .into_iter()
            .map(|(secret_id, material)| ((secret_id.as_uuid(), 1), material))
            .collect::<BTreeMap<_, _>>();
        let calls = materials
            .keys()
            .map(|(secret_id, _)| (*secret_id, 0))
            .collect();
        Self {
            expected_revision: Mutex::new(None),
            materials,
            calls: Mutex::new(calls),
        }
    }

    pub(super) fn bind_revision(&self, revision_id: Uuid) -> Result<(), String> {
        if revision_id.is_nil() {
            return Err("published Agent Secret transport received a nil revision".into());
        }
        let mut expected = self
            .expected_revision
            .lock()
            .map_err(|_| "published Agent Secret revision lock was poisoned")?;
        match *expected {
            Some(current) if current != revision_id => {
                Err("published Agent Secret transport changed its revision".into())
            }
            _ => {
                *expected = Some(revision_id);
                Ok(())
            }
        }
    }

    pub(super) fn calls(&self, secret_id: SecretId) -> Result<usize, String> {
        self.calls
            .lock()
            .map_err(|_| "published Agent Secret call lock was poisoned".to_owned())?
            .get(&secret_id.as_uuid())
            .copied()
            .ok_or_else(|| "published Agent Secret call counter is missing".into())
    }
}

#[async_trait]
impl NodeSecretTransport for PublishedAgentSecretTransport {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        reference
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let expected_revision = self
            .expected_revision
            .lock()
            .map_err(|_| {
                NodeControlClientError::Transport(
                    "published Agent Secret revision lock was poisoned".into(),
                )
            })?
            .ok_or_else(|| {
                NodeControlClientError::Invalid(
                    "published Agent Secret transport is not revision-bound".into(),
                )
            })?;
        if reference.workload_revision_id != expected_revision {
            return Err(NodeControlClientError::Invalid(
                "published Agent requested a Secret for another revision".into(),
            ));
        }
        let material = self
            .materials
            .get(&(reference.secret_id, reference.version))
            .cloned()
            .ok_or_else(|| {
                NodeControlClientError::Invalid(
                    "published Agent requested an unknown Secret version".into(),
                )
            })?;
        let mut calls = self.calls.lock().map_err(|_| {
            NodeControlClientError::Transport(
                "published Agent Secret call lock was poisoned".into(),
            )
        })?;
        let count = calls.get_mut(&reference.secret_id).ok_or_else(|| {
            NodeControlClientError::Invalid("published Agent Secret call counter is missing".into())
        })?;
        *count = count.checked_add(1).ok_or_else(|| {
            NodeControlClientError::Invalid("published Agent Secret call counter overflowed".into())
        })?;
        SecretMaterial::new(material).map_err(NodeControlClientError::Invalid)
    }
}

pub(super) struct PublishedRuntimeImage {
    pub(super) artifact: ArtifactRef,
    pub(super) size_bytes: u64,
}

impl PublishedRuntimeImage {
    pub(super) fn from_environment() -> TestResult<Self> {
        let image = std::env::var(AGENT_RUNTIME_IMAGE_ENV)?;
        let (repository, digest) = image
            .rsplit_once('@')
            .filter(|(repository, digest)| {
                !repository.is_empty()
                    && digest.strip_prefix("sha256:").is_some_and(|hex| {
                        hex.len() == 64
                            && hex
                                .bytes()
                                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    })
            })
            .ok_or_else(|| invalid("A0.4 Agent Runtime image is not digest-pinned"))?;
        let media_type = std::env::var(AGENT_RUNTIME_MEDIA_TYPE_ENV)?;
        if media_type != OCI_IMAGE_MANIFEST_MEDIA_TYPE {
            return Err(invalid("A0.4 Agent Runtime image must pin one platform manifest").into());
        }
        let size_bytes = std::env::var(AGENT_RUNTIME_SIZE_ENV)?.parse::<u64>()?;
        if size_bytes == 0 {
            return Err(invalid("A0.4 Agent Runtime descriptor size must be positive").into());
        }
        let artifact = ArtifactRef {
            uri: format!("oci://{repository}@{digest}"),
            digest: digest.into(),
            media_type,
        };
        artifact.validate().map_err(invalid)?;
        Ok(Self {
            artifact,
            size_bytes,
        })
    }
}

pub(super) struct PublishedManifestTransport {
    pub(super) artifact: ArtifactRef,
    pub(super) archive: Vec<u8>,
    pub(super) downloads: AtomicUsize,
}

#[async_trait]
impl NodeArtifactTransport for PublishedManifestTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request
            .artifact()
            .map_err(NodeControlClientError::Invalid)?
            != self.artifact
            || self.archive.len() as u64 > maximum_bytes
        {
            return Err(NodeControlClientError::Invalid(
                "real A0.4 gate requested an unexpected manifest Artifact".into(),
            ));
        }
        tokio::fs::write(destination, &self.archive)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(DownloadedNodeArtifact {
            size_bytes: self.archive.len() as u64,
        })
    }

    async fn upload(
        &self,
        _request: &NodeArtifactUploadRequest,
        _source: &Path,
    ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "real A0.4 Agent Service has no output Artifact".into(),
        ))
    }
}

pub(super) struct FixedInventory(pub(super) NodeResourceInventory);

#[async_trait]
impl NodeResourceInventoryAuthority for FixedInventory {
    async fn current_resource_inventory(
        &self,
    ) -> Result<NodeResourceInventory, ResourceInventoryError> {
        Ok(self.0.clone())
    }
}

pub(super) struct PinnedArtifactResolver(pub(super) ArtifactRef);

#[async_trait]
impl IOciArtifactResolver for PinnedArtifactResolver {
    async fn resolve(
        &self,
        reference: &OciArtifactReference,
        _registry_credential: Option<&OciRegistryCredentialReference>,
    ) -> Result<OciArtifact, OciArtifactResolutionError> {
        let digest = reference
            .expected_digest
            .as_deref()
            .or_else(|| reference.bound_digest().ok().flatten())
            .ok_or_else(|| {
                OciArtifactResolutionError::InvalidReference(
                    "published Agent Runtime reference lost its digest".into(),
                )
            })?;
        if digest != self.0.digest.as_str() {
            return Err(OciArtifactResolutionError::InvalidReference(
                "published Agent Runtime reference changed its digest".into(),
            ));
        }
        Ok(OciArtifact {
            uri: self.0.uri.clone(),
            digest: self.0.digest.clone(),
            media_type: self.0.media_type.clone(),
        })
    }
}
