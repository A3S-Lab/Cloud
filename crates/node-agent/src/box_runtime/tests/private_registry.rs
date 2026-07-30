use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, Response, StatusCode};
use axum::Router;
use base64::Engine as _;
use serde_json::json;
use sha2::Digest as _;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

const REPOSITORY: &str = "a3s/cloud-private-runtime";
pub(super) const OCI_IMAGE_INDEX: &str = "application/vnd.oci.image.index.v1+json";

#[derive(Clone)]
struct RegistryContent {
    bytes: Vec<u8>,
    digest: String,
    media_type: String,
}

#[derive(Clone)]
struct RecordedRequest {
    authorized: bool,
    path: String,
}

#[derive(Clone)]
struct RegistryState {
    expected_authorization: String,
    manifests: Arc<HashMap<String, RegistryContent>>,
    blobs: Arc<HashMap<String, RegistryContent>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

pub(super) struct PrivateRegistry {
    authority: String,
    pub(super) index_digest: String,
    manifest_digest: String,
    config_digest: String,
    layer_digests: Vec<String>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    task: tokio::task::JoinHandle<()>,
}

impl PrivateRegistry {
    pub(super) async fn start(
        source_layout: &Path,
        username: &str,
        password: &str,
    ) -> io::Result<Self> {
        let source_index = read_json(&source_layout.join("index.json"), "source OCI index")?;
        let selected = source_index
            .get("manifests")
            .and_then(serde_json::Value::as_array)
            .and_then(|manifests| manifests.first())
            .ok_or_else(|| io::Error::other("cached source OCI index has no manifest"))?;
        let manifest_digest = descriptor_digest(selected, "selected manifest")?;
        let manifest_media_type = descriptor_media_type(selected, "selected manifest")?;
        let manifest_bytes = read_blob(source_layout, &manifest_digest)?;
        let manifest = serde_json::from_slice::<serde_json::Value>(&manifest_bytes)
            .map_err(io::Error::other)?;

        let config = manifest
            .get("config")
            .ok_or_else(|| io::Error::other("cached image manifest has no config"))?;
        let config_digest = descriptor_digest(config, "image config")?;
        let config_media_type = descriptor_media_type(config, "image config")?;
        let config_bytes = read_blob(source_layout, &config_digest)?;
        let layers = manifest
            .get("layers")
            .and_then(serde_json::Value::as_array)
            .filter(|layers| !layers.is_empty())
            .ok_or_else(|| io::Error::other("cached image manifest has no layers"))?;
        let mut blobs = HashMap::from([(
            config_digest.clone(),
            RegistryContent {
                bytes: config_bytes,
                digest: config_digest.clone(),
                media_type: config_media_type,
            },
        )]);
        let mut layer_digests = Vec::with_capacity(layers.len());
        for layer in layers {
            let digest = descriptor_digest(layer, "image layer")?;
            let media_type = descriptor_media_type(layer, "image layer")?;
            let bytes = read_blob(source_layout, &digest)?;
            blobs.insert(
                digest.clone(),
                RegistryContent {
                    bytes,
                    digest: digest.clone(),
                    media_type,
                },
            );
            layer_digests.push(digest);
        }

        let index_bytes = serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": OCI_IMAGE_INDEX,
            "manifests": [{
                "mediaType": manifest_media_type,
                "digest": manifest_digest,
                "size": manifest_bytes.len(),
                "platform": {
                    "architecture": oci_architecture(),
                    "os": "linux"
                }
            }]
        }))
        .map_err(io::Error::other)?;
        let index_digest = digest(&index_bytes);
        let manifests = HashMap::from([
            (
                index_digest.clone(),
                RegistryContent {
                    bytes: index_bytes,
                    digest: index_digest.clone(),
                    media_type: OCI_IMAGE_INDEX.into(),
                },
            ),
            (
                manifest_digest.clone(),
                RegistryContent {
                    bytes: manifest_bytes,
                    digest: manifest_digest.clone(),
                    media_type: manifest_media_type,
                },
            ),
        ]);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = RegistryState {
            expected_authorization: format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
            ),
            manifests: Arc::new(manifests),
            blobs: Arc::new(blobs),
            requests: Arc::clone(&requests),
        };
        let app = Router::new().fallback(registry_handler).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let authority = listener.local_addr()?.to_string();
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                tracing::error!(%error, "Cloud private-registry fixture failed");
            }
        });

        Ok(Self {
            authority,
            index_digest,
            manifest_digest,
            config_digest,
            layer_digests,
            requests,
            task,
        })
    }

    pub(super) fn image_reference(&self) -> String {
        format!("{}/{REPOSITORY}@{}", self.authority, self.index_digest)
    }

    pub(super) fn protected_manifest_url(&self) -> String {
        format!(
            "http://{}/v2/{REPOSITORY}/manifests/{}",
            self.authority, self.index_digest
        )
    }

    pub(super) fn request_count(&self) -> io::Result<usize> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| io::Error::other("private registry request journal is poisoned"))?
            .len())
    }

    pub(super) fn assert_authenticated_pull(&self) -> io::Result<()> {
        let requests = self
            .requests
            .lock()
            .map_err(|_| io::Error::other("private registry request journal is poisoned"))?;
        let protected = requests
            .iter()
            .filter(|request| request.path != "/v2/")
            .collect::<Vec<_>>();
        if !protected.iter().any(|request| !request.authorized) {
            return Err(io::Error::other(
                "private registry did not reject an anonymous protected request",
            ));
        }
        let mut required = vec![
            format!("/v2/{REPOSITORY}/manifests/{}", self.index_digest),
            format!("/v2/{REPOSITORY}/manifests/{}", self.manifest_digest),
            format!("/v2/{REPOSITORY}/blobs/{}", self.config_digest),
        ];
        required.extend(
            self.layer_digests
                .iter()
                .map(|digest| format!("/v2/{REPOSITORY}/blobs/{digest}")),
        );
        if let Some(path) = required.iter().find(|path| {
            !protected
                .iter()
                .any(|request| request.authorized && request.path == path.as_str())
        }) {
            return Err(io::Error::other(format!(
                "authenticated private-registry pull omitted {path}"
            )));
        }
        Ok(())
    }
}

