use crate::modules::artifacts::domain::{
    BuildOutputValidationError, OciDescriptor, BUILD_CACHE_SCHEMA,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

const OCI_LAYOUT_VERSION: &str = "1.0.0";
const OCI_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const BUILDKIT_CACHE_CONFIG_MEDIA_TYPE: &str = "application/vnd.buildkit.cacheconfig.v0";
const MAX_CACHE_RECEIPT_BYTES: u64 = 4 * 1024;
const MAX_CACHE_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ANNOTATIONS: usize = 64;

pub(super) struct ValidatedCacheLayout {
    pub descriptor: OciDescriptor,
    pub content_bytes: u64,
    pub blob_count: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CacheReceipt {
    schema: String,
    key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OciLayout {
    image_layout_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OciIndex {
    schema_version: u32,
    media_type: String,
    manifests: Vec<Descriptor>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    #[serde(default)]
    artifact_type: Option<String>,
    #[serde(default)]
    subject: Option<Descriptor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OciManifest {
    schema_version: u32,
    media_type: String,
    config: Descriptor,
    layers: Vec<Descriptor>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    #[serde(default)]
    artifact_type: Option<String>,
    #[serde(default)]
    subject: Option<Descriptor>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Descriptor {
    media_type: String,
    digest: String,
    size: u64,
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    platform: Option<Value>,
    #[serde(default)]
    artifact_type: Option<String>,
}

pub(super) async fn validate_exported_cache(
    export_root: &Path,
    expected_key: &str,
    max_blobs: usize,
    max_bytes: u64,
) -> Result<ValidatedCacheLayout, BuildOutputValidationError> {
    validate_digest(expected_key, "expected build cache key")?;
    if max_blobs == 0 || max_bytes == 0 {
        return Err(invalid("build cache validation limits are invalid"));
    }
    let receipt_bytes = read_bounded(
        &export_root.join("build-cache.json"),
        MAX_CACHE_RECEIPT_BYTES,
        "build cache receipt",
    )
    .await?;
    let receipt: CacheReceipt = serde_json::from_slice(&receipt_bytes)
        .map_err(|_| integrity("build cache receipt is invalid"))?;
    if receipt.schema != BUILD_CACHE_SCHEMA || receipt.key != expected_key {
        return Err(integrity(
            "build cache receipt changed its immutable input identity",
        ));
    }

    let cache_root = export_root.join("cache");
    require_exact_cache_root(&cache_root)?;
    let layout_bytes = read_bounded(
        &cache_root.join("oci-layout"),
        1024,
        "build cache OCI layout",
    )
    .await?;
    let layout: OciLayout = serde_json::from_slice(&layout_bytes)
        .map_err(|_| integrity("build cache OCI layout is invalid"))?;
    if layout.image_layout_version != OCI_LAYOUT_VERSION {
        return Err(integrity("build cache OCI layout version is unsupported"));
    }

    let index_bytes = read_bounded(
        &cache_root.join("index.json"),
        MAX_CACHE_JSON_BYTES.min(max_bytes),
        "build cache OCI index",
    )
    .await?;
    let index: OciIndex = serde_json::from_slice(&index_bytes)
        .map_err(|_| integrity("build cache OCI index is invalid"))?;
    if index.schema_version != 2
        || index.media_type != OCI_INDEX_MEDIA_TYPE
        || index.manifests.len() != 1
        || index.artifact_type.is_some()
        || index.subject.is_some()
    {
        return Err(integrity("build cache OCI index shape is unsupported"));
    }
    validate_annotations(&index.annotations)?;

    let mut pending = VecDeque::new();
    for descriptor in index.manifests {
        validate_descriptor(&descriptor, max_bytes)?;
        if descriptor.media_type != OCI_MANIFEST_MEDIA_TYPE {
            return Err(integrity(
                "build cache index references an unsupported manifest",
            ));
        }
        pending.push_back(descriptor);
    }

    let blob_root = cache_root.join("blobs/sha256");
    let mut visited = BTreeMap::<String, (String, u64)>::new();
    let mut content_bytes = 0_u64;
    while let Some(descriptor) = pending.pop_front() {
        if let Some((media_type, size)) = visited.get(&descriptor.digest) {
            if media_type != &descriptor.media_type || *size != descriptor.size {
                return Err(integrity(
                    "build cache reuses one digest with conflicting metadata",
                ));
            }
            continue;
        }
        if visited.len() >= max_blobs {
            return Err(invalid("build cache exceeds its blob-count bound"));
        }
        let bytes = read_descriptor_blob(&blob_root, &descriptor, max_bytes).await?;
        content_bytes = content_bytes
            .checked_add(descriptor.size)
            .ok_or_else(|| invalid("build cache content size overflowed"))?;
        if content_bytes > max_bytes {
            return Err(invalid("build cache exceeds its content-byte bound"));
        }
        visited.insert(
            descriptor.digest.clone(),
            (descriptor.media_type.clone(), descriptor.size),
        );

        match descriptor.media_type.as_str() {
            OCI_MANIFEST_MEDIA_TYPE => {
                let manifest: OciManifest = serde_json::from_slice(&bytes)
                    .map_err(|_| integrity("build cache OCI manifest is invalid"))?;
                if manifest.schema_version != 2
                    || manifest.media_type != OCI_MANIFEST_MEDIA_TYPE
                    || manifest.config.media_type != BUILDKIT_CACHE_CONFIG_MEDIA_TYPE
                    || manifest.layers.is_empty()
                    || manifest.artifact_type.is_some()
                    || manifest.subject.is_some()
                {
                    return Err(integrity("build cache OCI manifest shape is unsupported"));
                }
                validate_annotations(&manifest.annotations)?;
                validate_descriptor(&manifest.config, max_bytes)?;
                pending.push_back(manifest.config);
                for layer in manifest.layers {
                    validate_descriptor(&layer, max_bytes)?;
                    if !is_cache_layer_media_type(&layer.media_type) {
                        return Err(integrity(
                            "build cache manifest references an unsupported layer",
                        ));
                    }
                    pending.push_back(layer);
                }
            }
            BUILDKIT_CACHE_CONFIG_MEDIA_TYPE => {
                let config: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| integrity("BuildKit cache configuration is invalid"))?;
                if !config.is_object() {
                    return Err(integrity(
                        "BuildKit cache configuration must be a JSON object",
                    ));
                }
            }
            media_type if is_cache_layer_media_type(media_type) => {}
            _ => {
                return Err(integrity(
                    "build cache graph contains an unsupported descriptor",
                ))
            }
        }
    }

    let inventory = blob_inventory(&blob_root)?;
    let referenced = visited
        .keys()
        .map(|digest| {
            digest
                .strip_prefix("sha256:")
                .expect("validated digest")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    if inventory != referenced {
        return Err(integrity(
            "build cache contains missing or unreferenced content",
        ));
    }

    let descriptor = OciDescriptor::new(
        OCI_INDEX_MEDIA_TYPE,
        format!("sha256:{:x}", Sha256::digest(&index_bytes)),
        index_bytes.len() as u64,
    )
    .map_err(BuildOutputValidationError::Integrity)?;
    Ok(ValidatedCacheLayout {
        descriptor,
        content_bytes,
        blob_count: visited.len(),
    })
}

async fn read_descriptor_blob(
    root: &Path,
    descriptor: &Descriptor,
    max_bytes: u64,
) -> Result<Vec<u8>, BuildOutputValidationError> {
    let digest = validate_digest(&descriptor.digest, "build cache descriptor digest")?;
    let bytes = read_bounded(
        &root.join(digest),
        descriptor.size.min(max_bytes),
        "build cache blob",
    )
    .await?;
    if bytes.len() as u64 != descriptor.size
        || format!("sha256:{:x}", Sha256::digest(&bytes)) != descriptor.digest
    {
        return Err(integrity("build cache blob does not match its descriptor"));
    }
    Ok(bytes)
}

async fn read_bounded(
    path: &Path,
    maximum: u64,
    label: &str,
) -> Result<Vec<u8>, BuildOutputValidationError> {
    if maximum == 0 {
        return Err(invalid(format!("{label} has an invalid byte bound")));
    }
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| storage(format!("could not inspect {label}: {error}")))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > maximum {
        return Err(integrity(format!(
            "{label} is not a bounded owned regular file"
        )));
    }
    tokio::fs::read(path)
        .await
        .map_err(|error| storage(format!("could not read {label}: {error}")))
}

fn require_exact_cache_root(root: &Path) -> Result<(), BuildOutputValidationError> {
    let mut names = std::fs::read_dir(root)
        .map_err(|error| storage(format!("could not scan build cache root: {error}")))?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|error| storage(format!("could not scan build cache entry: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if names
        != [
            std::ffi::OsString::from("blobs"),
            std::ffi::OsString::from("index.json"),
            std::ffi::OsString::from("ingest"),
            std::ffi::OsString::from("oci-layout"),
        ]
    {
        return Err(integrity(
            "build cache OCI root contains unexpected entries",
        ));
    }
    for (path, directory) in [
        (root.join("blobs"), true),
        (root.join("blobs/sha256"), true),
        (root.join("index.json"), false),
        (root.join("ingest"), true),
        (root.join("oci-layout"), false),
    ] {
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| storage(format!("could not inspect build cache path: {error}")))?;
        if metadata.file_type().is_symlink() || metadata.is_dir() != directory {
            return Err(integrity("build cache OCI path has an invalid type"));
        }
    }
    let algorithms = std::fs::read_dir(root.join("blobs"))
        .map_err(|error| storage(format!("could not scan build cache algorithms: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| storage(format!("could not scan build cache algorithm: {error}")))?;
    if algorithms.len() != 1 || algorithms[0].file_name() != "sha256" {
        return Err(integrity(
            "build cache contains an unsupported digest algorithm",
        ));
    }
    if std::fs::read_dir(root.join("ingest"))
        .map_err(|error| storage(format!("could not scan build cache ingest root: {error}")))?
        .next()
        .is_some()
    {
        return Err(integrity(
            "build cache contains an incomplete ingest operation",
        ));
    }
    Ok(())
}

fn blob_inventory(root: &Path) -> Result<BTreeSet<String>, BuildOutputValidationError> {
    let mut inventory = BTreeSet::new();
    for entry in std::fs::read_dir(root)
        .map_err(|error| storage(format!("could not scan build cache blobs: {error}")))?
    {
        let entry =
            entry.map_err(|error| storage(format!("could not scan build cache blob: {error}")))?;
        let metadata = entry
            .metadata()
            .map_err(|error| storage(format!("could not inspect build cache blob: {error}")))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| integrity("build cache blob name is not UTF-8"))?;
        validate_digest(&format!("sha256:{name}"), "build cache blob name")?;
        if !metadata.is_file() || entry.file_type().map_err(storage)?.is_symlink() {
            return Err(integrity("build cache blob is not an owned regular file"));
        }
        inventory.insert(name);
    }
    Ok(inventory)
}

fn validate_descriptor(
    descriptor: &Descriptor,
    max_bytes: u64,
) -> Result<(), BuildOutputValidationError> {
    validate_digest(&descriptor.digest, "build cache descriptor digest")?;
    if descriptor.media_type.trim().is_empty()
        || descriptor.media_type.len() > 255
        || descriptor.media_type.contains(['\0', '\r', '\n'])
        || descriptor.size == 0
        || descriptor.size > max_bytes
        || !descriptor.urls.is_empty()
        || descriptor.data.is_some()
        || descriptor.artifact_type.is_some()
    {
        return Err(integrity("build cache descriptor is invalid"));
    }
    validate_annotations(&descriptor.annotations)?;
    if descriptor
        .platform
        .as_ref()
        .is_some_and(|platform| !platform.is_object() || platform.to_string().len() > 4096)
    {
        return Err(integrity("build cache descriptor platform is invalid"));
    }
    Ok(())
}

fn validate_annotations(
    annotations: &BTreeMap<String, String>,
) -> Result<(), BuildOutputValidationError> {
    if annotations.len() > MAX_ANNOTATIONS
        || annotations.iter().any(|(key, value)| {
            key.trim().is_empty()
                || key.len() > 255
                || value.len() > 4096
                || key.contains(['\0', '\r', '\n'])
                || value.contains(['\0', '\r', '\n'])
        })
    {
        return Err(integrity("build cache OCI annotations are invalid"));
    }
    Ok(())
}

fn validate_digest<'a>(value: &'a str, label: &str) -> Result<&'a str, BuildOutputValidationError> {
    value
        .strip_prefix("sha256:")
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| integrity(format!("{label} is not lowercase SHA-256")))
}

fn is_cache_layer_media_type(value: &str) -> bool {
    matches!(
        value,
        "application/vnd.oci.image.layer.v1.tar"
            | "application/vnd.oci.image.layer.v1.tar+gzip"
            | "application/vnd.oci.image.layer.v1.tar+zstd"
            | "application/vnd.oci.image.layer.nondistributable.v1.tar"
            | "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip"
            | "application/vnd.oci.image.layer.nondistributable.v1.tar+zstd"
            | "application/vnd.docker.image.rootfs.diff.tar"
            | "application/vnd.docker.image.rootfs.diff.tar.gzip"
            | "application/vnd.docker.image.rootfs.foreign.diff.tar.gzip"
    )
}

fn invalid(message: impl Into<String>) -> BuildOutputValidationError {
    BuildOutputValidationError::Invalid(message.into())
}

fn integrity(message: impl Into<String>) -> BuildOutputValidationError {
    BuildOutputValidationError::Integrity(message.into())
}

fn storage(message: impl std::fmt::Display) -> BuildOutputValidationError {
    BuildOutputValidationError::Storage(message.to_string())
}
