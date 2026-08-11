use a3s_box_runtime::{BoxBuildPlan, BuildCachePolicy, BuildNetworkPolicy};
use a3s_cloud_contracts::{
    NodeBoxBuildOutput, NodeBoxBuildRequest, NodeCommandEnvelope, NodeCommandPayload,
    RegistryCredentialMaterial, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_cloud_control_plane::infrastructure::connect_and_migrate;
use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, BuildRun, INodeArtifactStore, LocalNodeArtifactStore, NodeArtifactDescriptor,
    OciBuildOutputValidator, OciRegistryArtifactPublisher, OciRegistryArtifactPublisherOptions,
};
use a3s_cloud_control_plane::modules::fleet::domain::entities::NodeCommandDraft;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::NodeCapabilities;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{BuildRunId, NodeCommandId, NodeId};
use a3s_cloud_control_plane::modules::sources::domain::{BuildRecipe, ExternalSourceRevision};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::ArtifactRef;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) const GATE_ENV: &str = "A3S_CLOUD_TEST_G0_EXTERNAL_RELEASE";

const SOURCE_HANDOFF_SCHEMA: &str = "a3s.cloud.g0-private-source-handoff.v1";
const BOX_HANDOFF_SCHEMA: &str = "a3s.cloud.g0-box-release-handoff.v1";
const SOURCE_EVIDENCE_SCHEMA: &str = "a3s.cloud.g0-private-github-provider-evidence.v1";
const BOX_EVIDENCE_SCHEMA: &str = "a3s.cloud.g0-box-build-provider-evidence.v1";
const PUBLICATION_CREDENTIAL_ENV: &str = "A3S_CLOUD_TEST_G0_PUBLICATION_CREDENTIAL";

pub(super) struct GateConfig {
    pub cloud_revision: String,
    pub box_revision: String,
    pub evidence_directory: PathBuf,
    pub source_handoff_path: PathBuf,
    pub box_handoff_directory: PathBuf,
    pub registry_url: String,
    pub registry_repository_prefix: String,
    pub registry_username: String,
    pub registry_password: Zeroizing<String>,
    pub vault_address: String,
    pub vault_token: Zeroizing<String>,
    pub vault_transit_mount: String,
    pub vault_signing_key: String,
}

impl GateConfig {
    pub(super) fn load() -> TestResult<Self> {
        require(GATE_ENV, Some("1"))?;
        let config = Self {
            cloud_revision: require("A3S_CLOUD_TEST_CLOUD_REVISION", None)?,
            box_revision: require("A3S_CLOUD_TEST_BOX_REVISION", None)?,
            evidence_directory: absolute_path("A3S_CLOUD_TEST_G0_EVIDENCE_DIR")?,
            source_handoff_path: absolute_path("A3S_CLOUD_TEST_G0_PRIVATE_SOURCE_HANDOFF")?,
            box_handoff_directory: absolute_path("A3S_CLOUD_TEST_G0_BOX_HANDOFF_DIR")?,
            registry_url: require("A3S_CLOUD_TEST_REGISTRY_URL", None)?,
            registry_repository_prefix: require("A3S_CLOUD_TEST_REGISTRY_REPOSITORY_PREFIX", None)?,
            registry_username: require("A3S_CLOUD_TEST_REGISTRY_USERNAME", None)?,
            registry_password: Zeroizing::new(require("A3S_CLOUD_TEST_REGISTRY_PASSWORD", None)?),
            vault_address: require("A3S_CLOUD_TEST_VAULT_ADDR", None)?,
            vault_token: Zeroizing::new(require("A3S_CLOUD_TEST_VAULT_TOKEN", None)?),
            vault_transit_mount: require("A3S_CLOUD_TEST_VAULT_TRANSIT_MOUNT", None)?,
            vault_signing_key: require("A3S_CLOUD_TEST_VAULT_SIGNING_KEY", None)?,
        };
        validate_revision(&config.cloud_revision, "Cloud")?;
        validate_revision(&config.box_revision, "Box")?;
        validate_https_origin(&config.vault_address, "Vault address")?;
        if config.source_handoff_path.parent().is_none()
            || config.box_handoff_directory.parent().is_none()
            || config.evidence_directory.parent().is_none()
        {
            return Err(test_error("G0 handoff and evidence paths are unsafe"));
        }
        Ok(config)
    }

