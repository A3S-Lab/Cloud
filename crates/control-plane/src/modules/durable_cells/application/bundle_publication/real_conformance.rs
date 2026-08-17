use super::*;
use crate::infrastructure::DisposableS3TestContext;
use crate::modules::data::{
    IObjectNamespace, ObjectNamespaceKey, ObjectNamespaceProviderProfile,
    ObjectNamespaceProviderProfileSpec, ObjectNamespaceRead,
};
use crate::modules::executions::project_execution_task;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, SecretId, StorageNamespaceId,
};
use a3s_cloud_contracts::{
    artifact_uri, CloudSecretReference, NodeArtifactDownloadRequest, NodeArtifactUploadRequest,
    NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use a3s_cloud_node_agent::{
    build_box_runtime_provider, ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation,
    CommandExecutor, DownloadedNodeArtifact, FileCommandJournal, NodeArtifactManager,
    NodeArtifactTransport, NodeControlClientError, NodeSecretTransport, SecretMaterial,
};
use a3s_runtime::contract::{
    NetworkMode, RuntimeActionRequest, RuntimeApplyRequest, RuntimeInspection, RuntimeUnitClass,
    RuntimeUnitState, SecretReference, SecretTarget,
};
use a3s_runtime::RuntimeClient;
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const GATE_ENV: &str = "A3S_CLOUD_TEST_CELL_BUNDLE_PUBLICATION";
const IMAGE_ENV: &str = "A3S_CLOUD_TEST_CELL_PROVIDER_IMAGE";
const ACCESS_KEY_ENV: &str = "A3S_CLOUD_TEST_S3_ACCESS_KEY_ID";
const SECRET_KEY_ENV: &str = "A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY";
const SESSION_TOKEN_ENV: &str = "A3S_CLOUD_TEST_S3_SESSION_TOKEN";
const SCRIPT_NAME: &str = "a3s-cloud-cell-publication-gate";
const PROVIDER_REVISION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/cell-conformance/celld-revision"
));
const WRANGLER_JSON: &[u8] = br#"{
  "name": "a3s-cloud-cell-publication-gate",
  "main": "worker.mjs",
  "no_bundle": true,
  "compatibility_date": "2026-08-16",
  "durable_objects": {
    "bindings": [
      { "name": "COUNTER", "class_name": "Counter" }
    ]
  },
  "migrations": [
    { "tag": "v1", "new_sqlite_classes": ["Counter"] }
  ]
}
"#;
const WORKER_MODULE: &[u8] = br#"export class Counter {
  async fetch() {
    return new Response("a3s-cloud-cell-publication-gate");
  }
}

export default {
  async fetch() {
    return new Response("a3s-cloud-cell-publication-gate");
  },
};
"#;

type GateResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Retained CELL0.5-C3 gate. It runs the exact publisher profile as an
/// ordinary node-bound Execution Task, resolves credentials through the sole
/// Cloud Secret adapter, materializes the typed bundle through the sole
/// Artifact adapter, and observes/cleans the result through the production S0
/// object-namespace client.
#[tokio::test]
#[ignore = "requires the dedicated Linux Box runner and disposable S3-compatible namespace"]
async fn real_celld_bundle_publication_uses_execution_box_secrets_artifacts_and_s0(
) -> GateResult<()> {
    if std::env::var(GATE_ENV).as_deref() != Ok("1") {
        return Err(invalid("dedicated gate did not enable real bundle publication").into());
    }
    let publisher = DurableCellPublisherProfile::pinned_celld_v0_2_1()?;
    publisher.validate()?;
    let expected_image = publisher
        .image_uri()
        .strip_prefix("oci://")
        .ok_or_else(|| invalid("publisher image URI is not OCI"))?;
    if std::env::var(IMAGE_ENV).as_deref() != Ok(expected_image) {
        return Err(invalid("real gate image differs from the publisher profile").into());
    }
    let revision = PROVIDER_REVISION.trim();
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("checked-in celld revision is invalid").into());
    }

    let namespace_id = StorageNamespaceId::new();
    let storage = DisposableS3TestContext::from_environment_with_id(
        "cell-bundle-publication",
        namespace_id.as_uuid(),
    )?;
    if !storage.uses_secure_transport() || storage.virtual_hosted_style() {
        return Err(invalid("celld publication requires HTTPS path-style S0 storage").into());
    }
    let storage_profile =
        ObjectNamespaceProviderProfile::from_spec(ObjectNamespaceProviderProfileSpec {
            endpoint: storage.endpoint().into(),
            region: storage.region().into(),
            bucket: storage.bucket().into(),
            prefix: "a3s-cloud-tests/cell-bundle-publication".into(),
            virtual_hosted_style: storage.virtual_hosted_style(),
        })?;
    if storage_profile.namespace_prefix(namespace_id)? != storage.prefix() {
        return Err(invalid("S0 test namespace differs from product prefix semantics").into());
    }

    let publication =
        execute_publication(&storage, &storage_profile, namespace_id, &publisher).await;
    let cleanup = storage.remove_all().await;
    let outcome = publication?;
    let removed = cleanup?;
    if removed != outcome.object_count {
        return Err(invalid("S0 cleanup count changed after publication verification").into());
    }

    println!(
        "A3S_CLOUD_CELL0_5_BUNDLE_PUBLICATION_CERTIFIED provider=celld revision={} image_digest={} publisher_profile_digest={} s0_profile_digest={} bundle_digest={} version={} task=succeeded replay=exact objects={} cleanup=verified secrets=ephemeral",
        revision,
        publisher.image_digest(),
        publisher.digest(),
        storage_profile.digest(),
        outcome.bundle_digest,
        outcome.version,
        outcome.object_count,
    );
    Ok(())
}

struct PublicationOutcome {
    bundle_digest: String,
    version: String,
    object_count: usize,
}

async fn execute_publication(
    storage: &DisposableS3TestContext,
    storage_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: StorageNamespaceId,
    publisher: &DurableCellPublisherProfile,
) -> GateResult<PublicationOutcome> {
    let bundle_bytes = directory_archive(&[
        ("worker.mjs", WORKER_MODULE),
        ("wrangler.json", WRANGLER_JSON),
    ])?;
    let bundle_digest = format!("sha256:{:x}", Sha256::digest(&bundle_bytes));
    let bundle = a3s_runtime::contract::ArtifactRef {
        uri: artifact_uri(&bundle_digest).map_err(invalid)?,
        digest: bundle_digest.clone(),
        media_type: DURABLE_CELL_BUNDLE_MEDIA_TYPE.into(),
    };
    let node_id = NodeId::new();
    let subject_id = Uuid::now_v7();
    let (secrets, secret_transport) = publication_secrets(subject_id, publisher)?;
    let artifact_transport = Arc::new(PublicationArtifactTransport {
        artifact: bundle.clone(),
        bytes: bundle_bytes,
        downloads: AtomicUsize::new(0),
    });
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let artifacts = Arc::new(NodeArtifactManager::new(
        node_state.path().join("artifacts"),
        ArtifactConfig {
            max_blob_bytes: 4 * 1024 * 1024,
            max_entries: 100,
            max_file_bytes: 2 * 1024 * 1024,
            max_expanded_bytes: 8 * 1024 * 1024,
        },
        node_id.as_uuid(),
        artifact_transport.clone(),
    )?);
    let home = PathBuf::from(std::env::var("A3S_HOME")?).canonicalize()?;
    let secret_root = home.join("runtime-secrets").canonicalize()?;
    let provider = build_box_runtime_provider(
        &BoxRuntimeConfig {
            home_dir: home,
            secret_root: secret_root.clone(),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
            sev_snp: None,
        },
        runtime_state.path(),
    )?;
    let secret_binding: Arc<dyn NodeSecretTransport> = secret_transport.clone();
    let runtime = provider
        .into_bound_client(secret_binding, artifacts.clone())
        .await?;
    let execution = publication_execution(
        node_id,
        subject_id,
        storage_namespace_id,
        storage_profile,
        publisher,
        bundle,
        secrets,
    )?;
    let spec = project_execution_task(&execution)?;
    if spec.class != RuntimeUnitClass::Task || spec.network.mode != NetworkMode::Outbound {
        return Err(invalid("publication Execution did not project to one outbound Task").into());
    }
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(node_state.path().join("journal"), node_id.as_uuid())?,
        runtime.clone(),
    )
    .with_artifacts(artifacts);
    let apply = command(
        node_id,
        execution.id.as_uuid(),
        1,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("cell-publication-apply-{}", execution.id),
                deadline_at_ms: None,
                spec: spec.clone(),
            }),
            resource_claim: None,
        },
    )?;
    let applied = executor.execute(apply.clone()).await?;
    let observation = applied_observation(&applied)?;
    if observation.state != RuntimeUnitState::Succeeded {
        return Err(invalid(format!(
            "celld publication Task did not succeed: {:?}",
            observation.state
        ))
        .into());
    }
    let mut replay = apply;
    replay.lease_id = Uuid::now_v7();
    if executor.execute(replay).await?.outcome != applied.outcome {
        return Err(invalid("Fleet journal changed publication Task replay").into());
    }
    let removed = executor
        .execute(command(
            node_id,
            execution.id.as_uuid(),
            2,
            NodeCommandPayload::RuntimeRemove {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: format!("cell-publication-remove-{}", execution.id),
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                    deadline_at_ms: None,
                },
            },
        )?)
        .await?;
    expect_removed(&removed, &spec.unit_id)?;
    if !matches!(
        runtime.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid("removed publication Task remained inspectable").into());
    }
    if artifact_transport.downloads.load(Ordering::SeqCst) != 1
        || secret_transport.total_calls()? != secret_transport.material_count()
        || directory_has_entries(&secret_root)?
    {
        return Err(invalid(
            "publication replay or cleanup changed Artifact/Secret materialization",
        )
        .into());
    }

    let namespace: Arc<dyn IObjectNamespace> = Arc::new(storage.client());
    let version = verify_publication(namespace.as_ref()).await?;
    Ok(PublicationOutcome {
        bundle_digest,
        version,
        object_count: 4,
    })
}

