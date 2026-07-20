use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Component, Path};
use tokio::io::AsyncReadExt;

use crate::modules::sources::domain::SourceCheckoutError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManifestKind {
    Regular { executable: bool },
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ManifestEntry {
    kind: ManifestKind,
    size: u64,
}

pub(super) struct GitTreeManifest {
    entries: BTreeMap<String, ManifestEntry>,
    content_bytes: u64,
}

pub(super) struct WorktreeDigest {
    pub(super) digest: String,
    pub(super) file_count: usize,
    pub(super) content_bytes: u64,
}

impl GitTreeManifest {
    pub(super) fn parse(
        encoded: &[u8],
        max_files: usize,
        max_bytes: u64,
    ) -> Result<Self, SourceCheckoutError> {
        let mut entries = BTreeMap::new();
        let mut content_bytes = 0_u64;
        for record in encoded
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
        {
            let tab = record
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| integrity("Git tree contains a malformed entry"))?;
            let header = std::str::from_utf8(&record[..tab])
                .map_err(|_| integrity("Git tree metadata is not UTF-8"))?;
            let path = std::str::from_utf8(&record[tab + 1..])
                .map_err(|_| integrity("Git tree path is not UTF-8"))?;
            validate_repository_path(path)?;
            let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
            if fields.len() != 4 {
                return Err(integrity("Git tree contains a malformed entry"));
            }
            let kind = match (fields[0], fields[1]) {
                ("100644", "blob") => ManifestKind::Regular { executable: false },
                ("100755", "blob") => ManifestKind::Regular { executable: true },
                ("120000", "blob") => ManifestKind::Symlink,
                ("160000", "commit") => {
                    return Err(integrity(
                        "Git submodules are not supported by the secure checkout boundary",
                    ))
                }
                _ => return Err(integrity("Git tree contains an unsupported entry kind")),
            };
            if !valid_object_id(fields[2]) {
                return Err(integrity("Git tree contains an invalid object ID"));
            }
            let size = fields[3]
                .parse::<u64>()
                .map_err(|_| integrity("Git tree contains an invalid object size"))?;
            content_bytes = content_bytes
                .checked_add(size)
                .ok_or_else(|| integrity("Git tree content size overflowed"))?;
            if content_bytes > max_bytes {
                return Err(integrity("Git tree exceeds the checkout byte limit"));
            }
            if entries
                .insert(path.to_owned(), ManifestEntry { kind, size })
                .is_some()
            {
                return Err(integrity("Git tree contains a duplicate path"));
            }
            if entries.len() > max_files {
                return Err(integrity("Git tree exceeds the checkout file limit"));
            }
        }
        Ok(Self {
            entries,
            content_bytes,
        })
    }

    pub(super) fn validate_worktree(
        &self,
        scanned: &ScannedWorktree,
    ) -> Result<(), SourceCheckoutError> {
        if scanned.files.len() != self.entries.len() || scanned.content_bytes != self.content_bytes
        {
            return Err(integrity(
                "checked-out files do not match the accepted Git tree",
            ));
        }
        for (path, expected) in &self.entries {
            let actual = scanned
                .files
                .get(path)
                .ok_or_else(|| integrity("checked-out files do not match the accepted Git tree"))?;
            if actual != expected {
                return Err(integrity(
                    "checked-out files do not match the accepted Git tree",
                ));
            }
        }
        Ok(())
    }
}

