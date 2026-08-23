use crate::modules::shared_kernel::domain::{GitCommitSha, Sha256Digest};
use serde::{Deserialize, Serialize};

pub const MAX_SOURCE_LAYOUT_ENTRIES: usize = 16_384;
pub const MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES: usize = 64 * 1024;
pub const MAX_SOURCE_LAYOUT_CONTENT_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLayoutIdentity {
    pub source_identity_digest: Sha256Digest,
    pub commit_sha: GitCommitSha,
    pub content_digest: Sha256Digest,
}

impl SourceLayoutIdentity {
    pub fn new(
        source_identity_digest: Sha256Digest,
        commit_sha: GitCommitSha,
        content_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            source_identity_digest,
            commit_sha,
            content_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Sha256Digest::parse(self.source_identity_digest.as_str())? != self.source_identity_digest
            || GitCommitSha::parse(self.commit_sha.as_str())? != self.commit_sha
            || Sha256Digest::parse(self.content_digest.as_str())? != self.content_digest
        {
            return Err("source layout identity is not canonical".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLayoutEntryKind {
    Regular,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLayoutEntry {
    path: String,
    kind: SourceLayoutEntryKind,
    size_bytes: u64,
    content_digest: Sha256Digest,
    inspected_content: Option<Vec<u8>>,
}

impl SourceLayoutEntry {
    pub fn metadata(
        path: impl Into<String>,
        kind: SourceLayoutEntryKind,
        size_bytes: u64,
        content_digest: Sha256Digest,
    ) -> Result<Self, String> {
        Self::new(path.into(), kind, size_bytes, content_digest, None)
    }

    pub fn inspected_regular(
        path: impl Into<String>,
        content: impl Into<Vec<u8>>,
    ) -> Result<Self, String> {
        let content = content.into();
        let size_bytes = content.len() as u64;
        let content_digest = Sha256Digest::from_bytes(&content);
        Self::new(
            path.into(),
            SourceLayoutEntryKind::Regular,
            size_bytes,
            content_digest,
            Some(content),
        )
    }

    fn new(
        path: String,
        kind: SourceLayoutEntryKind,
        size_bytes: u64,
        content_digest: Sha256Digest,
        inspected_content: Option<Vec<u8>>,
    ) -> Result<Self, String> {
        let value = Self {
            path,
            kind,
            size_bytes,
            content_digest,
            inspected_content,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_repository_file_path(&self.path)?;
        if self.size_bytes > MAX_SOURCE_LAYOUT_CONTENT_BYTES
            || Sha256Digest::parse(self.content_digest.as_str())? != self.content_digest
        {
            return Err("source layout entry size or digest is invalid".into());
        }
        match (&self.kind, &self.inspected_content) {
            (SourceLayoutEntryKind::Symlink, Some(_)) => {
                return Err("source layout cannot inspect symlink content".into())
            }
            (SourceLayoutEntryKind::Regular, Some(content)) => {
                if content.len() > MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES
                    || content.len() as u64 != self.size_bytes
                    || Sha256Digest::from_bytes(content) != self.content_digest
                {
                    return Err("source layout inspected content is invalid".into());
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub const fn kind(&self) -> SourceLayoutEntryKind {
        self.kind
    }

    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub const fn content_digest(&self) -> &Sha256Digest {
        &self.content_digest
    }

    pub fn inspected_content(&self) -> Option<&[u8]> {
        self.inspected_content.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLayoutSnapshot {
    identity: SourceLayoutIdentity,
    entries: Vec<SourceLayoutEntry>,
}

impl SourceLayoutSnapshot {
    pub fn new(
        identity: SourceLayoutIdentity,
        mut entries: Vec<SourceLayoutEntry>,
    ) -> Result<Self, String> {
        identity.validate()?;
        if entries.len() > MAX_SOURCE_LAYOUT_ENTRIES {
            return Err("source layout contains too many entries".into());
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let mut total_bytes = 0_u64;
        let mut previous: Option<&str> = None;
        for entry in &entries {
            entry.validate()?;
            if previous == Some(entry.path()) {
                return Err("source layout contains a duplicate path".into());
            }
            previous = Some(entry.path());
            total_bytes = total_bytes
                .checked_add(entry.size_bytes)
                .ok_or_else(|| "source layout content size overflowed".to_owned())?;
            if total_bytes > MAX_SOURCE_LAYOUT_CONTENT_BYTES {
                return Err("source layout exceeds the detection content bound".into());
            }
        }
        Ok(Self { identity, entries })
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::new(self.identity.clone(), self.entries.clone())? != *self {
            return Err("source layout snapshot is not canonical".into());
        }
        Ok(())
    }

    pub const fn identity(&self) -> &SourceLayoutIdentity {
        &self.identity
    }

    pub fn entries(&self) -> &[SourceLayoutEntry] {
        &self.entries
    }

    pub fn entry(&self, path: &str) -> Option<&SourceLayoutEntry> {
        self.entries
            .binary_search_by(|entry| entry.path().cmp(path))
            .ok()
            .map(|index| &self.entries[index])
    }
}

pub(crate) fn validate_repository_file_path(value: &str) -> Result<(), String> {
    validate_repository_path(value, false)
}

pub(crate) fn validate_repository_root(value: &str) -> Result<(), String> {
    validate_repository_path(value, true)
}

fn validate_repository_path(value: &str, allow_root: bool) -> Result<(), String> {
    if value == "." {
        return allow_root
            .then_some(())
            .ok_or_else(|| "repository file path cannot be the source root".into());
    }
    if value.is_empty()
        || value.len() > 4096
        || value.starts_with('/')
        || value.starts_with("./")
        || value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
    {
        return Err("repository path must be a canonical bounded relative POSIX path".into());
    }
    for segment in value.split('/') {
        if segment.is_empty()
            || matches!(segment, "." | "..")
            || segment.eq_ignore_ascii_case(".git")
        {
            return Err("repository path contains an unsafe segment".into());
        }
    }
    Ok(())
}

pub(crate) fn parent_root(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_owned())
        .unwrap_or_else(|| ".".to_owned())
}

pub(crate) fn path_is_within_root(path: &str, root: &str) -> bool {
    root == "."
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
