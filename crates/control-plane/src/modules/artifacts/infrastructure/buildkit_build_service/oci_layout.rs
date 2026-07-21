mod filesystem;

use self::filesystem::{
    read_regular_file, remove_empty_ingest_directory, require_owned_directory,
    validate_blob_inventory, validate_root_entries,
};
use crate::modules::artifacts::domain::{
    BuildServiceError, OciDescriptor, OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE,
};
use crate::modules::sources::domain::BuildPlatform;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::Path;
use tokio::io::AsyncReadExt;

const OCI_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";
const OCI_LAYER_MEDIA_TYPES: [&str; 6] = [
    "application/vnd.oci.image.layer.v1.tar",
    "application/vnd.oci.image.layer.v1.tar+gzip",
    "application/vnd.oci.image.layer.v1.tar+zstd",
    "application/vnd.oci.image.layer.nondistributable.v1.tar",
    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
    "application/vnd.oci.image.layer.nondistributable.v1.tar+zstd",
];
const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_GRAPH_DEPTH: usize = 8;
const MAX_OUTPUT_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(super) struct OciLayoutLimits {
    max_blobs: usize,
    max_bytes: u64,
}

impl OciLayoutLimits {
    pub(super) fn new(max_blobs: usize, max_bytes: u64) -> Result<Self, String> {
        if max_blobs == 0 || max_blobs > 1_000_000 || max_bytes == 0 || max_bytes > MAX_OUTPUT_BYTES
        {
            return Err("OCI layout limits are invalid".into());
        }
        Ok(Self {
            max_blobs,
            max_bytes,
        })
    }
}

#[derive(Debug)]
pub(super) struct ValidatedOciLayout {
    pub(super) platforms: Vec<BuildPlatform>,
    pub(super) content_bytes: u64,
    pub(super) blob_count: usize,
    pub(super) blobs: Vec<OciLayoutBlob>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::modules::artifacts::infrastructure) struct OciLayoutBlob {
    pub(in crate::modules::artifacts::infrastructure) media_type: String,
    pub(in crate::modules::artifacts::infrastructure) digest: String,
    pub(in crate::modules::artifacts::infrastructure) size: u64,
    pub(in crate::modules::artifacts::infrastructure) depth: usize,
}

