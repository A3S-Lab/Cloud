use super::*;
use crate::modules::artifacts::domain::{
    BuildArtifact, IBuildArtifactPublisher, IBuildOutputValidator, INodeArtifactStore,
    NodeArtifactDescriptor, OciPublicationRequest,
};
use crate::modules::artifacts::infrastructure::{
    LocalNodeArtifactStore, RuntimeBuildOutputValidator,
};
use crate::modules::sources::domain::BuildRecipe;
use a3s_cloud_contracts::{
    artifact_uri, RegistryCredentialMaterial, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::ArtifactRef;
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, WWW_AUTHENTICATE};
use axum::http::{Response, StatusCode};
use axum::routing::any;
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tar::Builder;
use tempfile::TempDir;
use tokio::task::JoinHandle;

const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const OCI_LAYER: &str = "application/vnd.oci.image.layer.v1.tar";
const REPOSITORY: &str = "a3s/tests/build";

#[tokio::test]
async fn publishes_single_manifest_once_and_finds_the_complete_graph(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = PublicationFixture::single().await?;
    let registry = TestRegistry::start(
        AuthMode::Anonymous,
        RegistryFault::None,
        fixture.output.descriptor.digest(),
    )
    .await?;
    let publisher = fixture.publisher(&registry, None)?;
    let request = fixture.request(&registry)?;

    let published = publisher.publish(&request).await?;
    assert_eq!(
        published,
        PublishedOciArtifact::from_target(&request.target)
    );
    assert_eq!(registry.state.blob_puts.load(Ordering::SeqCst), 2);
    assert_eq!(
        registry
            .state
            .manifest_puts
            .lock()
            .expect("manifest puts")
            .as_slice(),
        [request.target.descriptor.digest()]
    );

    assert_eq!(publisher.find(&request).await?, Some(published.clone()));
    assert_eq!(publisher.publish(&request).await?, published);
    assert_eq!(registry.state.blob_puts.load(Ordering::SeqCst), 2);
    assert_eq!(
        registry
            .state
            .manifest_puts
            .lock()
            .expect("manifest puts")
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn publishes_multi_platform_children_before_the_root_index(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = PublicationFixture::multi_platform().await?;
    let root_digest = fixture.output.descriptor.digest().to_owned();
    let registry =
        TestRegistry::start(AuthMode::Anonymous, RegistryFault::None, &root_digest).await?;
    let publisher = fixture.publisher(&registry, None)?;
    let request = fixture.request(&registry)?;

    publisher.publish(&request).await?;
    let manifests = registry
        .state
        .manifest_puts
        .lock()
        .expect("manifest puts")
        .clone();
    assert_eq!(registry.state.blob_puts.load(Ordering::SeqCst), 4);
    assert_eq!(manifests.len(), 3);
    assert_eq!(manifests.last(), Some(&root_digest));
    assert!(manifests[..2].iter().all(|digest| digest != &root_digest));
    assert!(publisher.find(&request).await?.is_some());
    Ok(())
}

#[tokio::test]
async fn publishes_with_basic_and_bearer_registry_authentication(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = PublicationFixture::single().await?;
    for auth in [AuthMode::Basic, AuthMode::Bearer] {
        let credential = CredentialEnv::new("registry-user", "registry-password")?;
        let registry = TestRegistry::start(
            auth,
            RegistryFault::None,
            fixture.output.descriptor.digest(),
        )
        .await?;
        let publisher = fixture.publisher(&registry, Some(credential.name()))?;
        publisher.publish(&fixture.request(&registry)?).await?;
        assert!(registry.state.anonymous_requests.load(Ordering::SeqCst) > 0);
        assert!(registry.state.authenticated_requests.load(Ordering::SeqCst) > 0);
        if auth == AuthMode::Bearer {
            assert!(registry.state.token_requests.load(Ordering::SeqCst) > 0);
        }
    }
    Ok(())
}

#[tokio::test]
async fn rejects_registry_authorization_and_token_failures_without_leaking_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = PublicationFixture::single().await?;
    let wrong = CredentialEnv::new("wrong-user", "do-not-leak-this-password")?;
    let unauthorized = TestRegistry::start(
        AuthMode::Basic,
        RegistryFault::None,
        fixture.output.descriptor.digest(),
    )
    .await?;
    let error = fixture
        .publisher(&unauthorized, Some(wrong.name()))?
        .publish(&fixture.request(&unauthorized)?)
        .await
        .expect_err("wrong Basic credential must be rejected");
    assert!(matches!(error, BuildArtifactPublicationError::Unauthorized));
    assert!(!format!("{error:?}").contains("do-not-leak-this-password"));

    let forbidden = TestRegistry::start(
        AuthMode::Anonymous,
        RegistryFault::Forbidden,
        fixture.output.descriptor.digest(),
    )
    .await?;
    assert!(matches!(
        fixture
            .publisher(&forbidden, None)?
            .publish(&fixture.request(&forbidden)?)
            .await,
        Err(BuildArtifactPublicationError::Unauthorized)
    ));

    let credential = CredentialEnv::new("registry-user", "registry-password")?;
    let token_failure = TestRegistry::start(
        AuthMode::Bearer,
        RegistryFault::TokenFailure,
        fixture.output.descriptor.digest(),
    )
    .await?;
    assert!(matches!(
        fixture
            .publisher(&token_failure, Some(credential.name()))?
            .publish(&fixture.request(&token_failure)?)
            .await,
        Err(BuildArtifactPublicationError::Unauthorized)
    ));

    let oversized_token = TestRegistry::start(
        AuthMode::Bearer,
        RegistryFault::OversizedTokenResponse,
        fixture.output.descriptor.digest(),
    )
    .await?;
    assert!(matches!(
        fixture
            .publisher(&oversized_token, Some(credential.name()))?
            .publish(&fixture.request(&oversized_token)?)
            .await,
        Err(BuildArtifactPublicationError::Protocol(_))
    ));
    Ok(())
}

#[tokio::test]
async fn rejects_upload_locations_that_escape_the_bound_registry_repository(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = PublicationFixture::single().await?;
    for fault in [
        RegistryFault::CrossOriginLocation,
        RegistryFault::SiblingRepositoryLocation,
        RegistryFault::EncodedTraversalLocation,
    ] {
        let registry = TestRegistry::start(
            AuthMode::Anonymous,
            fault,
            fixture.output.descriptor.digest(),
        )
        .await?;
        let error = fixture
            .publisher(&registry, None)?
            .publish(&fixture.request(&registry)?)
            .await
            .expect_err("escaping upload Location must fail closed");
        assert!(matches!(error, BuildArtifactPublicationError::Protocol(_)));
        assert_eq!(registry.state.blob_puts.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[tokio::test]
async fn rejects_registry_digest_media_type_and_size_mismatches(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = PublicationFixture::single().await?;
    for fault in [
        RegistryFault::DigestMismatch,
        RegistryFault::MediaTypeMismatch,
        RegistryFault::SizeMismatch,
    ] {
        let registry = TestRegistry::start(
            AuthMode::Anonymous,
            fault,
            fixture.output.descriptor.digest(),
        )
        .await?;
        let error = fixture
            .publisher(&registry, None)?
            .publish(&fixture.request(&registry)?)
            .await
            .expect_err("registry descriptor mismatch must fail closed");
        assert!(matches!(error, BuildArtifactPublicationError::Protocol(_)));
    }
    Ok(())
}

#[tokio::test]
async fn replay_adopts_a_root_manifest_stored_before_a_transient_response_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = PublicationFixture::single().await?;
    let registry = TestRegistry::start(
        AuthMode::Anonymous,
        RegistryFault::FailRootPutOnce,
        fixture.output.descriptor.digest(),
    )
    .await?;
    let publisher = fixture.publisher(&registry, None)?;
    let request = fixture.request(&registry)?;

    assert!(matches!(
        publisher.publish(&request).await,
        Err(BuildArtifactPublicationError::Unavailable(_))
    ));
    assert_eq!(
        registry
            .state
            .manifest_puts
            .lock()
            .expect("manifest puts")
            .len(),
        1
    );
    assert_eq!(
        publisher.publish(&request).await?,
        PublishedOciArtifact::from_target(&request.target)
    );
    assert_eq!(
        registry
            .state
            .manifest_puts
            .lock()
            .expect("manifest puts")
            .len(),
        1
    );
    Ok(())
}

struct PublicationFixture {
    _root: TempDir,
    validator: Arc<RuntimeBuildOutputValidator>,
    output: crate::modules::artifacts::domain::ValidatedOciBuildOutput,
}

impl PublicationFixture {
    async fn single() -> Result<Self, Box<dyn std::error::Error>> {
        Self::create(&[("linux", "amd64")], false).await
    }

    async fn multi_platform() -> Result<Self, Box<dyn std::error::Error>> {
        Self::create(&[("linux", "amd64"), ("linux", "arm64")], true).await
    }

    async fn create(
        platforms: &[(&str, &str)],
        indexed: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let export = root.path().join("export");
        create_export(&export, platforms, indexed)?;
        let archive = root.path().join("output.tar");
        archive_directory(&export, &archive)?;
        let store = Arc::new(LocalNodeArtifactStore::new(
            root.path().join("store"),
            64 * 1024 * 1024,
        )?);
        let artifact = admit(&store, &archive).await?;
        let validator = Arc::new(RuntimeBuildOutputValidator::new(
            store,
            root.path().join("validation"),
            64 * 1024 * 1024,
            1_024,
            64 * 1024 * 1024,
            64,
            64 * 1024 * 1024,
        )?);
        let recipe = BuildRecipe::dockerfile(
            BuildRecipe::SCHEMA,
            BuildRecipe::DOCKERFILE_KIND,
            ".",
            "Dockerfile",
            None,
            platforms
                .iter()
                .map(|(os, architecture)| format!("{os}/{architecture}"))
                .collect(),
        )?;
        let output = validator.validate(&artifact, &recipe, None).await?.output;
        Ok(Self {
            _root: root,
            validator,
            output,
        })
    }

    fn request(
        &self,
        registry: &TestRegistry,
    ) -> Result<OciPublicationRequest, Box<dyn std::error::Error>> {
        let target = OciPublicationTarget::new(
            registry.authority.clone(),
            REPOSITORY,
            self.output.descriptor.clone(),
        )?;
        Ok(OciPublicationRequest::new(target, self.output.clone())?)
    }

    fn publisher(
        &self,
        registry: &TestRegistry,
        credential_env: Option<&str>,
    ) -> Result<OciRegistryArtifactPublisher, String> {
        OciRegistryArtifactPublisher::new(
            Arc::clone(&self.validator),
            Duration::from_secs(2),
            [registry.authority.clone()],
            OciRegistryArtifactPublisherOptions {
                registry: registry.authority.clone(),
                repository_prefix: "a3s/tests".into(),
                credential_env: credential_env.unwrap_or_default().into(),
                allow_anonymous: credential_env.is_none(),
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    Anonymous,
    Basic,
    Bearer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryFault {
    None,
    Forbidden,
    TokenFailure,
    OversizedTokenResponse,
    CrossOriginLocation,
    SiblingRepositoryLocation,
    EncodedTraversalLocation,
    DigestMismatch,
    MediaTypeMismatch,
    SizeMismatch,
    FailRootPutOnce,
}

struct StoredObject {
    content: Vec<u8>,
    media_type: Option<String>,
}

struct RegistryState {
    authority: String,
    root_digest: String,
    auth: AuthMode,
    fault: RegistryFault,
    expected_basic: String,
    objects: Mutex<BTreeMap<String, StoredObject>>,
    manifest_puts: Mutex<Vec<String>>,
    blob_puts: AtomicUsize,
    anonymous_requests: AtomicUsize,
    authenticated_requests: AtomicUsize,
    token_requests: AtomicUsize,
    fail_root_put: AtomicBool,
}

struct TestRegistry {
    authority: String,
    state: Arc<RegistryState>,
    task: JoinHandle<()>,
}

impl TestRegistry {
    async fn start(
        auth: AuthMode,
        fault: RegistryFault,
        root_digest: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let authority = listener.local_addr()?.to_string();
        let expected_basic = format!(
            "Basic {}",
            STANDARD.encode("registry-user:registry-password")
        );
        let state = Arc::new(RegistryState {
            authority: authority.clone(),
            root_digest: root_digest.into(),
            auth,
            fault,
            expected_basic,
            objects: Mutex::new(BTreeMap::new()),
            manifest_puts: Mutex::new(Vec::new()),
            blob_puts: AtomicUsize::new(0),
            anonymous_requests: AtomicUsize::new(0),
            authenticated_requests: AtomicUsize::new(0),
            token_requests: AtomicUsize::new(0),
            fail_root_put: AtomicBool::new(fault == RegistryFault::FailRootPutOnce),
        });
        let router = Router::new()
            .fallback(any(registry_request))
            .with_state(Arc::clone(&state));
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test registry server");
        });
        Ok(Self {
            authority,
            state,
            task,
        })
    }
}

impl Drop for TestRegistry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn registry_request(
    State(state): State<Arc<RegistryState>>,
    request: Request,
) -> Response<Body> {
    if request.uri().path() == "/token" {
        return token_response(&state, &request);
    }
    if state.fault == RegistryFault::Forbidden {
        return response(StatusCode::FORBIDDEN);
    }
    if let Some(challenge) = authorize(&state, request.headers()) {
        return challenge;
    }
    let prefix = format!("/v2/{REPOSITORY}/");
    let Some(path) = request
        .uri()
        .path()
        .strip_prefix(&prefix)
        .map(str::to_owned)
    else {
        return response(StatusCode::NOT_FOUND);
    };
    let method = request.method().clone();
    let query = request.uri().query().map(str::to_owned);
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = match to_bytes(request.into_body(), 64 * 1024 * 1024).await {
        Ok(body) => body.to_vec(),
        Err(_) => return response(StatusCode::BAD_REQUEST),
    };

    if method == axum::http::Method::HEAD {
        if let Some(digest) = path.strip_prefix("manifests/") {
            return head_object(&state, digest, true);
        }
        if let Some(digest) = path.strip_prefix("blobs/") {
            return head_object(&state, digest, false);
        }
    }
    if method == axum::http::Method::POST && path == "blobs/uploads/" {
        let escaping_location = match state.fault {
            RegistryFault::CrossOriginLocation => {
                Some("http://example.invalid/v2/a3s/tests/build/blobs/uploads/stolen".to_owned())
            }
            RegistryFault::SiblingRepositoryLocation => {
                Some("/v2/a3s/tests/sibling/blobs/uploads/stolen".to_owned())
            }
            RegistryFault::EncodedTraversalLocation => Some(format!(
                "{prefix}blobs/uploads/%2e%2e%2f%2e%2e%2fsibling/blobs/uploads/stolen"
            )),
            _ => None,
        };
        if let Some(location) = escaping_location {
            return Response::builder()
                .status(StatusCode::ACCEPTED)
                .header(LOCATION, location)
                .body(Body::empty())
                .expect("malicious upload response");
        }
        return Response::builder()
            .status(StatusCode::ACCEPTED)
            .header(LOCATION, format!("{prefix}blobs/uploads/fixture"))
            .body(Body::empty())
            .expect("upload response");
    }
    if method == axum::http::Method::PUT && path == "blobs/uploads/fixture" {
        let digest = query
            .as_deref()
            .and_then(|query| {
                url::form_urlencoded::parse(query.as_bytes())
                    .find(|(name, _)| name == "digest")
                    .map(|(_, value)| value.into_owned())
            })
            .unwrap_or_default();
        if digest != sha256(&body) {
            return response(StatusCode::BAD_REQUEST);
        }
        state.objects.lock().expect("registry objects").insert(
            digest.clone(),
            StoredObject {
                content: body,
                media_type: None,
            },
        );
        state.blob_puts.fetch_add(1, Ordering::SeqCst);
        return Response::builder()
            .status(StatusCode::CREATED)
            .header(DOCKER_CONTENT_DIGEST, digest)
            .body(Body::empty())
            .expect("blob completion response");
    }
    if method == axum::http::Method::PUT {
        if let Some(digest) = path.strip_prefix("manifests/") {
            if digest != sha256(&body) {
                return response(StatusCode::BAD_REQUEST);
            }
            state.objects.lock().expect("registry objects").insert(
                digest.into(),
                StoredObject {
                    content: body,
                    media_type: content_type,
                },
            );
            state
                .manifest_puts
                .lock()
                .expect("manifest puts")
                .push(digest.into());
            if digest == state.root_digest && state.fail_root_put.swap(false, Ordering::SeqCst) {
                return response(StatusCode::SERVICE_UNAVAILABLE);
            }
            let response_digest =
                if digest == state.root_digest && state.fault == RegistryFault::DigestMismatch {
                    format!("sha256:{}", "f".repeat(64))
                } else {
                    digest.into()
                };
            return Response::builder()
                .status(StatusCode::CREATED)
                .header(DOCKER_CONTENT_DIGEST, response_digest)
                .body(Body::empty())
                .expect("manifest completion response");
        }
    }
    response(StatusCode::NOT_FOUND)
}

fn authorize(state: &RegistryState, headers: &axum::http::HeaderMap) -> Option<Response<Body>> {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let authorized = match state.auth {
        AuthMode::Anonymous => true,
        AuthMode::Basic => authorization == Some(state.expected_basic.as_str()),
        AuthMode::Bearer => authorization == Some("Bearer issued-test-token"),
    };
    if authorized {
        state.authenticated_requests.fetch_add(1, Ordering::SeqCst);
        return None;
    }
    state.anonymous_requests.fetch_add(1, Ordering::SeqCst);
    let challenge = match state.auth {
        AuthMode::Anonymous => return None,
        AuthMode::Basic => "Basic realm=\"A3S Cloud test\"".to_owned(),
        AuthMode::Bearer => format!(
            "Bearer realm=\"http://{}/token\",service=\"a3s-test\"",
            state.authority
        ),
    };
    Some(
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(WWW_AUTHENTICATE, challenge)
            .body(Body::empty())
            .expect("authorization challenge"),
    )
}

fn token_response(state: &RegistryState, request: &Request) -> Response<Body> {
    state.token_requests.fetch_add(1, Ordering::SeqCst);
    if state.fault == RegistryFault::TokenFailure {
        return response(StatusCode::SERVICE_UNAVAILABLE);
    }
    if request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        != Some(state.expected_basic.as_str())
    {
        return response(StatusCode::UNAUTHORIZED);
    }
    if state.fault == RegistryFault::OversizedTokenResponse {
        let chunks = futures_util::stream::iter([
            Ok::<_, std::convert::Infallible>(Bytes::from(vec![b'a'; 1024 * 1024])),
            Ok(Bytes::from_static(b"x")),
        ]);
        return Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from_stream(chunks))
            .expect("oversized token response");
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"token":"issued-test-token"}"#))
        .expect("token response")
}

fn head_object(state: &RegistryState, digest: &str, manifest: bool) -> Response<Body> {
    let objects = state.objects.lock().expect("registry objects");
    let Some(object) = objects.get(digest) else {
        return response(StatusCode::NOT_FOUND);
    };
    let size = if digest == state.root_digest && state.fault == RegistryFault::SizeMismatch {
        object.content.len().saturating_add(1)
    } else {
        object.content.len()
    };
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(DOCKER_CONTENT_DIGEST, digest)
        .header(CONTENT_LENGTH, size);
    if manifest {
        let media_type =
            if digest == state.root_digest && state.fault == RegistryFault::MediaTypeMismatch {
                OCI_INDEX
            } else {
                object.media_type.as_deref().unwrap_or(OCI_MANIFEST)
            };
        builder = builder.header(CONTENT_TYPE, media_type);
    }
    builder.body(Body::empty()).expect("object HEAD response")
}

fn response(status: StatusCode) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .expect("test registry response")
}

struct CredentialEnv {
    name: String,
}

impl CredentialEnv {
    fn new(username: &str, password: &str) -> Result<Self, serde_json::Error> {
        static NEXT_ENV: AtomicUsize = AtomicUsize::new(1);
        let name = format!(
            "A3S_TEST_REGISTRY_CREDENTIAL_{}",
            NEXT_ENV.fetch_add(1, Ordering::SeqCst)
        );
        let value = serde_json::to_string(&json!({
            "schema": RegistryCredentialMaterial::SCHEMA,
            "username": username,
            "password": password,
        }))?;
        std::env::set_var(&name, value);
        Ok(Self { name })
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for CredentialEnv {
    fn drop(&mut self) {
        std::env::remove_var(&self.name);
    }
}

async fn admit(
    store: &Arc<LocalNodeArtifactStore>,
    archive: &Path,
) -> Result<BuildArtifact, Box<dyn std::error::Error>> {
    let bytes = tokio::fs::read(archive).await?;
    let digest = sha256(&bytes);
    let reference = ArtifactRef {
        uri: artifact_uri(&digest)?,
        digest: digest.clone(),
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    };
    let descriptor = NodeArtifactDescriptor::new(reference.clone(), bytes.len() as u64)?;
    let file = tokio::fs::File::open(archive).await?;
    store.put(&descriptor, Box::pin(file)).await?;
    Ok(BuildArtifact::new(
        reference.uri,
        digest,
        reference.media_type,
        bytes.len() as u64,
    )?)
}

fn create_export(
    export: &Path,
    platforms: &[(&str, &str)],
    indexed: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let layout = export.join("oci");
    let blobs = layout.join("blobs/sha256");
    std::fs::create_dir_all(&blobs)?;
    std::fs::write(
        layout.join("oci-layout"),
        br#"{"imageLayoutVersion":"1.0.0"}"#,
    )?;
    let mut manifests = Vec::new();
    for (os, architecture) in platforms {
        let layer_content = format!("fixture layer {os}/{architecture}\n");
        let layer = write_blob(&blobs, OCI_LAYER, layer_content.as_bytes())?;
        let layer_digest = layer["digest"].as_str().ok_or("layer digest")?;
        let config = write_json_blob(
            &blobs,
            OCI_CONFIG,
            &json!({
                "architecture": architecture,
                "os": os,
                "config": {},
                "rootfs": {"type": "layers", "diff_ids": [layer_digest]},
            }),
        )?;
        let mut manifest = write_json_blob(
            &blobs,
            OCI_MANIFEST,
            &json!({
                "schemaVersion": 2,
                "mediaType": OCI_MANIFEST,
                "config": config,
                "layers": [layer],
            }),
        )?;
        manifest["platform"] = json!({"architecture": architecture, "os": os});
        manifests.push(manifest);
    }
    let root = if indexed {
        write_json_blob(
            &blobs,
            OCI_INDEX,
            &json!({
                "schemaVersion": 2,
                "mediaType": OCI_INDEX,
                "manifests": manifests,
            }),
        )?
    } else {
        manifests.into_iter().next().ok_or("missing manifest")?
    };
    std::fs::write(
        layout.join("index.json"),
        serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": OCI_INDEX,
            "manifests": [root.clone()],
        }))?,
    )?;
    std::fs::write(
        export.join("buildkit-metadata.json"),
        serde_json::to_vec(&json!({
            "containerimage.digest": root["digest"],
            "containerimage.descriptor": root,
        }))?,
    )?;
    Ok(())
}

fn archive_directory(export: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = Builder::new(File::create(destination)?);
    for directory in ["oci", "oci/blobs", "oci/blobs/sha256"] {
        builder.append_dir(directory, export.join(directory))?;
    }
    builder.append_path_with_name(
        export.join("buildkit-metadata.json"),
        "buildkit-metadata.json",
    )?;
    builder.append_path_with_name(export.join("oci/oci-layout"), "oci/oci-layout")?;
    builder.append_path_with_name(export.join("oci/index.json"), "oci/index.json")?;
    let mut blobs =
        std::fs::read_dir(export.join("oci/blobs/sha256"))?.collect::<Result<Vec<_>, _>>()?;
    blobs.sort_by_key(|entry| entry.file_name());
    for blob in blobs {
        builder.append_path_with_name(
            blob.path(),
            Path::new("oci/blobs/sha256").join(blob.file_name()),
        )?;
    }
    builder.finish()?;
    Ok(())
}

fn write_json_blob(
    blobs: &Path,
    media_type: &str,
    value: &Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    write_blob(blobs, media_type, &serde_json::to_vec(value)?)
}

fn write_blob(
    blobs: &Path,
    media_type: &str,
    content: &[u8],
) -> Result<Value, Box<dyn std::error::Error>> {
    let digest = sha256(content);
    std::fs::write(
        blobs.join(digest.strip_prefix("sha256:").ok_or("SHA-256 digest")?),
        content,
    )?;
    Ok(json!({
        "mediaType": media_type,
        "digest": digest,
        "size": content.len(),
    }))
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