fn publication_execution(
    node_id: NodeId,
    subject_id: Uuid,
    storage_namespace_id: StorageNamespaceId,
    storage_profile: &ObjectNamespaceProviderProfile,
    publisher: &DurableCellPublisherProfile,
    bundle: a3s_runtime::contract::ArtifactRef,
    secrets: Vec<SecretReference>,
) -> Result<Execution, String> {
    let execution_id = ExecutionId::new();
    let definition = build_publication_task_definition(
        storage_profile,
        publisher,
        PublicationTaskDefinitionInput {
            node_id,
            storage_namespace_id,
            image_media_type: OCI_IMAGE_INDEX_MEDIA_TYPE.into(),
            authority: ExecutionTaskAuthority {
                kind: PUBLICATION_AUTHORITY_KIND.into(),
                subject_id,
                digest: Sha256Digest::from_bytes(
                    format!(
                        "cell0.5-c3:{}:{}:{}",
                        publisher.digest(),
                        storage_profile.digest(),
                        bundle.digest
                    )
                    .as_bytes(),
                ),
            },
            input: serde_json::json!({
                "schema": PUBLICATION_INPUT_SCHEMA,
                "conformance": "cell0.5-c3",
            }),
            bundle,
            secrets,
        },
    )?;
    Execution::create_bound_task(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        execution_id,
        definition.template,
        node_id,
        definition.task_policy,
        Utc::now() - ChronoDuration::seconds(1),
    )
}

