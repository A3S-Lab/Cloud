use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ArchiveLimits {
    pub(super) max_entries: usize,
    pub(super) max_file_bytes: u64,
    pub(super) max_expanded_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ArchiveSummary {
    pub(super) entries: usize,
    pub(super) expanded_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlannedKind {
    Directory,
    File {
        executable: bool,
        size: u64,
        digest: [u8; 32],
    },
    Symlink {
        target: PathBuf,
    },
    Hardlink {
        target: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedEntry {
    path: PathBuf,
    kind: PlannedKind,
}

pub(super) fn extract_directory_archive(
    archive_path: &Path,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<ArchiveSummary, String> {
    validate_options(archive_path, destination, limits)?;
    let (plans, summary) = plan_archive(archive_path, limits)?;
    validate_plan(&plans)?;
    std::fs::create_dir(destination)
        .map_err(|error| format!("could not create artifact extraction root: {error}"))?;
    if let Err(error) = extract_plans(archive_path, destination, &plans) {
        let _ = std::fs::remove_dir_all(destination);
        return Err(error);
    }
    Ok(summary)
}

fn validate_options(
    archive_path: &Path,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<(), String> {
    if !archive_path.is_file()
        || destination.exists()
        || destination.as_os_str().is_empty()
        || limits.max_entries == 0
        || limits.max_file_bytes == 0
        || limits.max_expanded_bytes == 0
        || limits.max_file_bytes > limits.max_expanded_bytes
    {
        return Err("artifact archive extraction options are invalid".into());
    }
    Ok(())
}

fn plan_archive(
    archive_path: &Path,
    limits: ArchiveLimits,
) -> Result<(Vec<PlannedEntry>, ArchiveSummary), String> {
    let file = File::open(archive_path)
        .map_err(|error| format!("could not open artifact archive: {error}"))?;
    let mut archive = tar::Archive::new(file);
    let entries = archive
        .entries()
        .map_err(|error| format!("could not read artifact archive: {error}"))?;
    let mut plans = Vec::new();
    let mut expanded_bytes = 0_u64;
    let mut has_explicit_root = false;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| format!("artifact archive entry is invalid: {error}"))?;
        let raw_path = entry
            .path()
            .map_err(|error| format!("artifact archive path is invalid: {error}"))?;
        let entry_type = entry.header().entry_type();
        if is_explicit_archive_root(&raw_path) {
            if has_explicit_root || !plans.is_empty() || !entry_type.is_dir() || entry.size() != 0 {
                return Err("artifact archive has an invalid explicit root entry".into());
            }
            has_explicit_root = true;
            continue;
        }
        if plans.len() >= limits.max_entries {
            return Err("artifact archive exceeds the entry limit".into());
        }
        let path = normalized_archive_path(&raw_path)?;
        let kind = if entry_type.is_dir() {
            if entry.size() != 0 {
                return Err("artifact archive directory has a nonzero size".into());
            }
            PlannedKind::Directory
        } else if entry_type.is_file() {
            let size = entry.size();
            if size > limits.max_file_bytes {
                return Err("artifact archive file exceeds the single-file limit".into());
            }
            expanded_bytes = expanded_bytes
                .checked_add(size)
                .ok_or_else(|| "artifact archive expanded size overflowed".to_owned())?;
            if expanded_bytes > limits.max_expanded_bytes {
                return Err("artifact archive exceeds the expanded-byte limit".into());
            }
            let mode = entry
                .header()
                .mode()
                .map_err(|error| format!("artifact archive file mode is invalid: {error}"))?;
            let mut digest = Sha256::new();
            let mut read_size = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = entry
                    .read(&mut buffer)
                    .map_err(|error| format!("could not hash artifact archive file: {error}"))?;
                if read == 0 {
                    break;
                }
                read_size = read_size
                    .checked_add(read as u64)
                    .ok_or_else(|| "artifact archive file size overflowed".to_owned())?;
                digest.update(&buffer[..read]);
            }
            if read_size != size {
                return Err("artifact archive file size changed while planning".into());
            }
            PlannedKind::File {
                executable: mode & 0o111 != 0,
                size,
                digest: digest.finalize().into(),
            }
        } else if entry_type.is_symlink() {
            if entry.size() != 0 {
                return Err("artifact archive symlink has a nonzero size".into());
            }
            let target = entry
                .link_name()
                .map_err(|error| format!("artifact symlink target is invalid: {error}"))?
                .ok_or_else(|| "artifact symlink omits its target".to_owned())?
                .into_owned();
            resolve_link(path.parent().unwrap_or_else(|| Path::new("")), &target)?;
            PlannedKind::Symlink { target }
        } else if entry_type.is_hard_link() {
            if entry.size() != 0 {
                return Err("artifact archive hardlink has a nonzero size".into());
            }
            let target = entry
                .link_name()
                .map_err(|error| format!("artifact hardlink target is invalid: {error}"))?
                .ok_or_else(|| "artifact hardlink omits its target".to_owned())?
                .into_owned();
            let target = resolve_link(Path::new(""), &target)?;
            PlannedKind::Hardlink { target }
        } else {
            return Err(
                "artifact archive contains a device, FIFO, or unsupported entry type".into(),
            );
        };
        plans.push(PlannedEntry { path, kind });
    }
    if plans.is_empty() {
        return Err("artifact archive is empty".into());
    }
    let entry_count = plans.len();
    Ok((
        plans,
        ArchiveSummary {
            entries: entry_count,
            expanded_bytes,
        },
    ))
}

fn validate_plan(plans: &[PlannedEntry]) -> Result<(), String> {
    let mut by_path = BTreeMap::new();
    for plan in plans {
        if by_path.insert(plan.path.clone(), &plan.kind).is_some() {
            return Err("artifact archive contains duplicate paths".into());
        }
    }
    for plan in plans {
        let mut ancestor = plan.path.parent();
        while let Some(path) = ancestor.filter(|path| !path.as_os_str().is_empty()) {
            if by_path
                .get(path)
                .is_some_and(|kind| !matches!(kind, PlannedKind::Directory))
            {
                return Err("artifact archive path descends through a non-directory entry".into());
            }
            ancestor = path.parent();
        }
        if let PlannedKind::Hardlink { target } = &plan.kind {
            if !by_path
                .get(target)
                .is_some_and(|kind| matches!(kind, PlannedKind::File { .. }))
            {
                return Err(
                    "artifact hardlink must reference a regular file in the archive".into(),
                );
            }
        }
    }
    Ok(())
}

fn extract_plans(
    archive_path: &Path,
    destination: &Path,
    plans: &[PlannedEntry],
) -> Result<(), String> {
    let directories = required_directories(plans);
    for directory in &directories {
        std::fs::create_dir_all(destination.join(directory))
            .map_err(|error| format!("could not create artifact directory: {error}"))?;
    }

    let expected = plans
        .iter()
        .map(|plan| (plan.path.clone(), &plan.kind))
        .collect::<BTreeMap<_, _>>();
    let file = File::open(archive_path)
        .map_err(|error| format!("could not reopen artifact archive: {error}"))?;
    let mut archive = tar::Archive::new(file);
    let mut has_explicit_root = false;
    let mut extracted_entries = 0_usize;
    for entry in archive
        .entries()
        .map_err(|error| format!("could not reread artifact archive: {error}"))?
    {
        let mut entry =
            entry.map_err(|error| format!("artifact archive entry is invalid: {error}"))?;
        let raw_path = entry
            .path()
            .map_err(|error| format!("artifact archive path is invalid: {error}"))?;
        let entry_type = entry.header().entry_type();
        if is_explicit_archive_root(&raw_path) {
            if has_explicit_root
                || extracted_entries != 0
                || !entry_type.is_dir()
                || entry.size() != 0
            {
                return Err("artifact archive changed its explicit root entry".into());
            }
            has_explicit_root = true;
            continue;
        }
        let path = normalized_archive_path(&raw_path)?;
        extracted_entries += 1;
        let Some(kind) = expected.get(&path) else {
            return Err("artifact archive changed between validation and extraction".into());
        };
        let PlannedKind::File {
            size,
            digest: expected_digest,
            ..
        } = kind
        else {
            continue;
        };
        let target = destination.join(&path);
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .map_err(|error| format!("could not create artifact file: {error}"))?;
        let mut copied = 0_u64;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = entry
                .read(&mut buffer)
                .map_err(|error| format!("could not extract artifact file: {error}"))?;
            if read == 0 {
                break;
            }
            copied = copied
                .checked_add(read as u64)
                .ok_or_else(|| "artifact extracted file size overflowed".to_owned())?;
            output
                .write_all(&buffer[..read])
                .map_err(|error| format!("could not write artifact file: {error}"))?;
            digest.update(&buffer[..read]);
        }
        if copied != *size {
            return Err("artifact file size changed during extraction".into());
        }
        if digest.finalize().as_slice() != *expected_digest {
            return Err("artifact file content changed during extraction".into());
        }
        output
            .sync_all()
            .map_err(|error| format!("could not sync artifact file: {error}"))?;
    }
    if extracted_entries != plans.len() {
        return Err("artifact archive changed between validation and extraction".into());
    }

    for plan in plans {
        if let PlannedKind::Hardlink { target } = &plan.kind {
            std::fs::hard_link(destination.join(target), destination.join(&plan.path))
                .map_err(|error| format!("could not create artifact hardlink: {error}"))?;
        }
    }
    for plan in plans {
        if let PlannedKind::Symlink { target } = &plan.kind {
            create_symlink(target, &destination.join(&plan.path))?;
        }
    }
    for plan in plans {
        if let PlannedKind::File { executable, .. } = plan.kind {
            set_read_only_mode(&destination.join(&plan.path), executable)?;
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        set_directory_mode(&destination.join(directory))?;
    }
    Ok(())
}

pub(super) fn seal_directory_root(path: &Path) -> Result<(), String> {
    set_directory_mode(path)
}

pub(super) fn verify_directory_archive(
    archive_path: &Path,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<ArchiveSummary, String> {
    if !archive_path.is_file() || !destination.is_dir() {
        return Err("artifact archive verification options are invalid".into());
    }
    let (plans, summary) = plan_archive(archive_path, limits)?;
    validate_plan(&plans)?;
    verify_extracted_tree(destination, &plans)?;
    Ok(summary)
}

fn verify_extracted_tree(destination: &Path, plans: &[PlannedEntry]) -> Result<(), String> {
    let root_metadata = std::fs::symlink_metadata(destination)
        .map_err(|error| format!("could not inspect artifact root: {error}"))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err("materialized artifact root is not a directory".into());
    }
    verify_directory_permissions(&root_metadata)?;

    let directories = required_directories(plans);
    let mut expected = directories.clone();
    expected.extend(plans.iter().map(|plan| plan.path.clone()));
    let mut actual = BTreeSet::new();
    collect_materialized_paths(destination, destination, &mut actual)?;
    if actual != expected {
        return Err("materialized artifact path inventory changed".into());
    }

    for directory in directories {
        let metadata = std::fs::symlink_metadata(destination.join(&directory))
            .map_err(|error| format!("could not inspect artifact directory: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("materialized artifact directory changed type".into());
        }
        verify_directory_permissions(&metadata)?;
    }
    for plan in plans {
        let path = destination.join(&plan.path);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect materialized artifact entry: {error}"))?;
        match &plan.kind {
            PlannedKind::Directory => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err("materialized artifact directory changed type".into());
                }
            }
            PlannedKind::File {
                executable,
                size,
                digest,
            } => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() != *size
                {
                    return Err("materialized artifact file changed type or size".into());
                }
                verify_file_permissions(&metadata, *executable)?;
                if hash_file(&path)? != *digest {
                    return Err("materialized artifact file changed content".into());
                }
            }
            PlannedKind::Symlink { target } => {
                if !metadata.file_type().is_symlink()
                    || std::fs::read_link(&path)
                        .map_err(|error| format!("could not read artifact symlink: {error}"))?
                        != *target
                {
                    return Err("materialized artifact symlink changed identity".into());
                }
            }
            PlannedKind::Hardlink { target } => {
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err("materialized artifact hardlink changed type".into());
                }
                verify_hardlink_identity(&metadata, &destination.join(target))?;
            }
        }
    }
    Ok(())
}

