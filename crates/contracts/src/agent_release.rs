use crate::node::validate_lower_sha256;
use uuid::Uuid;

pub const AGENT_RELEASE_MANIFEST_ARCHIVE_PATH: &str = "asset.acl";

/// Stable Cloud provenance URI for the exact staged source bytes.
pub fn agent_release_source_uri(source_digest: &str) -> Result<String, String> {
    validate_lower_sha256("Agent release source digest", source_digest)?;
    Ok(format!(
        "urn:a3s:cloud:source-content:{}",
        source_digest.trim_start_matches("sha256:")
    ))
}

/// Stable Cloud provenance URI for the build evidence that binds a release.
pub fn agent_release_builder_uri(build_run_id: Uuid) -> Result<String, String> {
    if build_run_id.is_nil() {
        return Err("Agent release build identity cannot be nil".into());
    }
    Ok(format!("urn:a3s:cloud:build-evidence:{build_run_id}"))
}

/// Encode the final Code manifest as the exact read-only directory artifact
/// mounted by Runtime at `/app/.a3s`.
pub fn agent_release_manifest_archive(canonical_acl: &[u8]) -> Result<Vec<u8>, String> {
    if canonical_acl.is_empty() {
        return Err("Agent release manifest archive cannot be empty".into());
    }
    let mut archive = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(canonical_acl.len() as u64);
    header.set_mode(0o444);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive
        .append_data(
            &mut header,
            AGENT_RELEASE_MANIFEST_ARCHIVE_PATH,
            canonical_acl,
        )
        .map_err(|error| format!("could not encode Agent release manifest archive: {error}"))?;
    archive
        .into_inner()
        .map_err(|error| format!("could not finish Agent release manifest archive: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn archive_and_provenance_identities_are_canonical_and_deterministic() {
        let bytes = b"agent_release {}\n";
        let first = agent_release_manifest_archive(bytes).expect("archive");
        let second = agent_release_manifest_archive(bytes).expect("archive replay");
        assert_eq!(first, second);

        let mut archive = tar::Archive::new(first.as_slice());
        let mut entries = archive.entries().expect("archive entries");
        let mut entry = entries.next().expect("manifest entry").expect("entry");
        assert_eq!(
            entry.path().expect("entry path").as_ref(),
            std::path::Path::new(AGENT_RELEASE_MANIFEST_ARCHIVE_PATH)
        );
        assert_eq!(entry.header().mode().expect("mode"), 0o444);
        assert_eq!(entry.header().uid().expect("uid"), 0);
        assert_eq!(entry.header().gid().expect("gid"), 0);
        assert_eq!(entry.header().mtime().expect("mtime"), 0);
        let mut content = Vec::new();
        entry.read_to_end(&mut content).expect("manifest content");
        assert_eq!(content, bytes);
        drop(entry);
        assert!(entries.next().is_none());

        let digest = format!("sha256:{}", "a".repeat(64));
        assert!(agent_release_source_uri(&digest)
            .expect("source URI")
            .ends_with(&"a".repeat(64)));
        assert!(agent_release_builder_uri(Uuid::nil()).is_err());
    }
}
