use super::require;
use std::collections::BTreeSet;
use std::error::Error;
use std::io::{Cursor, Read};
use std::path::{Component, Path};

const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_024;
const ELF_IDENTITY_BYTES: usize = 20;

pub(super) fn validate_busybox_rootfs(bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    require(
        !bytes.is_empty() && bytes.len() <= MAX_ARCHIVE_BYTES,
        "Runtime BuildKit gate BusyBox rootfs is empty or exceeds its archive bound",
    )?;
    let mut archive = tar::Archive::new(Cursor::new(bytes));
    let mut entries = 0_usize;
    let mut expanded_bytes = 0_u64;
    let mut paths = BTreeSet::new();
    let mut busybox_path_is_bound = false;
    let mut has_busybox_elf = false;
    let mut has_loader_elf = false;
    let mut has_libc_elf = false;
    let mut has_lib64_link = false;
    for entry in archive.entries()? {
        let mut entry = entry?;
        entries = entries
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("BusyBox rootfs entry count overflowed"))?;
        require(
            entries <= MAX_ENTRIES,
            "Runtime BuildKit gate BusyBox rootfs has too many entries",
        )?;
        let path = entry.path()?.into_owned();
        require(
            archive_path_is_internal(&path),
            format!("BusyBox rootfs path {path:?} escapes or is invalid"),
        )?;
        require(
            paths.insert(path.clone()),
            format!("BusyBox rootfs path {path:?} is duplicated"),
        )?;
        let entry_type = entry.header().entry_type();
        require(
            entry_type.is_file()
                || entry_type.is_dir()
                || entry_type.is_symlink()
                || entry_type.is_hard_link(),
            format!("BusyBox rootfs path {path:?} has an unsupported entry type"),
        )?;
        if entry_type.is_file() {
            expanded_bytes = expanded_bytes
                .checked_add(entry.size())
                .ok_or_else(|| std::io::Error::other("BusyBox rootfs size overflowed"))?;
            require(
                expanded_bytes <= MAX_EXPANDED_BYTES,
                "Runtime BuildKit gate BusyBox rootfs exceeds its expanded-byte bound",
            )?;
        } else {
            require(
                entry.size() == 0,
                format!("BusyBox rootfs non-file path {path:?} has content"),
            )?;
        }
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            let target = entry.link_name()?.ok_or_else(|| {
                std::io::Error::other(format!("BusyBox rootfs link {path:?} omits its target"))
            })?;
            let known_runtime_link = path == Path::new("etc/mtab")
                && entry_type.is_symlink()
                && target == Path::new("/proc/mounts");
            require(
                known_runtime_link
                    || archive_link_is_internal(&path, &target, entry_type.is_symlink()),
                format!("BusyBox rootfs link {path:?} escapes its root"),
            )?;
            if path == Path::new("lib64") && entry_type.is_symlink() && target == Path::new("lib") {
                has_lib64_link = true;
            }
            if path == Path::new("bin/busybox")
                && entry_type.is_hard_link()
                && target == Path::new("bin/[")
            {
                busybox_path_is_bound = true;
            }
        }
        if entry_type.is_file()
            && matches!(
                path.to_str(),
                Some("bin/[")
                    | Some("bin/busybox")
                    | Some("lib/ld-linux-x86-64.so.2")
                    | Some("lib/libc.so.6")
            )
        {
            let mode = entry.header().mode()?;
            let mut identity = [0_u8; ELF_IDENTITY_BYTES];
            entry.read_exact(&mut identity)?;
            let executable = is_linux_amd64_elf(&identity) && mode & 0o111 != 0;
            match path.to_str() {
                Some("bin/[") => has_busybox_elf |= executable,
                Some("bin/busybox") => {
                    has_busybox_elf |= executable;
                    busybox_path_is_bound |= executable;
                }
                Some("lib/ld-linux-x86-64.so.2") => has_loader_elf |= executable,
                Some("lib/libc.so.6") => has_libc_elf |= executable,
                _ => unreachable!("expected ELF path was checked above"),
            }
        }
    }
    require(entries > 0, "Runtime BuildKit gate BusyBox rootfs is empty")?;
    require(
        busybox_path_is_bound
            && has_busybox_elf
            && has_loader_elf
            && has_libc_elf
            && has_lib64_link,
        "Runtime BuildKit gate BusyBox rootfs omits its executable or glibc runtime closure",
    )?;
    Ok(())
}

