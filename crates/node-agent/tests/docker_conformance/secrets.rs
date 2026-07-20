use super::fixture::{require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_cloud_contracts::CloudSecretReference;
use a3s_cloud_node_agent::{NodeControlClientError, NodeSecretTransport, SecretMaterial};
use a3s_runtime::contract::{
    RuntimeLogQuery, RuntimeUnitSpec, RuntimeUnitState, SecretReference, SecretTarget,
};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use async_trait::async_trait;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

pub(crate) fn conformance_secret_transport() -> Arc<dyn NodeSecretTransport> {
    Arc::new(ConformanceSecretTransport)
}

fn conformance_secret_value(reference: CloudSecretReference) -> String {
    format!(
        "a3s-runtime-conformance-secret-{}",
        reference.secret_id.simple()
    )
}

struct ConformanceSecretTransport;

#[async_trait]
impl NodeSecretTransport for ConformanceSecretTransport {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        SecretMaterial::new(conformance_secret_value(reference).into_bytes())
            .map_err(NodeControlClientError::Invalid)
    }
}

impl DockerConformanceFixture {
    pub(crate) async fn verify_secret_nondisclosure_retry_and_recovery(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let reference = CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 1)
            .map_err(RuntimeError::InvalidRequest)?;
        let secret_value = Zeroizing::new(conformance_secret_value(reference));
        let mut spec = specs::service_spec(
            specs::unit_id(&self.namespace, "security-secret"),
            "set -eu; value=$(cat /run/secrets/runtime-token); printf 'secret=%s\\n' \"$value\"; exec sleep 300",
        );
        spec.secrets.push(SecretReference {
            name: "runtime-token".into(),
            reference: reference.to_string(),
            target: SecretTarget::File {
                path: "/run/secrets/runtime-token".into(),
                mode: 0o400,
            },
        });
        let spec_digest = spec.digest().map_err(RuntimeError::InvalidRequest)?;
        let secret_directory = self.secret_generation_directory(&spec_digest)?;
        require_serialized_value_absent("Runtime spec", &spec, secret_value.as_str())?;

        let first = client
            .apply(&specs::apply("security-secret-initial", spec.clone()))
            .await?;
        let first_resource = resource_id(&first)?.to_owned();
        require(
            first.state == RuntimeUnitState::Running,
            "Docker Secret conformance Service did not start",
        )?;
        require_serialized_value_absent(
            "Runtime apply observation",
            &first,
            secret_value.as_str(),
        )?;

        let execution = async {
            self.require_secret_surfaces_redacted(
                client,
                &spec,
                &first_resource,
                secret_value.as_str(),
            )
            .await?;
            require_single_secret_file(&secret_directory, secret_value.as_bytes()).await?;

            let retry = client
                .apply(&specs::apply("security-secret-retry", spec.clone()))
                .await?;
            require(
                resource_id(&retry)? == first_resource,
                "Docker Secret retry duplicated the provider resource",
            )?;
            self.require_single_secret_unit_container(&spec, &first_resource, "retry")
                .await?;
            require_single_secret_file(&secret_directory, secret_value.as_bytes()).await?;

            if std::env::var_os("A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER").is_some() {
                self.restart_provider().await?;
                let recovered = client
                    .apply(&specs::apply("security-secret-recovery", spec.clone()))
                    .await?;
                require(
                    recovered.state == RuntimeUnitState::Running
                        && resource_id(&recovered)? == first_resource,
                    "Docker Secret provider restart did not adopt the original Service",
                )?;
                self.require_single_secret_unit_container(
                    &spec,
                    &first_resource,
                    "provider restart",
                )
                .await?;
                require_single_secret_file(&secret_directory, secret_value.as_bytes()).await?;
                self.require_secret_surfaces_redacted(
                    client,
                    &spec,
                    &first_resource,
                    secret_value.as_str(),
                )
                .await?;
            }
            Ok(())
        }
        .await;