pub(super) async fn digest_worktree(
    root: &Path,
    max_files: usize,
    max_bytes: u64,
) -> Result<(WorktreeDigest, ScannedWorktree), SourceCheckoutError> {
    let mut queue = VecDeque::from([(root.to_path_buf(), String::new())]);
    let mut entries = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut directory_count = 0_usize;
    let mut content_bytes = 0_u64;
    while let Some((directory, relative_parent)) = queue.pop_front() {
        let mut reader = tokio::fs::read_dir(&directory)
            .await
            .map_err(|_| storage("could not inspect checked-out source"))?;
        while let Some(entry) = reader
            .next_entry()
            .await
            .map_err(|_| storage("could not inspect checked-out source"))?
        {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| integrity("checked-out source path is not UTF-8"))?;
            validate_path_segment(&name)?;
            let relative = if relative_parent.is_empty() {
                name
            } else {
                format!("{relative_parent}/{name}")
            };
            if relative.len() > 4096 {
                return Err(integrity("checked-out source path is too long"));
            }
            let metadata = tokio::fs::symlink_metadata(entry.path())
                .await
                .map_err(|_| storage("could not inspect checked-out source"))?;
            let file_type = metadata.file_type();
            if file_type.is_dir() {
                directory_count = directory_count
                    .checked_add(1)
                    .ok_or_else(|| integrity("checked-out directory count overflowed"))?;
                if directory_count > max_files.saturating_mul(4) {
                    return Err(integrity("checked-out source has too many directories"));
                }
                entries.insert(relative.clone(), ScannedEntry::Directory);
                queue.push_back((entry.path(), relative));
                continue;
            }
            let scanned = if file_type.is_file() {
                let size = metadata.len();
                let executable = executable(&metadata);
                ScannedEntry::Regular { executable, size }
            } else if file_type.is_symlink() {
                let target = tokio::fs::read_link(entry.path())
                    .await
                    .map_err(|_| storage("could not inspect checked-out symlink"))?;
                let target = target
                    .to_str()
                    .ok_or_else(|| integrity("checked-out symlink target is not UTF-8"))?;
                validate_symlink_target(&relative, target)?;
                ScannedEntry::Symlink {
                    target: target.to_owned(),
                }
            } else {
                return Err(integrity(
                    "checked-out source contains an unsupported filesystem entry",
                ));
            };
            let manifest = scanned.manifest_entry()?;
            content_bytes = content_bytes
                .checked_add(manifest.size)
                .ok_or_else(|| integrity("checked-out content size overflowed"))?;
            if content_bytes > max_bytes {
                return Err(integrity(
                    "checked-out source exceeds the checkout byte limit",
                ));
            }
            files.insert(relative.clone(), manifest);
            entries.insert(relative, scanned);
            if files.len() > max_files {
                return Err(integrity(
                    "checked-out source exceeds the checkout file limit",
                ));
            }
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(b"a3s.cloud.source-tree.v1\0");
    for (relative, entry) in &entries {
        hash_field(&mut hasher, relative.as_bytes());
        match entry {
            ScannedEntry::Directory => hasher.update(b"d"),
            ScannedEntry::Regular { executable, size } => {
                hasher.update(if *executable { b"x" } else { b"f" });
                hasher.update(size.to_be_bytes());
                let mut file = tokio::fs::File::open(root.join(relative))
                    .await
                    .map_err(|_| storage("could not read checked-out source file"))?;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    let read = file
                        .read(&mut buffer)
                        .await
                        .map_err(|_| storage("could not read checked-out source file"))?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..read]);
                }
            }
            ScannedEntry::Symlink { target } => {
                hasher.update(b"l");
                hash_field(&mut hasher, target.as_bytes());
            }
        }
    }
    Ok((
        WorktreeDigest {
            digest: format!("sha256:{:x}", hasher.finalize()),
            file_count: files.len(),
            content_bytes,
        },
        ScannedWorktree {
            files,
            content_bytes,
        },
    ))
}

pub(super) fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(super) struct ScannedWorktree {
    files: BTreeMap<String, ManifestEntry>,
    content_bytes: u64,
}

#[derive(Debug)]
enum ScannedEntry {
    Directory,
    Regular { executable: bool, size: u64 },
    Symlink { target: String },
}

impl ScannedEntry {
    fn manifest_entry(&self) -> Result<ManifestEntry, SourceCheckoutError> {
        match self {
            Self::Regular { executable, size } => Ok(ManifestEntry {
                kind: ManifestKind::Regular {
                    executable: *executable,
                },
                size: *size,
            }),
            Self::Symlink { target } => Ok(ManifestEntry {
                kind: ManifestKind::Symlink,
                size: target.len() as u64,
            }),
            Self::Directory => Err(integrity(
                "checked-out directory cannot be a Git file entry",
            )),
        }
    }
}

fn validate_repository_path(value: &str) -> Result<(), SourceCheckoutError> {
    if value.is_empty() || value.len() > 4096 || value.starts_with('/') || value.contains('\\') {
        return Err(integrity("Git tree contains an unsafe path"));
    }
    for segment in value.split('/') {
        validate_path_segment(segment)?;
    }
    Ok(())
}

fn validate_path_segment(value: &str) -> Result<(), SourceCheckoutError> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.eq_ignore_ascii_case(".git")
        || value.chars().any(char::is_control)
        || value.contains(['/', '\\'])
    {
        return Err(integrity("checked-out source contains an unsafe path"));
    }
    Ok(())
}

fn validate_symlink_target(path: &str, target: &str) -> Result<(), SourceCheckoutError> {
    if target.is_empty()
        || target.len() > 4096
        || target.contains('\\')
        || target.chars().any(char::is_control)
        || Path::new(target).is_absolute()
    {
        return Err(integrity("checked-out source contains an unsafe symlink"));
    }
    let parent = Path::new(path).parent().unwrap_or_else(|| Path::new(""));
    let mut resolved = parent
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for component in Path::new(target).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| integrity("checked-out symlink target is not UTF-8"))?;
                validate_path_segment(value)?;
                resolved.push(value.into());
            }
            Component::ParentDir => {
                if resolved.pop().is_none() {
                    return Err(integrity(
                        "checked-out symlink target escapes the source root",
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(integrity("checked-out source contains an unsafe symlink"))
            }
        }
    }
    Ok(())
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

#[cfg(unix)]
fn executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn integrity(message: impl Into<String>) -> SourceCheckoutError {
    SourceCheckoutError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> SourceCheckoutError {
    SourceCheckoutError::Storage(message.into())
}
