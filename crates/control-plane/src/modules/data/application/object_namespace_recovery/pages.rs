mod delete;
mod restore;
mod seal;
mod support;

use super::*;
use crate::modules::data::domain::ObjectNamespaceEntry;
use support::{entries_after, require_entry_body, require_metadata_complete};

const CHECKPOINT_PAGE_OBJECTS: usize = 32;
const CHECKPOINT_PAGE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const OBJECT_NAMESPACE_MAXIMUM_CHECKPOINT_PAGES: u32 = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectNamespaceSealPageCheckpoint {
    page_index: u32,
    start_after: Option<ObjectNamespaceKey>,
    entries: Vec<RecoveryManifestEntry>,
    complete: bool,
    checkpoint_digest: Sha256Digest,
}

impl ObjectNamespaceSealPageCheckpoint {
    pub(crate) const fn page_index(&self) -> u32 {
        self.page_index
    }

    pub(crate) const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectNamespaceObservationPageCheckpoint {
    phase: String,
    binding_digest: Sha256Digest,
    page_index: u32,
    start_after: Option<ObjectNamespaceKey>,
    last_key: Option<ObjectNamespaceKey>,
    processed_objects: u32,
    processed_bytes: u64,
    complete: bool,
    checkpoint_digest: Sha256Digest,
}

impl ObjectNamespaceObservationPageCheckpoint {
    pub(crate) const fn page_index(&self) -> u32 {
        self.page_index
    }

    pub(crate) const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectNamespaceManifestPageCheckpoint {
    phase: String,
    binding_digest: Sha256Digest,
    manifest_digest: Sha256Digest,
    page_index: u32,
    start_index: u32,
    next_index: u32,
    processed_bytes: u64,
    complete: bool,
    checkpoint_digest: Sha256Digest,
}

impl ObjectNamespaceManifestPageCheckpoint {
    pub(crate) const fn page_index(&self) -> u32 {
        self.page_index
    }

    pub(crate) const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectNamespaceCleanupPageCheckpoint {
    binding_digest: Sha256Digest,
    page_index: u32,
    entries: Vec<ObjectNamespaceCleanupEntry>,
    complete: bool,
    checkpoint_digest: Sha256Digest,
}

impl ObjectNamespaceCleanupPageCheckpoint {
    pub(crate) const fn page_index(&self) -> u32 {
        self.page_index
    }

    pub(crate) const fn is_complete(&self) -> bool {
        self.complete
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObjectNamespaceCleanupEntry {
    key: ObjectNamespaceKey,
    size_bytes: u64,
    digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectNamespaceRecoveryAnchorCheckpoint {
    binding_digest: Sha256Digest,
    manifest_digest: Sha256Digest,
    checkpoint_digest: Sha256Digest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SealCheckpointProjection<'a> {
    page_index: u32,
    start_after: &'a Option<ObjectNamespaceKey>,
    entries: &'a [RecoveryManifestEntry],
    complete: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservationCheckpointProjection<'a> {
    phase: &'a str,
    binding_digest: &'a Sha256Digest,
    page_index: u32,
    start_after: &'a Option<ObjectNamespaceKey>,
    last_key: &'a Option<ObjectNamespaceKey>,
    processed_objects: u32,
    processed_bytes: u64,
    complete: bool,
    previous_checkpoint_digest: Option<&'a Sha256Digest>,
    page_entries: &'a [RecoveryManifestEntry],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestCheckpointProjection<'a> {
    phase: &'a str,
    binding_digest: &'a Sha256Digest,
    manifest_digest: &'a Sha256Digest,
    page_index: u32,
    start_index: u32,
    next_index: u32,
    processed_bytes: u64,
    complete: bool,
    previous_checkpoint_digest: Option<&'a Sha256Digest>,
    page_entries: &'a [RecoveryManifestEntry],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanupCheckpointProjection<'a> {
    binding_digest: &'a Sha256Digest,
    page_index: u32,
    entries: &'a [ObjectNamespaceCleanupEntry],
    complete: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AnchorCheckpointProjection<'a> {
    binding_digest: &'a Sha256Digest,
    manifest_digest: &'a Sha256Digest,
}