impl OciLayoutBlob {
    pub(in crate::modules::artifacts::infrastructure) fn is_manifest(&self) -> bool {
        matches!(
            self.media_type.as_str(),
            OCI_IMAGE_INDEX_MEDIA_TYPE | OCI_IMAGE_MANIFEST_MEDIA_TYPE
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDescriptor {
    media_type: String,
    digest: String,
    size: u64,
    #[serde(default)]
    platform: Option<RawPlatform>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawPlatform {
    architecture: String,
    os: String,
    #[serde(default)]
    variant: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciLayoutMarker {
    image_layout_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciIndex {
    schema_version: u64,
    #[serde(default)]
    media_type: Option<String>,
    manifests: Vec<RawDescriptor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciManifest {
    schema_version: u64,
    #[serde(default)]
    media_type: Option<String>,
    config: RawDescriptor,
    layers: Vec<RawDescriptor>,
}

#[derive(Deserialize)]
struct OciImageConfig {
    architecture: String,
    os: String,
    #[serde(default)]
    variant: Option<String>,
}

struct PendingDescriptor {
    descriptor: RawDescriptor,
    depth: usize,
    platform_hint: Option<RawPlatform>,
}

struct ValidationState {
    limits: OciLayoutLimits,
    seen: HashMap<String, (String, u64)>,
    dependencies: HashMap<String, Vec<String>>,
    config_platforms: HashMap<String, BuildPlatform>,
    platforms: BTreeSet<BuildPlatform>,
    content_bytes: u64,
}

pub(super) async fn validate_oci_layout(
    layout: &Path,
    expected_root: &OciDescriptor,
    expected_platforms: &[BuildPlatform],
    limits: OciLayoutLimits,
) -> Result<ValidatedOciLayout, BuildServiceError> {
    require_owned_directory(layout, "OCI layout directory").await?;
    validate_root_entries(layout).await?;
    let marker = read_regular_file(&layout.join("oci-layout"), 4096).await?;
    let index = read_regular_file(&layout.join("index.json"), MAX_JSON_BYTES).await?;
    let mut state = ValidationState {
        limits,
        seen: HashMap::new(),
        dependencies: HashMap::new(),
        config_platforms: HashMap::new(),
        platforms: BTreeSet::new(),
        content_bytes: 0,
    };
    state.add_bytes(marker.len() as u64)?;
    state.add_bytes(index.len() as u64)?;

    let marker: OciLayoutMarker =
        serde_json::from_slice(&marker).map_err(|_| integrity("OCI layout marker is invalid"))?;
    if marker.image_layout_version != "1.0.0" {
        return Err(integrity("OCI image layout version is unsupported"));
    }
    let index: OciIndex =
        serde_json::from_slice(&index).map_err(|_| integrity("OCI index is invalid"))?;
    if index.schema_version != 2
        || index
            .media_type
            .as_deref()
            .is_some_and(|value| value != OCI_IMAGE_INDEX_MEDIA_TYPE)
        || index.manifests.len() != 1
    {
        return Err(integrity("OCI index root is invalid"));
    }
    let root = index
        .manifests
        .into_iter()
        .next()
        .ok_or_else(|| integrity("OCI index omitted its image root"))?;
    if root.media_type != expected_root.media_type()
        || root.digest != expected_root.digest()
        || root.size != expected_root.size()
    {
        return Err(integrity("OCI index root does not match BuildKit metadata"));
    }
    let root_hint = root.platform.clone();
    let mut pending = VecDeque::from([PendingDescriptor {
        descriptor: root,
        depth: 0,
        platform_hint: root_hint,
    }]);
    while let Some(item) = pending.pop_front() {
        validate_descriptor(&item.descriptor)?;
        if item.depth > MAX_GRAPH_DEPTH {
            return Err(integrity("OCI descriptor graph exceeds its depth bound"));
        }
        if let Some((media_type, size)) = state.seen.get(&item.descriptor.digest) {
            if media_type != &item.descriptor.media_type || *size != item.descriptor.size {
                return Err(integrity(
                    "OCI digest is reused with conflicting descriptor metadata",
                ));
            }
            if item.descriptor.media_type == OCI_CONFIG_MEDIA_TYPE {
                let platform = state
                    .config_platforms
                    .get(&item.descriptor.digest)
                    .ok_or_else(|| integrity("OCI config platform was not validated"))?;
                validate_platform_hint(item.platform_hint.as_ref(), platform)?;
            }
            continue;
        }
        if state.seen.len() >= state.limits.max_blobs {
            return Err(integrity("OCI descriptor graph exceeds its blob bound"));
        }
        state.seen.insert(
            item.descriptor.digest.clone(),
            (item.descriptor.media_type.clone(), item.descriptor.size),
        );
        let is_json = matches!(
            item.descriptor.media_type.as_str(),
            OCI_IMAGE_INDEX_MEDIA_TYPE | OCI_IMAGE_MANIFEST_MEDIA_TYPE | OCI_CONFIG_MEDIA_TYPE
        );
        let content = read_verified_blob(layout, &item.descriptor, is_json, &mut state).await?;
        match item.descriptor.media_type.as_str() {
            OCI_IMAGE_INDEX_MEDIA_TYPE => {
                let index: OciIndex = serde_json::from_slice(&content)
                    .map_err(|_| integrity("OCI image-index blob is invalid"))?;
                if index.schema_version != 2
                    || index
                        .media_type
                        .as_deref()
                        .is_some_and(|value| value != OCI_IMAGE_INDEX_MEDIA_TYPE)
                    || index.manifests.is_empty()
                {
                    return Err(integrity("OCI image-index blob is invalid"));
                }
                state.dependencies.insert(
                    item.descriptor.digest.clone(),
                    index
                        .manifests
                        .iter()
                        .map(|descriptor| descriptor.digest.clone())
                        .collect(),
                );
                for descriptor in index.manifests {
                    let platform_hint = descriptor.platform.clone();
                    pending.push_back(PendingDescriptor {
                        descriptor,
                        depth: item.depth + 1,
                        platform_hint,
                    });
                }
            }
            OCI_IMAGE_MANIFEST_MEDIA_TYPE => {
                let manifest: OciManifest = serde_json::from_slice(&content)
                    .map_err(|_| integrity("OCI image-manifest blob is invalid"))?;
                if manifest.schema_version != 2
                    || manifest
                        .media_type
                        .as_deref()
                        .is_some_and(|value| value != OCI_IMAGE_MANIFEST_MEDIA_TYPE)
                    || manifest.config.media_type != OCI_CONFIG_MEDIA_TYPE
                {
                    return Err(integrity("OCI image-manifest blob is invalid"));
                }
                state.dependencies.insert(
                    item.descriptor.digest.clone(),
                    std::iter::once(manifest.config.digest.clone())
                        .chain(
                            manifest
                                .layers
                                .iter()
                                .map(|descriptor| descriptor.digest.clone()),
                        )
                        .collect(),
                );
                pending.push_back(PendingDescriptor {
                    descriptor: manifest.config,
                    depth: item.depth + 1,
                    platform_hint: item.platform_hint,
                });
                for layer in manifest.layers {
                    pending.push_back(PendingDescriptor {
                        descriptor: layer,
                        depth: item.depth + 1,
                        platform_hint: None,
                    });
                }
            }
            OCI_CONFIG_MEDIA_TYPE => {
                let config: OciImageConfig = serde_json::from_slice(&content)
                    .map_err(|_| integrity("OCI image-config blob is invalid"))?;
                let platform =
                    parse_platform(&config.os, &config.architecture, config.variant.as_deref())?;
                validate_platform_hint(item.platform_hint.as_ref(), &platform)?;
                state
                    .config_platforms
                    .insert(item.descriptor.digest, platform.clone());
                state.platforms.insert(platform);
            }
            media_type if OCI_LAYER_MEDIA_TYPES.contains(&media_type) => {}
            _ => return Err(integrity("OCI descriptor media type is unsupported")),
        }
    }

    validate_blob_inventory(layout, &state.seen).await?;
    let expected = expected_platforms.iter().cloned().collect::<BTreeSet<_>>();
    if state.platforms != expected {
        return Err(integrity(
            "OCI image platforms do not match the accepted build recipe",
        ));
    }
    let depths = descriptor_depths(expected_root.digest(), &state.dependencies)?;
    let mut blobs = Vec::with_capacity(state.seen.len());
    for (digest, (media_type, size)) in &state.seen {
        let depth = depths
            .get(digest)
            .copied()
            .ok_or_else(|| integrity("OCI descriptor graph contains an unreachable blob"))?;
        blobs.push(OciLayoutBlob {
            media_type: media_type.clone(),
            digest: digest.clone(),
            size: *size,
            depth,
        });
    }
    blobs.sort_by(|left, right| left.digest.cmp(&right.digest));
    Ok(ValidatedOciLayout {
        platforms: state.platforms.into_iter().collect(),
        content_bytes: state.content_bytes,
        blob_count: state.seen.len(),
        blobs,
    })
}

pub(super) fn descriptor_depths(
    root: &str,
    dependencies: &HashMap<String, Vec<String>>,
) -> Result<HashMap<String, usize>, BuildServiceError> {
    let mut depths = HashMap::from([(root.to_owned(), 0_usize)]);
    let mut pending = VecDeque::from([root.to_owned()]);
    while let Some(parent) = pending.pop_front() {
        let parent_depth = depths.get(&parent).copied().unwrap_or_default();
        for child in dependencies.get(&parent).into_iter().flatten() {
            let depth = parent_depth
                .checked_add(1)
                .ok_or_else(|| integrity("OCI descriptor graph depth overflowed"))?;
            if depth > MAX_GRAPH_DEPTH {
                return Err(integrity("OCI descriptor graph exceeds its depth bound"));
            }
            let current = depths.entry(child.clone()).or_default();
            if depth > *current {
                *current = depth;
                pending.push_back(child.clone());
            }
        }
    }
    Ok(depths)
}

pub(super) async fn normalize_buildctl_layout(layout: &Path) -> Result<(), BuildServiceError> {
    remove_empty_ingest_directory(layout).await
}

impl ValidationState {
    fn add_bytes(&mut self, count: u64) -> Result<(), BuildServiceError> {
        self.content_bytes = self
            .content_bytes
            .checked_add(count)
            .ok_or_else(|| integrity("OCI output byte count overflowed"))?;
        if self.content_bytes > self.limits.max_bytes {
            return Err(integrity("OCI output exceeds its byte bound"));
        }
        Ok(())
    }
}

async fn read_verified_blob(
    layout: &Path,
    descriptor: &RawDescriptor,
    collect: bool,
    state: &mut ValidationState,
) -> Result<Vec<u8>, BuildServiceError> {
    if collect && descriptor.size > MAX_JSON_BYTES {
        return Err(integrity("OCI JSON blob exceeds its bound"));
    }
    let digest = descriptor
        .digest
        .strip_prefix("sha256:")
        .ok_or_else(|| integrity("OCI descriptor digest is invalid"))?;
    let path = layout.join("blobs/sha256").join(digest);
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|_| integrity("OCI descriptor blob is unavailable"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != descriptor.size
    {
        return Err(integrity(
            "OCI descriptor blob does not match its declared size",
        ));
    }
    state.add_bytes(metadata.len())?;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| storage("could not read OCI descriptor blob"))?;
    let mut hasher = Sha256::new();
    let mut content = if collect {
        Vec::with_capacity(metadata.len() as usize)
    } else {
        Vec::new()
    };
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|_| storage("could not read OCI descriptor blob"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        if collect {
            content.extend_from_slice(&buffer[..read]);
        }
    }
    if format!("sha256:{:x}", hasher.finalize()) != descriptor.digest {
        return Err(integrity("OCI descriptor blob digest does not match"));
    }
    Ok(content)
}

fn validate_descriptor(descriptor: &RawDescriptor) -> Result<(), BuildServiceError> {
    if descriptor.size == 0
        || !descriptor
            .digest
            .strip_prefix("sha256:")
            .is_some_and(valid_hex_digest)
    {
        return Err(integrity("OCI descriptor is invalid"));
    }
    if !matches!(
        descriptor.media_type.as_str(),
        OCI_IMAGE_INDEX_MEDIA_TYPE | OCI_IMAGE_MANIFEST_MEDIA_TYPE | OCI_CONFIG_MEDIA_TYPE
    ) && !OCI_LAYER_MEDIA_TYPES.contains(&descriptor.media_type.as_str())
    {
        return Err(integrity("OCI descriptor media type is unsupported"));
    }
    Ok(())
}

fn validate_platform_hint(
    hint: Option<&RawPlatform>,
    actual: &BuildPlatform,
) -> Result<(), BuildServiceError> {
    if let Some(hint) = hint {
        let hint = parse_platform(&hint.os, &hint.architecture, hint.variant.as_deref())?;
        if &hint != actual {
            return Err(integrity(
                "OCI descriptor platform does not match its image config",
            ));
        }
    }
    Ok(())
}

fn parse_platform(
    os: &str,
    architecture: &str,
    variant: Option<&str>,
) -> Result<BuildPlatform, BuildServiceError> {
    let valid_variant = matches!(
        (architecture, variant),
        ("amd64", None | Some("")) | ("arm64", None | Some("") | Some("v8"))
    );
    if os != "linux" || !valid_variant {
        return Err(integrity("OCI image platform is unsupported"));
    }
    BuildPlatform::parse(format!("{os}/{architecture}")).map_err(integrity)
}

fn valid_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn integrity(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Integrity(message.into())
}

fn storage(message: impl Into<String>) -> BuildServiceError {
    BuildServiceError::Storage(message.into())
}