fn collect_materialized_paths(
    root: &Path,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("could not scan materialized artifact: {error}"))?
    {
        let entry = entry.map_err(|error| format!("could not scan artifact entry: {error}"))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "materialized artifact path escaped its root".to_owned())?
            .to_path_buf();
        if !paths.insert(relative) {
            return Err("materialized artifact contains a duplicate path".into());
        }
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect materialized artifact: {error}"))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            collect_materialized_paths(root, &path, paths)?;
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<[u8; 32], String> {
    let mut file = File::open(path)
        .map_err(|error| format!("could not open materialized artifact file: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("could not hash materialized artifact file: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

#[cfg(unix)]
fn verify_file_permissions(metadata: &std::fs::Metadata, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let expected = if executable { 0o555 } else { 0o444 };
    if metadata.permissions().mode() & 0o777 == expected {
        Ok(())
    } else {
        Err("materialized artifact file permissions changed".into())
    }
}

#[cfg(not(unix))]
fn verify_file_permissions(metadata: &std::fs::Metadata, _executable: bool) -> Result<(), String> {
    if metadata.permissions().readonly() {
        Ok(())
    } else {
        Err("materialized artifact file permissions changed".into())
    }
}

#[cfg(unix)]
fn verify_directory_permissions(metadata: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode() & 0o777 == 0o555 {
        Ok(())
    } else {
        Err("materialized artifact directory permissions changed".into())
    }
}

#[cfg(not(unix))]
fn verify_directory_permissions(metadata: &std::fs::Metadata) -> Result<(), String> {
    if metadata.permissions().readonly() {
        Ok(())
    } else {
        Err("materialized artifact directory permissions changed".into())
    }
}

#[cfg(unix)]
fn verify_hardlink_identity(metadata: &std::fs::Metadata, target: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let target = std::fs::symlink_metadata(target)
        .map_err(|error| format!("could not inspect artifact hardlink target: {error}"))?;
    if target.is_file()
        && !target.file_type().is_symlink()
        && metadata.dev() == target.dev()
        && metadata.ino() == target.ino()
    {
        Ok(())
    } else {
        Err("materialized artifact hardlink changed identity".into())
    }
}

#[cfg(not(unix))]
fn verify_hardlink_identity(metadata: &std::fs::Metadata, target: &Path) -> Result<(), String> {
    let target = std::fs::symlink_metadata(target)
        .map_err(|error| format!("could not inspect artifact hardlink target: {error}"))?;
    if target.is_file() && metadata.len() == target.len() {
        Ok(())
    } else {
        Err("materialized artifact hardlink changed identity".into())
    }
}

fn required_directories(plans: &[PlannedEntry]) -> BTreeSet<PathBuf> {
    let mut directories = BTreeSet::new();
    for plan in plans {
        if matches!(plan.kind, PlannedKind::Directory) {
            directories.insert(plan.path.clone());
        }
        let mut parent = plan.path.parent();
        while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
            directories.insert(path.to_path_buf());
            parent = path.parent();
        }
    }
    directories
}

fn normalized_archive_path(path: &Path) -> Result<PathBuf, String> {
    let text = path
        .to_str()
        .ok_or_else(|| "artifact archive path must be UTF-8".to_owned())?;
    if text.is_empty() || text.len() > 4096 || text.contains('\0') {
        return Err("artifact archive path is invalid".into());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("artifact archive path escapes its extraction root".into())
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("artifact archive path is empty".into());
    }
    Ok(normalized)
}

fn is_explicit_archive_root(path: &Path) -> bool {
    path.as_os_str().is_empty()
        || path
            .components()
            .all(|component| matches!(component, Component::CurDir))
}

fn resolve_link(base: &Path, target: &Path) -> Result<PathBuf, String> {
    let text = target
        .to_str()
        .ok_or_else(|| "artifact link target must be UTF-8".to_owned())?;
    if text.is_empty() || text.len() > 4096 || text.contains('\0') || target.is_absolute() {
        return Err("artifact link target is invalid".into());
    }
    let mut resolved = base
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for component in target.components() {
        match component {
            Component::Normal(value) => resolved.push(value.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved.pop().is_none() {
                    return Err("artifact link target escapes its extraction root".into());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("artifact link target escapes its extraction root".into())
            }
        }
    }
    if resolved.is_empty() {
        return Err("artifact link target resolves to the extraction root".into());
    }
    Ok(resolved.into_iter().collect())
}

#[cfg(unix)]
fn create_symlink(target: &Path, destination: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, destination)
        .map_err(|error| format!("could not create artifact symlink: {error}"))
}

#[cfg(not(unix))]
fn create_symlink(_target: &Path, _destination: &Path) -> Result<(), String> {
    Err("artifact symlinks require a Unix node".into())
}

#[cfg(unix)]
fn set_read_only_mode(path: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o555 } else { 0o444 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|error| format!("could not secure artifact file permissions: {error}"))
}

#[cfg(not(unix))]
fn set_read_only_mode(path: &Path, _executable: bool) -> Result<(), String> {
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect artifact file permissions: {error}"))?
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| format!("could not secure artifact file permissions: {error}"))
}