    pub(super) fn registry_settings(&self) -> TestResult<RegistrySettings> {
        let base = Url::parse(&self.registry_url)?;
        if base.scheme() != "https"
            || base.username() != ""
            || base.password().is_some()
            || !matches!(base.path(), "" | "/")
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(test_error(
                "G0 Registry URL must be an HTTPS origin without credentials or a path",
            ));
        }
        let host = base
            .host_str()
            .ok_or_else(|| test_error("G0 Registry URL omitted its host"))?;
        let authority = match base.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        Ok(RegistrySettings { authority })
    }

    pub(super) fn install_registry_credential(&self) -> TestResult<EnvironmentGuard> {
        let material = serde_json::to_string(&serde_json::json!({
            "schema": RegistryCredentialMaterial::SCHEMA,
            "username": self.registry_username.as_str(),
            "password": self.registry_password.as_str(),
        }))?;
        Ok(EnvironmentGuard::set(PUBLICATION_CREDENTIAL_ENV, material))
    }
}

pub(super) struct RegistrySettings {
    pub authority: String,
}

impl RegistrySettings {
    pub(super) fn publisher(
        &self,
        outputs: Arc<OciBuildOutputValidator>,
        repository_prefix: &str,
    ) -> TestResult<OciRegistryArtifactPublisher> {
        Ok(OciRegistryArtifactPublisher::new(
            outputs,
            Duration::from_secs(30),
            std::iter::empty(),
            OciRegistryArtifactPublisherOptions {
                registry: self.authority.clone(),
                repository_prefix: repository_prefix.to_owned(),
                credential_env: PUBLICATION_CREDENTIAL_ENV.into(),
                allow_anonymous: false,
            },
        )?)
    }
}