fn publication_secrets(
    subject_id: Uuid,
    publisher: &DurableCellPublisherProfile,
) -> GateResult<(Vec<SecretReference>, Arc<PublicationSecretTransport>)> {
    let mut bindings = Vec::new();
    let mut materials = HashMap::new();
    for (name, variable, environment) in [
        (
            "s0-access-key-id",
            publisher.access_key_environment(),
            ACCESS_KEY_ENV,
        ),
        (
            "s0-secret-access-key",
            publisher.secret_access_key_environment(),
            SECRET_KEY_ENV,
        ),
    ] {
        add_secret(
            subject_id,
            name,
            variable,
            required_environment(environment)?,
            &mut bindings,
            &mut materials,
        )?;
    }
    if let Some(value) = optional_environment(SESSION_TOKEN_ENV)? {
        add_secret(
            subject_id,
            "s0-session-token",
            publisher.session_token_environment(),
            value,
            &mut bindings,
            &mut materials,
        )?;
    }
    Ok((
        bindings,
        Arc::new(PublicationSecretTransport {
            materials,
            calls: Mutex::new(HashMap::new()),
        }),
    ))
}

fn add_secret(
    subject_id: Uuid,
    name: &str,
    variable: &str,
    value: String,
    bindings: &mut Vec<SecretReference>,
    materials: &mut HashMap<String, Vec<u8>>,
) -> Result<(), String> {
    let reference = CloudSecretReference::new(subject_id, SecretId::new().as_uuid(), 1)?;
    materials.insert(reference.to_string(), value.into_bytes());
    bindings.push(SecretReference {
        name: name.into(),
        reference: reference.to_string(),
        target: SecretTarget::Environment {
            variable: variable.into(),
        },
    });
    Ok(())
}

struct PublicationSecretTransport {
    materials: HashMap<String, Vec<u8>>,
    calls: Mutex<HashMap<String, usize>>,
}

impl PublicationSecretTransport {
    fn material_count(&self) -> usize {
        self.materials.len()
    }

    fn total_calls(&self) -> Result<usize, String> {
        self.calls
            .lock()
            .map(|calls| calls.values().sum())
            .map_err(|_| "publication Secret call lock poisoned".into())
    }
}

#[async_trait]
impl NodeSecretTransport for PublicationSecretTransport {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        let reference = reference.to_string();
        let material = self.materials.get(&reference).cloned().ok_or_else(|| {
            NodeControlClientError::Invalid("publication Secret reference is unknown".into())
        })?;
        *self
            .calls
            .lock()
            .map_err(|_| NodeControlClientError::Transport("Secret call lock poisoned".into()))?
            .entry(reference)
            .or_default() += 1;
        SecretMaterial::new(material).map_err(NodeControlClientError::Invalid)
    }
}

struct PublicationArtifactTransport {
    artifact: a3s_runtime::contract::ArtifactRef,
    bytes: Vec<u8>,
    downloads: AtomicUsize,
}

#[async_trait]
impl NodeArtifactTransport for PublicationArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request
            .artifact()
            .map_err(NodeControlClientError::Invalid)?
            != self.artifact
            || self.bytes.len() as u64 > maximum_bytes
        {
            return Err(NodeControlClientError::Invalid(
                "publication Task requested an unexpected bundle Artifact".into(),
            ));
        }
        tokio::fs::write(destination, &self.bytes)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(DownloadedNodeArtifact {
            size_bytes: self.bytes.len() as u64,
        })
    }

    async fn upload(
        &self,
        _request: &NodeArtifactUploadRequest,
        _source: &Path,
    ) -> Result<a3s_cloud_contracts::NodeArtifactUploadReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "publication Task has no output Artifact authority".into(),
        ))
    }
}