fn is_linux_amd64_elf(identity: &[u8; ELF_IDENTITY_BYTES]) -> bool {
    identity[..4] == *b"\x7fELF"
        && identity[4] == 2
        && identity[5] == 1
        && identity[18..20] == [0x3e, 0x00]
}

fn archive_path_is_internal(path: &Path) -> bool {
    path.to_str().is_some_and(|value| {
        !value.is_empty()
            && value.len() <= 4_096
            && !value.contains('\0')
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
    })
}

fn archive_link_is_internal(path: &Path, target: &Path, relative_to_parent: bool) -> bool {
    let Some(text) = target.to_str() else {
        return false;
    };
    if text.is_empty() || text.len() > 4_096 || text.contains('\0') || target.is_absolute() {
        return false;
    }
    let mut depth = if relative_to_parent {
        path.parent()
            .into_iter()
            .flat_map(Path::components)
            .filter(|component| matches!(component, Component::Normal(_)))
            .count()
    } else {
        0
    };
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    depth > 0
}

#[test]
fn fixture_is_bounded_and_cannot_escape() -> Result<(), Box<dyn Error>> {
    validate_busybox_rootfs(&busybox_rootfs_fixture("lib", "/proc/mounts")?)?;
    let error = validate_busybox_rootfs(&busybox_rootfs_fixture("../../escape", "/proc/mounts")?)
        .expect_err("escaping BusyBox rootfs link must fail");
    assert!(error.to_string().contains("escapes its root"));
    let error = validate_busybox_rootfs(&busybox_rootfs_fixture("lib", "/proc/self/mounts")?)
        .expect_err("unrecognized absolute BusyBox rootfs link must fail");
    assert!(error.to_string().contains("escapes its root"));
    Ok(())
}

fn busybox_rootfs_fixture(
    lib64_target: &str,
    mtab_target: &str,
) -> Result<Vec<u8>, std::io::Error> {
    let mut builder = tar::Builder::new(Vec::new());
    append_fixture_file(&mut builder, "bin/[", &fixture_amd64_elf())?;
    append_fixture_link(&mut builder, "bin/busybox", "bin/[", tar::EntryType::Link)?;
    append_fixture_file(
        &mut builder,
        "lib/ld-linux-x86-64.so.2",
        &fixture_amd64_elf(),
    )?;
    append_fixture_file(&mut builder, "lib/libc.so.6", &fixture_amd64_elf())?;
    append_fixture_link(&mut builder, "lib64", lib64_target, tar::EntryType::Symlink)?;
    append_fixture_link(
        &mut builder,
        "etc/mtab",
        mtab_target,
        tar::EntryType::Symlink,
    )?;
    builder.into_inner()
}

fn fixture_amd64_elf() -> [u8; ELF_IDENTITY_BYTES] {
    let mut identity = [0_u8; ELF_IDENTITY_BYTES];
    identity[..6].copy_from_slice(b"\x7fELF\x02\x01");
    identity[18..20].copy_from_slice(&[0x3e, 0x00]);
    identity
}

fn append_fixture_file(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &str,
    bytes: &[u8],
) -> Result<(), std::io::Error> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(bytes.len() as u64);
    header.set_cksum();
    builder.append_data(&mut header, path, Cursor::new(bytes))
}

fn append_fixture_link(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &str,
    target: &str,
    entry_type: tar::EntryType,
) -> Result<(), std::io::Error> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(entry_type);
    header.set_mode(0o777);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(0);
    header.set_link_name(target)?;
    header.set_cksum();
    builder.append_data(&mut header, path, Cursor::new([]))
}
