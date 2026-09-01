use super::*;

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