async fn verify_publication(namespace: &dyn IObjectNamespace) -> GateResult<String> {
    let pointer = read_required(namespace, "deploy/current.json", 64 * 1024).await?;
    let named_pointer = read_required(
        namespace,
        &format!("deploy/{SCRIPT_NAME}/current.json"),
        64 * 1024,
    )
    .await?;
    if pointer != named_pointer {
        return Err(invalid("celld named and fleet deployment pointers differ").into());
    }
    let pointer: serde_json::Value = serde_json::from_slice(&pointer)?;
    let version = pointer["version"]
        .as_str()
        .filter(|value| {
            value.len() == 16
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| invalid("celld deployment pointer version is invalid"))?;
    let expected_prefix = format!("deploy/{SCRIPT_NAME}/{version}");
    if pointer["script_name"].as_str() != Some(SCRIPT_NAME)
        || pointer["prefix"].as_str() != Some(expected_prefix.as_str())
        || pointer["rollout"]["percent"].as_u64() != Some(100)
    {
        return Err(invalid("celld deployment pointer changed its committed identity").into());
    }
    let manifest = read_required(
        namespace,
        &format!("{expected_prefix}/manifest.json"),
        256 * 1024,
    )
    .await?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest)?;
    if manifest["schema_version"].as_u64() != Some(1)
        || manifest["version"].as_str() != Some(version)
        || manifest["script_name"].as_str() != Some(SCRIPT_NAME)
        || manifest["main_module"].as_str() != Some("index.js")
        || manifest["do_classes"] != serde_json::json!(["Counter"])
        || manifest["sqlite_classes"] != serde_json::json!(["Counter"])
        || manifest["modules"]
            .as_array()
            .is_none_or(|modules| modules.len() != 1)
        || manifest["modules"][0]["name"].as_str() != Some("index.js")
        || manifest["modules"][0]["bytes"].as_u64() != Some(WORKER_MODULE.len() as u64)
    {
        return Err(invalid("celld deployment manifest changed the typed test bundle").into());
    }
    let module = read_required(
        namespace,
        &format!("{expected_prefix}/index.js"),
        2 * 1024 * 1024,
    )
    .await?;
    let module_digest = format!("{:x}", Sha256::digest(&module));
    if module != WORKER_MODULE
        || manifest["modules"][0]["sha256"].as_str() != Some(&module_digest[..16])
    {
        return Err(invalid("celld published module bytes or digest changed").into());
    }
    Ok(version.into())
}

async fn read_required(
    namespace: &dyn IObjectNamespace,
    key: &str,
    maximum_bytes: u64,
) -> GateResult<Vec<u8>> {
    match namespace
        .read(&ObjectNamespaceKey::parse(key)?, maximum_bytes)
        .await?
    {
        ObjectNamespaceRead::Found { body, .. } => Ok(body),
        ObjectNamespaceRead::Missing => {
            Err(invalid(format!("published object {key} is missing")).into())
        }
        ObjectNamespaceRead::Corrupt => {
            Err(invalid(format!("published object {key} is corrupt")).into())
        }
    }
}

fn directory_archive(entries: &[(&str, &[u8])]) -> GateResult<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, path, *bytes)?;
    }
    builder.finish()?;
    Ok(builder.into_inner()?)
}

fn command(
    node_id: NodeId,
    aggregate_id: Uuid,
    sequence: u64,
    payload: NodeCommandPayload,
) -> GateResult<NodeCommandEnvelope> {
    let issued_at = Utc::now() - ChronoDuration::seconds(1);
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id: node_id.as_uuid(),
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(15),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
    .map_err(|error| invalid(error).into())
}

fn applied_observation(
    acknowledgement: &NodeCommandAck,
) -> GateResult<&a3s_runtime::contract::RuntimeObservation> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
        _ => Err(invalid("publication apply returned an unexpected result").into()),
    }
}

fn expect_removed(acknowledgement: &NodeCommandAck, unit_id: &str) -> GateResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeRemoved { removal } if removal.unit_id == unit_id => Ok(()),
        _ => Err(invalid("publication remove returned an unexpected result").into()),
    }
}

fn succeeded_result(acknowledgement: &NodeCommandAck) -> GateResult<&NodeCommandResult> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => Ok(result),
        _ => Err(invalid("publication node command did not succeed").into()),
    }
}

fn required_environment(name: &str) -> Result<String, String> {
    let value = std::env::var(name).map_err(|_| format!("{name} is required"))?;
    if value.is_empty() || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{name} is invalid"));
    }
    Ok(value)
}

fn optional_environment(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) if value.contains(['\0', '\r', '\n']) => Err(format!("{name} is invalid")),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is invalid")),
    }
}

fn directory_has_entries(path: &Path) -> io::Result<bool> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().transpose()?.is_some()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