#[cfg(unix)]
fn set_directory_mode(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o555))
        .map_err(|error| format!("could not secure artifact directory permissions: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[derive(Clone, Copy)]
    struct Entry<'a> {
        path: &'a str,
        kind: tar::EntryType,
        link: Option<&'a str>,
        data: &'a [u8],
    }

    #[test]
    fn rejects_parent_and_absolute_paths() {
        for path in ["../escape", "/absolute"] {
            let error = extract(&archive(&[file(path, b"bad")]), generous_limits())
                .expect_err("escaping archive path");
            assert!(error.contains("escapes its extraction root"), "{error}");
        }
    }

    #[test]
    fn accepts_one_leading_explicit_root_without_changing_the_mount_shape() {
        let bytes = archive(&[
            Entry {
                path: ".",
                kind: tar::EntryType::Directory,
                link: None,
                data: b"",
            },
            file("./cache/index.json", b"{}"),
        ]);
        let temporary = tempfile::tempdir().expect("archive test directory");
        let archive_path = temporary.path().join("archive.tar");
        let destination = temporary.path().join("root");
        std::fs::write(&archive_path, bytes).expect("write archive");
        let summary = extract_directory_archive(&archive_path, &destination, generous_limits())
            .expect("explicit root archive");

        assert_eq!(summary.entries, 1);
        assert_eq!(
            std::fs::read(destination.join("cache/index.json"))
                .expect("read extracted cache index"),
            b"{}"
        );
        assert!(!destination.join("a3s-output").exists());

        let duplicate_root = archive(&[
            Entry {
                path: ".",
                kind: tar::EntryType::Directory,
                link: None,
                data: b"",
            },
            Entry {
                path: "./",
                kind: tar::EntryType::Directory,
                link: None,
                data: b"",
            },
            file("cache/index.json", b"{}"),
        ]);
        assert!(extract(&duplicate_root, generous_limits())
            .expect_err("duplicate explicit root")
            .contains("invalid explicit root"));
    }

    #[test]
    fn rejects_escaping_links_and_unsupported_entries() {
        let cases = [
            Entry {
                path: "escape",
                kind: tar::EntryType::symlink(),
                link: Some("../outside"),
                data: b"",
            },
            Entry {
                path: "escape",
                kind: tar::EntryType::hard_link(),
                link: Some("../outside"),
                data: b"",
            },
            Entry {
                path: "pipe",
                kind: tar::EntryType::fifo(),
                link: None,
                data: b"",
            },
            Entry {
                path: "device",
                kind: tar::EntryType::character_special(),
                link: None,
                data: b"",
            },
        ];
        for entry in cases {
            assert!(extract(&archive(&[entry]), generous_limits()).is_err());
        }
    }

    #[test]
    fn rejects_duplicate_paths_and_non_directory_ancestors() {
        let duplicate = archive(&[file("same", b"one"), file("same", b"two")]);
        assert!(extract(&duplicate, generous_limits())
            .expect_err("duplicate path")
            .contains("duplicate paths"));

        let ancestor = archive(&[
            Entry {
                path: "link",
                kind: tar::EntryType::symlink(),
                link: Some("target"),
                data: b"",
            },
            file("link/child", b"bad"),
            file("target", b"safe"),
        ]);
        assert!(extract(&ancestor, generous_limits())
            .expect_err("symlink ancestor")
            .contains("non-directory"));
    }

    #[test]
    fn enforces_entry_file_and_expanded_byte_limits() {
        let two_files = archive(&[file("one", b"1"), file("two", b"2")]);
        assert!(extract(
            &two_files,
            ArchiveLimits {
                max_entries: 1,
                ..generous_limits()
            }
        )
        .expect_err("entry limit")
        .contains("entry limit"));

        let large_file = archive(&[file("large", b"12345")]);
        assert!(extract(
            &large_file,
            ArchiveLimits {
                max_file_bytes: 4,
                max_expanded_bytes: 10,
                ..generous_limits()
            }
        )
        .expect_err("file limit")
        .contains("single-file limit"));

        assert!(extract(
            &two_files,
            ArchiveLimits {
                max_file_bytes: 1,
                max_expanded_bytes: 1,
                ..generous_limits()
            }
        )
        .expect_err("expanded limit")
        .contains("expanded-byte limit"));
    }

    #[cfg(unix)]
    #[test]
    fn extracts_internal_symlinks_and_hardlinks_read_only() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let archive = archive(&[
            file("dir/file", b"trusted"),
            Entry {
                path: "dir/symlink",
                kind: tar::EntryType::symlink(),
                link: Some("file"),
                data: b"",
            },
            Entry {
                path: "hardlink",
                kind: tar::EntryType::hard_link(),
                link: Some("dir/file"),
                data: b"",
            },
        ]);
        let temporary = tempfile::tempdir().expect("archive test directory");
        let archive_path = temporary.path().join("archive.tar");
        let destination = temporary.path().join("root");
        std::fs::write(&archive_path, archive).expect("write archive");
        let summary = extract_directory_archive(&archive_path, &destination, generous_limits())
            .expect("safe archive");
        seal_directory_root(&destination).expect("seal root");

        assert_eq!(summary.entries, 3);
        assert_eq!(summary.expanded_bytes, 7);
        assert_eq!(
            std::fs::read(destination.join("dir/symlink")).expect("read safe symlink"),
            b"trusted"
        );
        assert_eq!(
            std::fs::read(destination.join("hardlink")).expect("read safe hardlink"),
            b"trusted"
        );
        assert_eq!(
            std::fs::metadata(destination.join("dir/file"))
                .expect("source inode")
                .ino(),
            std::fs::metadata(destination.join("hardlink"))
                .expect("hardlink inode")
                .ino()
        );
        assert_eq!(
            std::fs::metadata(destination.join("dir/file"))
                .expect("file permissions")
                .permissions()
                .mode()
                & 0o777,
            0o444
        );
        make_test_tree_writable(&destination);
    }

    fn extract(bytes: &[u8], limits: ArchiveLimits) -> Result<ArchiveSummary, String> {
        let temporary = tempfile::tempdir().expect("archive test directory");
        let archive_path = temporary.path().join("archive.tar");
        let destination = temporary.path().join("root");
        std::fs::write(&archive_path, bytes).expect("write archive");
        let result = extract_directory_archive(&archive_path, &destination, limits);
        if destination.exists() {
            make_test_tree_writable(&destination);
        }
        result
    }

    fn archive(entries: &[Entry<'_>]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for entry in entries {
            let mut header = tar::Header::new_gnu();
            let name = entry.path.as_bytes();
            assert!(name.len() < 100);
            header.as_mut_bytes()[..name.len()].copy_from_slice(name);
            header.set_entry_type(entry.kind);
            header.set_mode(0o644);
            header.set_size(entry.data.len() as u64);
            if let Some(link) = entry.link {
                header
                    .set_link_name_literal(link.as_bytes())
                    .expect("set link target");
            }
            header.set_cksum();
            builder
                .append(&header, Cursor::new(entry.data))
                .expect("append raw archive entry");
        }
        builder.finish().expect("finish archive");
        builder.into_inner().expect("archive bytes")
    }

    fn file<'a>(path: &'a str, data: &'a [u8]) -> Entry<'a> {
        Entry {
            path,
            kind: tar::EntryType::file(),
            link: None,
            data,
        }
    }

    fn generous_limits() -> ArchiveLimits {
        ArchiveLimits {
            max_entries: 16,
            max_file_bytes: 1024,
            max_expanded_bytes: 4096,
        }
    }

    #[cfg(unix)]
    fn make_test_tree_writable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    make_test_tree_writable(&entry.path());
                }
            }
        }
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }

    #[cfg(not(unix))]
    fn make_test_tree_writable(_path: &Path) {}
}

#[cfg(not(unix))]
fn set_directory_mode(path: &Path) -> Result<(), String> {
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect artifact directory permissions: {error}"))?
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| format!("could not secure artifact directory permissions: {error}"))
}