        client
            .remove(&specs::action("security-secret-remove", &spec))
            .await?;
        require(
            !tokio::fs::try_exists(&secret_directory)
                .await
                .map_err(|error| {
                    RuntimeError::ProviderUnavailable(format!(
                        "could not inspect Docker Secret cleanup: {error}"
                    ))
                })?,
            "Docker Secret generation directory remained after removal",
        )?;
        execution?;
        eprintln!(
            "A3S_RUNTIME_SECURITY_CASE_PASS case=SECURITY-SECRET-NONDISCLOSURE retry=true provider_restart={}",
            std::env::var_os("A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER").is_some()
        );
        Ok(())
    }

    async fn require_single_secret_unit_container(
        &self,
        spec: &RuntimeUnitSpec,
        expected: &str,
        phase: &str,
    ) -> RuntimeResult<()> {
        let ids = self.unit_container_ids(&spec.unit_id).await?;
        require(
            ids == vec![expected.to_owned()],
            format!(
                "Docker Secret {phase} did not preserve one provider resource: count={}, ids={ids:?}",
                ids.len()
            ),
        )
    }

    async fn require_secret_surfaces_redacted(
        &self,
        client: &dyn RuntimeClient,
        spec: &RuntimeUnitSpec,
        resource: &str,
        secret_value: &str,
    ) -> RuntimeResult<()> {
        let provider = self
            .docker_call(
                "inspect Secret conformance container",
                self.docker.inspect_container(resource, None),
            )
            .await?;
        require_serialized_value_absent("Docker inspection", &provider, secret_value)?;

        let inspection = client.inspect(&spec.unit_id).await?;
        require_serialized_value_absent("Runtime inspection", &inspection, secret_value)?;

        let mut chunks = Vec::new();
        for _ in 0..30 {
            chunks = client
                .logs(&RuntimeLogQuery {
                    schema: RuntimeLogQuery::SCHEMA.into(),
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                    cursor: None,
                    limit: 32,
                    stream: None,
                })
                .await?;
            if chunks.iter().any(|chunk| chunk.data.contains("[REDACTED]")) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        require(
            !chunks.is_empty()
                && chunks
                    .iter()
                    .all(|chunk| !chunk.data.contains(secret_value))
                && chunks
                    .iter()
                    .any(|chunk| chunk.data == "secret=[REDACTED]\n"),
            "Docker Secret log material was absent, unredacted, or malformed",
        )
    }
}

fn require_serialized_value_absent<T>(
    surface: &str,
    value: &T,
    secret_value: &str,
) -> RuntimeResult<()>
where
    T: serde::Serialize,
{
    let serialized = serde_json::to_string(value).map_err(|error| {
        RuntimeError::Protocol(format!(
            "could not serialize {surface} for Secret nondisclosure: {error}"
        ))
    })?;
    require(
        !serialized.contains(secret_value),
        format!("{surface} disclosed Docker Secret material"),
    )
}

async fn require_single_secret_file(
    directory: &Path,
    secret_value: &[u8],
) -> RuntimeResult<PathBuf> {
    let mut entries = tokio::fs::read_dir(directory).await.map_err(|error| {
        RuntimeError::ProviderUnavailable(format!(
            "could not inspect Docker Secret generation directory: {error}"
        ))
    })?;
    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        RuntimeError::ProviderUnavailable(format!(
            "could not read Docker Secret generation entry: {error}"
        ))
    })? {
        let file_type = entry.file_type().await.map_err(|error| {
            RuntimeError::ProviderUnavailable(format!(
                "could not inspect Docker Secret generation entry type: {error}"
            ))
        })?;
        require(
            file_type.is_file(),
            "Docker Secret generation directory contains a non-file entry",
        )?;
        files.push(entry.path());
    }
    require(
        files.len() == 1,
        "Docker Secret retry or recovery did not preserve exactly one material file",
    )?;
    let path = files
        .pop()
        .ok_or_else(|| RuntimeError::Protocol("Docker Secret material file is absent".into()))?;
    let material = Zeroizing::new(tokio::fs::read(&path).await.map_err(|error| {
        RuntimeError::ProviderUnavailable(format!(
            "could not read Docker Secret conformance material: {error}"
        ))
    })?);
    let metadata = tokio::fs::metadata(&path).await.map_err(|error| {
        RuntimeError::ProviderUnavailable(format!(
            "could not inspect Docker Secret conformance material: {error}"
        ))
    })?;
    require(
        material.as_slice() == secret_value && metadata.permissions().mode() & 0o777 == 0o400,
        "Docker Secret material content or mode changed across retry or recovery",
    )?;
    Ok(path)
}