impl Drop for PrivateRegistry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn registry_handler(
    State(state): State<RegistryState>,
    request: Request<Body>,
) -> Response<Body> {
    let path = request.uri().path().to_string();
    let authorized = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(state.expected_authorization.as_str());
    if let Ok(mut requests) = state.requests.lock() {
        requests.push(RecordedRequest {
            authorized,
            path: path.clone(),
        });
    } else {
        return response(StatusCode::INTERNAL_SERVER_ERROR, None, None, Vec::new());
    }

    if request.method() != Method::GET {
        return response(StatusCode::METHOD_NOT_ALLOWED, None, None, Vec::new());
    }
    if path == "/v2/" {
        return response(StatusCode::OK, None, None, Vec::new());
    }
    if !authorized {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("www-authenticate", "Basic realm=\"A3S Cloud Box Gate\"")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"errors":[{"code":"UNAUTHORIZED","message":"authentication required","detail":{}}]}"#,
            ))
            .unwrap_or_else(|_| {
                response(StatusCode::INTERNAL_SERVER_ERROR, None, None, Vec::new())
            });
    }

    let manifest_prefix = format!("/v2/{REPOSITORY}/manifests/");
    if let Some(reference) = path.strip_prefix(&manifest_prefix) {
        return state.manifests.get(reference).map_or_else(
            || response(StatusCode::NOT_FOUND, None, None, Vec::new()),
            |content| {
                response(
                    StatusCode::OK,
                    Some(&content.media_type),
                    Some(&content.digest),
                    content.bytes.clone(),
                )
            },
        );
    }
    let blob_prefix = format!("/v2/{REPOSITORY}/blobs/");
    if let Some(reference) = path.strip_prefix(&blob_prefix) {
        return state.blobs.get(reference).map_or_else(
            || response(StatusCode::NOT_FOUND, None, None, Vec::new()),
            |content| {
                response(
                    StatusCode::OK,
                    Some(&content.media_type),
                    Some(&content.digest),
                    content.bytes.clone(),
                )
            },
        );
    }
    response(StatusCode::NOT_FOUND, None, None, Vec::new())
}

fn response(
    status: StatusCode,
    media_type: Option<&str>,
    digest: Option<&str>,
    bytes: Vec<u8>,
) -> Response<Body> {
    let mut builder = Response::builder().status(status);
    if let Some(media_type) = media_type {
        builder = builder.header("content-type", media_type);
    }
    if let Some(digest) = digest {
        builder = builder.header("docker-content-digest", digest);
    }
    builder
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn read_json(path: &Path, label: &str) -> io::Result<serde_json::Value> {
    let bytes = std::fs::read(path)
        .map_err(|error| io::Error::other(format!("could not read {label}: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::other(format!("could not decode {label}: {error}")))
}

fn read_blob(layout: &Path, digest: &str) -> io::Result<Vec<u8>> {
    let digest = digest
        .strip_prefix("sha256:")
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| io::Error::other("source OCI descriptor has an invalid digest"))?;
    std::fs::read(layout.join("blobs/sha256").join(digest))
}

fn descriptor_digest(value: &serde_json::Value, label: &str) -> io::Result<String> {
    value
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other(format!("{label} has no digest")))
}

fn descriptor_media_type(value: &serde_json::Value, label: &str) -> io::Result<String> {
    value
        .get("mediaType")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other(format!("{label} has no media type")))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", sha2::Sha256::digest(bytes))
}

fn oci_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        architecture => architecture,
    }
}