pub(super) struct EnvironmentGuard {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvironmentGuard {
    fn set(name: &'static str, value: String) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrivateSourceHandoff {
    pub schema: String,
    pub cloud_revision: String,
    pub build_run_id: BuildRunId,
    pub revision: ExternalSourceRevision,
    pub source_content_digest: String,
    pub input_artifact: BuildArtifact,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct BoxReleaseHandoff {
    pub schema: String,
    pub cloud_revision: String,
    pub box_revision: String,
    pub build_run_id: BuildRunId,
    pub source_content_digest: String,
    pub source_artifact: ArtifactRef,
    pub build_request_digest: String,
    pub output: NodeBoxBuildOutput,
    pub commands: [NodeCommandEnvelope; 3],
}

pub(super) struct GateInputs {
    pub source: PrivateSourceHandoff,
    pub box_release: BoxReleaseHandoff,
    pub source_archive: PathBuf,
    pub box_output_archive: PathBuf,
    pub source_evidence_digest: String,
    pub box_evidence_digest: String,
}

pub(super) async fn load_inputs(config: &GateConfig) -> TestResult<GateInputs> {
    let source: PrivateSourceHandoff =
        serde_json::from_slice(&tokio::fs::read(&config.source_handoff_path).await?)?;
    let box_handoff_path = config.box_handoff_directory.join("box-output.json");
    let box_release: BoxReleaseHandoff =
        serde_json::from_slice(&tokio::fs::read(&box_handoff_path).await?)?;
    if source.schema != SOURCE_HANDOFF_SCHEMA
        || source.cloud_revision != config.cloud_revision
        || source.build_run_id != BuildRun::id_for(source.revision.id)
        || source.revision.clone().validate()? != source.revision
        || source.input_artifact.validate().is_err()
        || source.source_content_digest.len() != 71
        || !source.source_content_digest.starts_with("sha256:")
    {
        return Err(test_error(
            "private source handoff failed identity validation",
        ));
    }
    if box_release.schema != BOX_HANDOFF_SCHEMA
        || box_release.cloud_revision != config.cloud_revision
        || box_release.box_revision != config.box_revision
        || box_release.build_run_id != source.build_run_id
        || box_release.source_content_digest != source.source_content_digest
        || box_release.source_artifact.uri != source.input_artifact.uri
        || box_release.source_artifact.digest != source.input_artifact.digest
        || box_release.source_artifact.media_type != source.input_artifact.media_type
    {
        return Err(test_error(
            "Box release handoff changed the private source identity",
        ));
    }
    validate_box_commands(&box_release, &source.revision.recipe)?;

    let source_archive = config
        .source_handoff_path
        .parent()
        .ok_or_else(|| test_error("private source handoff has no parent"))?
        .join("source-input.tar");
    let box_output_archive = config.box_handoff_directory.join("box-output.tar");
    verify_file_artifact(&source_archive, &source.input_artifact).await?;
    let output_artifact = box_output_artifact(&box_release.output)?;
    verify_file_reference(
        &box_output_archive,
        output_artifact,
        box_release.output.artifact.size_bytes,
    )
    .await?;

    let source_evidence =
        tokio::fs::read(config.evidence_directory.join("github-private-source.json")).await?;
    let box_evidence =
        tokio::fs::read(config.evidence_directory.join("box-build-provider.json")).await?;
    let build_run_identity_digest = sha256(source.build_run_id.to_string().as_bytes());
    validate_public_evidence(
        &source_evidence,
        SOURCE_EVIDENCE_SCHEMA,
        &config.cloud_revision,
        &[
            ("contentDigest", source.source_content_digest.as_str()),
            ("buildInputDigest", source.input_artifact.digest.as_str()),
            ("buildRunIdentityDigest", build_run_identity_digest.as_str()),
        ],
    )?;
    let build_run_id = source.build_run_id.to_string();
    validate_public_evidence(
        &box_evidence,
        BOX_EVIDENCE_SCHEMA,
        &config.cloud_revision,
        &[
            ("buildRunId", build_run_id.as_str()),
            ("sourceKind", "private_github"),
            ("sourceContentDigest", source.source_content_digest.as_str()),
            ("sourceDigest", source.input_artifact.digest.as_str()),
            (
                "firstOutputDigest",
                box_release.output.descriptor.digest.as_str(),
            ),
            (
                "retryOutputDigest",
                box_release.output.descriptor.digest.as_str(),
            ),
        ],
    )?;
    Ok(GateInputs {
        source,
        box_release,
        source_archive,
        box_output_archive,
        source_evidence_digest: sha256(&source_evidence),
        box_evidence_digest: sha256(&box_evidence),
    })
}

fn validate_box_commands(handoff: &BoxReleaseHandoff, recipe: &BuildRecipe) -> TestResult {
    let expected_kinds = ["box_build_start", "box_build_inspect", "box_build_remove"];
    let mut request: Option<&NodeBoxBuildRequest> = None;
    for (index, (command, expected_kind)) in handoff.commands.iter().zip(expected_kinds).enumerate()
    {
        command.validate()?;
        if command.sequence != (index + 1) as u64
            || command.aggregate_id != handoff.build_run_id.as_uuid()
            || command.node_id != handoff.commands[0].node_id
            || command.correlation_id != handoff.commands[0].correlation_id
            || command.payload.kind() != expected_kind
        {
            return Err(test_error(
                "Box release command chain is not exact or monotonic",
            ));
        }
        let current = box_request(&command.payload)?;
        if request.is_some_and(|existing| existing != current) {
            return Err(test_error(
                "Box release command chain changed its build request",
            ));
        }
        request = Some(current);
    }
    let request = request.ok_or_else(|| test_error("Box release command chain is empty"))?;
    if request.source != handoff.source_artifact
        || request.binding_digest()? != handoff.build_request_digest
        || request.generation != 1
    {
        return Err(test_error(
            "Box release request changed its admitted source or binding",
        ));
    }
    validate_build_plan(request, recipe, handoff.build_run_id)?;
    handoff.output.validate()?;
    Ok(())
}

fn validate_build_plan(
    request: &NodeBoxBuildRequest,
    recipe: &BuildRecipe,
    build_run_id: BuildRunId,
) -> TestResult {
    let expected_assembly =
        (recipe.platforms().len() > 1).then(|| format!("cloud-build-{build_run_id}-assembly"));
    if request.plans.len() != recipe.platforms().len()
        || request.assembly_reference != expected_assembly
    {
        return Err(test_error(
            "Box release request changed the production recipe platform set",
        ));
    }
    for (plan, platform) in request.plans.iter().zip(recipe.platforms()) {
        let parsed = BoxBuildPlan::parse_acl(&plan.plan_acl)?;
        let expected_operation = format!(
            "cloud-build-{build_run_id}-{}",
            platform.as_str().replace('/', "-")
        );
        if plan.operation_id != expected_operation
            || plan.cache.is_some()
            || parsed.canonical_acl()? != plan.plan_acl
            || parsed.context() != recipe.context_path()
            || parsed.file() != recipe.dockerfile_path()
            || parsed.platform().to_string() != platform.as_str()
            || parsed.target() != recipe.target()
            || parsed.network() != BuildNetworkPolicy::None
            || parsed.cache() != BuildCachePolicy::ContentAddressed
        {
            return Err(test_error(
                "Box release request is not the canonical production recipe projection",
            ));
        }
    }
    Ok(())
}

fn box_request(payload: &NodeCommandPayload) -> TestResult<&NodeBoxBuildRequest> {
    match payload {
        NodeCommandPayload::BoxBuildStart { request }
        | NodeCommandPayload::BoxBuildInspect { request }
        | NodeCommandPayload::BoxBuildRemove { request } => Ok(request),
        _ => Err(test_error("G0 Box handoff contains a non-build command")),
    }
}

fn box_output_artifact(output: &NodeBoxBuildOutput) -> TestResult<&ArtifactRef> {
    output.validate()?;
    Ok(&output.artifact.artifact)
}

async fn verify_file_artifact(path: &Path, artifact: &BuildArtifact) -> TestResult {
    let reference = ArtifactRef {
        uri: artifact.uri.clone(),
        digest: artifact.digest.clone(),
        media_type: artifact.media_type.clone(),
    };
    verify_file_reference(path, &reference, artifact.size_bytes).await
}

async fn verify_file_reference(path: &Path, artifact: &ArtifactRef, size_bytes: u64) -> TestResult {
    let bytes = tokio::fs::read(path).await?;
    if bytes.len() as u64 != size_bytes
        || artifact.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
        || artifact.uri != a3s_cloud_contracts::artifact_uri(&artifact.digest)?
        || sha256(&bytes) != artifact.digest
    {
        return Err(test_error("G0 handoff Artifact changed before consumption"));
    }
    Ok(())
}

fn validate_public_evidence(
    bytes: &[u8],
    schema: &str,
    cloud_revision: &str,
    expected: &[(&str, &str)],
) -> TestResult {
    let document: Value = serde_json::from_slice(bytes)?;
    let checks = document["checks"]
        .as_object()
        .filter(|checks| !checks.is_empty())
        .ok_or_else(|| test_error("G0 provider evidence omitted its checks"))?;
    if document["schema"] != schema
        || document["cloudRevision"] != cloud_revision
        || checks.values().any(|value| value != true)
        || expected
            .iter()
            .any(|(name, value)| document[*name] != *value)
    {
        return Err(test_error(
            "G0 provider evidence does not match its handoff",
        ));
    }
    Ok(())
}

pub(super) async fn connect(url: &str) -> TestResult<PostgresExecutor> {
    Ok(connect_and_migrate(url, 8).await?)
}

pub(super) async fn admit_artifact(
    store: &Arc<LocalNodeArtifactStore>,
    artifact: &BuildArtifact,
    path: &Path,
) -> TestResult {
    verify_file_artifact(path, artifact).await?;
    let reference = ArtifactRef {
        uri: artifact.uri.clone(),
        digest: artifact.digest.clone(),
        media_type: artifact.media_type.clone(),
    };
    let descriptor = NodeArtifactDescriptor::new(reference, artifact.size_bytes)?;
    let write = store
        .put(&descriptor, Box::pin(tokio::fs::File::open(path).await?))
        .await?;
    if write.descriptor != descriptor {
        return Err(test_error(
            "node Artifact store changed an admitted handoff",
        ));
    }
    Ok(())
}

pub(super) async fn admit_box_output(
    store: &Arc<LocalNodeArtifactStore>,
    output: &NodeBoxBuildOutput,
    path: &Path,
) -> TestResult<BuildArtifact> {
    let reference = box_output_artifact(output)?.clone();
    let artifact = BuildArtifact::new(
        reference.uri.clone(),
        reference.digest.clone(),
        reference.media_type.clone(),
        output.artifact.size_bytes,
    )?;
    admit_artifact(store, &artifact, path).await?;
    Ok(artifact)
}

pub(super) async fn seed_tenant(
    executor: &PostgresExecutor,
    source: &PrivateSourceHandoff,
) -> TestResult {
    let revision = &source.revision;
    let database = Database::new(PostgresDialect, executor.clone());
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(revision.organization_id.as_uuid())
            .append(", 'G0 external release', 'g0-external-release', 1, ")
            .bind(revision.accepted_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(revision.organization_id.as_uuid())
            .append(", ")
            .bind(revision.project_id.as_uuid())
            .append(", 'G0 release project', 'g0-release-project', 1, ")
            .bind(revision.accepted_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(revision.organization_id.as_uuid())
            .append(", ")
            .bind(revision.project_id.as_uuid())
            .append(", ")
            .bind(revision.environment_id.as_uuid())
            .append(", 'G0 release', 'g0-release', 1, ")
            .bind(revision.accepted_at)
            .append(")"),
        )
        .await?;
    Ok(())
}

pub(super) async fn enqueue_box_commands(
    executor: &PostgresExecutor,
    organization_id: uuid::Uuid,
    handoff: &BoxReleaseHandoff,
    box_revision: &str,
) -> TestResult<(NodeId, NodeCommandId, NodeCommandId)> {
    let node_id = NodeId::from_uuid(handoff.commands[0].node_id);
    let database = Database::new(PostgresDialect, executor.clone());
    let capabilities = NodeCapabilities::new(
        "a3s-box",
        box_revision,
        serde_json::json!({"boxBuild": true}),
    )?;
    database
        .execute(
            sql_query::<()>(
                "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, aggregate_version) values (",
            )
            .bind(organization_id)
            .append(", ")
            .bind(node_id.as_uuid())
            .append(", 'G0 Box build node', 'g0-box-build-node', 'ready', ")
            .bind(uuid::Uuid::now_v7())
            .append(", 'g0-conformance', 'a3s-box', ")
            .bind(box_revision)
            .append(", ")
            .bind(capabilities.digest())
            .append(", ")
            .bind(capabilities.document().clone())
            .append(", ")
            .bind(handoff.commands[0].issued_at)
            .append(", ")
            .bind(handoff.commands[0].issued_at)
            .append(", 1)"),
        )
        .await?;
    let fleet = PostgresNodeRepository::new(executor.clone());
    for envelope in &handoff.commands {
        let draft = NodeCommandDraft {
            proposed_command_id: NodeCommandId::from_uuid(envelope.command_id),
            node_id,
            aggregate_id: envelope.aggregate_id,
            payload: envelope.payload.clone(),
            issued_at: envelope.issued_at,
            not_after: envelope.not_after,
            correlation_id: envelope.correlation_id,
        };
        let accepted = fleet.enqueue_command(draft.clone()).await?;
        if accepted.replayed
            || accepted.value.sequence != envelope.sequence
            || accepted.value.envelope(envelope.lease_id)? != *envelope
        {
            return Err(test_error("PostgreSQL Fleet changed the exact Box command"));
        }
        let replay = fleet.enqueue_command(draft).await?;
        if !replay.replayed || replay.value != accepted.value {
            return Err(test_error(
                "PostgreSQL Fleet did not replay the exact Box command",
            ));
        }
    }
    Ok((
        node_id,
        NodeCommandId::from_uuid(handoff.commands[0].command_id),
        NodeCommandId::from_uuid(handoff.commands[2].command_id),
    ))
}

pub(super) fn output_validator(
    store: Arc<LocalNodeArtifactStore>,
    root: &Path,
) -> TestResult<Arc<OciBuildOutputValidator>> {
    Ok(Arc::new(OciBuildOutputValidator::new(
        store,
        root.join("validation"),
        128 * 1024 * 1024,
        100_000,
        256 * 1024 * 1024,
        10_000,
        256 * 1024 * 1024,
    )?))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn require(name: &str, expected: Option<&str>) -> TestResult<String> {
    let value = std::env::var(name).map_err(|_| test_error(format!("{name} is required")))?;
    if value.trim().is_empty() || expected.is_some_and(|expected| value != expected) {
        return Err(test_error(format!("{name} has an invalid value")));
    }
    Ok(value)
}

fn absolute_path(name: &str) -> TestResult<PathBuf> {
    let path = PathBuf::from(require(name, None)?);
    if !path.is_absolute() {
        return Err(test_error(format!("{name} must be absolute")));
    }
    Ok(path)
}

fn validate_revision(value: &str, label: &str) -> TestResult {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(test_error(format!(
            "G0 {label} revision must be a full lowercase Git SHA"
        )));
    }
    Ok(())
}

fn validate_https_origin(value: &str, label: &str) -> TestResult {
    let url = Url::parse(value)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(test_error(format!(
            "G0 {label} must be an HTTPS origin without credentials or a path"
        )));
    }
    Ok(())
}

fn test_error(message: impl Into<String>) -> Box<dyn std::error::Error> {
    std::io::Error::other(message.into()).into()
}
