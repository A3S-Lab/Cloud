use super::fixture::{connect_driver, found, require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_runtime::contract::{
    RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeUnitState,
};
use a3s_runtime::{RuntimeClient, RuntimeDriver, RuntimeError, RuntimeResult, RuntimeStateStore};
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const PROVIDER_RESTART_LOG_MARKER: &str = "provider-restart-log";

impl DockerConformanceFixture {
    pub(crate) async fn run_recovery(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        self.create_before_ack_client_and_provider_restart().await?;
        self.external_deletion_and_single_replacement(client)
            .await?;
        self.duplicate_resource_detection(client).await
    }

    async fn create_before_ack_client_and_provider_restart(&self) -> RuntimeResult<()> {
        let spec = specs::service_spec(
            specs::unit_id(&self.namespace, "recovery-ack"),
            "printf 'provider-restart-log\\n'; exec sleep 300",
        );
        let apply = specs::apply("recovery-create-before-ack", spec.clone());
        let reservation = self.store.reserve_apply(&apply, now_ms()).await?;
        let created = self
            .driver
            .apply(&apply.spec, &reservation.record.observation)
            .await?;
        let original_resource = resource_id(&created)?.to_owned();

        let restarted_client = self.restarted_client(self.driver.clone());
        let reattached = restarted_client.apply(&apply).await?;
        require(
            resource_id(&reattached)? == original_resource,
            "client restart did not reattach the create-before-ack Docker resource",
        )?;
        let before_restart_logs = wait_for_provider_restart_log(&restarted_client, &spec).await?;
        let (before_restart_cursor, before_restart_sequence) = before_restart_logs
            .iter()
            .find(|chunk| chunk.data.contains(PROVIDER_RESTART_LOG_MARKER))
            .map(|chunk| (chunk.cursor.clone(), chunk.sequence))
            .ok_or_else(|| {
                RuntimeError::Protocol(
                    "Docker provider restart fixture omitted its pre-restart log cursor".into(),
                )
            })?;

        self.restart_provider().await?;
        let restarted_driver = Arc::new(connect_driver(&self.namespace, self.node_id).await?);
        let record = self.store.load(&spec.unit_id).await?;
        let mut provider_observation = None;
        for _ in 0..60 {
            match restarted_driver.inspect(&record).await? {
                RuntimeInspection::Found { observation, .. }
                    if observation.state == RuntimeUnitState::Running =>
                {
                    provider_observation = Some(*observation);
                    break;
                }
                RuntimeInspection::Found { .. } | RuntimeInspection::NotFound { .. } => {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
        let provider_observation = provider_observation.ok_or_else(|| {
            RuntimeError::ProviderUnavailable(
                "Docker Service did not recover after isolated provider restart".into(),
            )
        })?;
        require(
            resource_id(&provider_observation)? == original_resource,
            "Docker provider restart substituted durable resource identity",
        )?;
        let provider_restarted_client = self.restarted_client(restarted_driver);
        let inspected = found(provider_restarted_client.inspect(&spec.unit_id).await?)?;
        require(
            inspected.state == RuntimeUnitState::Running
                && resource_id(&inspected)? == original_resource,
            "Runtime did not converge after Docker provider restart",
        )?;
        let after_restart_logs =
            wait_for_provider_restart_log(&provider_restarted_client, &spec).await?;
        require(
            after_restart_logs
                .iter()
                .any(|chunk| chunk.cursor == before_restart_cursor),
            "Docker provider restart lost the durable pre-restart log cursor",
        )?;
        let resumed = provider_restarted_client
            .logs(&RuntimeLogQuery {
                schema: RuntimeLogQuery::SCHEMA.into(),
                unit_id: spec.unit_id.clone(),
                generation: spec.generation,
                cursor: Some(before_restart_cursor.clone()),
                limit: 32,
                stream: None,
            })
            .await?;
        require(
            resumed.iter().all(|chunk| {
                chunk.cursor != before_restart_cursor && chunk.sequence > before_restart_sequence
            }),
            "Docker provider restart replayed or reordered the requested log cursor",
        )?;
        provider_restarted_client
            .remove(&specs::action("recovery-restart-remove", &spec))
            .await?;
        Ok(())
    }

    async fn external_deletion_and_single_replacement(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let spec = specs::service_spec(
            specs::unit_id(&self.namespace, "recovery-loss"),
            "exec sleep 300",
        );
        let first = client
            .apply(&specs::apply("recovery-loss-initial", spec.clone()))
            .await?;
        let first_resource = resource_id(&first)?.to_owned();
        self.docker_call(
            "delete provider resource externally",
            self.docker.remove_container(
                &first_resource,
                Some(RemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            ),
        )
        .await?;

        let lost = found(client.inspect(&spec.unit_id).await?)?;
        require(
            lost.state == RuntimeUnitState::Unknown,
            "external Docker deletion did not become durable unknown",
        )?;
        let replacement_request = specs::apply("recovery-loss-replace", spec.clone());
        let replacement = client.apply(&replacement_request).await?;
        let replacement_resource = resource_id(&replacement)?.to_owned();
        require(
            replacement.state == RuntimeUnitState::Running
                && replacement_resource != first_resource,
            "same-generation recovery did not adopt one replacement resource",
        )?;
        let replay = client.apply(&replacement_request).await?;
        require(
            resource_id(&replay)? == replacement_resource,
            "same-generation recovery replay created another Docker resource",
        )?;
        require(
            self.unit_container_ids(&spec.unit_id).await?.len() == 1,
            "same-generation recovery left duplicate Docker resources",
        )?;
        client
            .remove(&specs::action("recovery-loss-remove", &spec))
            .await?;
        Ok(())
    }

    async fn duplicate_resource_detection(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let spec = specs::service_spec(
            specs::unit_id(&self.namespace, "recovery-duplicate"),
            "exec sleep 300",
        );
        let observation = client
            .apply(&specs::apply("recovery-duplicate-apply", spec.clone()))
            .await?;
        let original = self
            .docker_call(
                "inspect duplicate source",
                self.docker
                    .inspect_container(resource_id(&observation)?, None),
            )
            .await?;
        let config = original.config.ok_or_else(|| {
            RuntimeError::Protocol("Docker duplicate source omitted configuration".into())
        })?;
        let duplicate_name = format!(
            "a3s-{}-duplicate-{}",
            self.namespace,
            &Uuid::now_v7().simple().to_string()[..8]
        );
        let duplicate = self
            .docker_call(
                "create duplicate managed resource",
                self.docker.create_container(
                    Some(CreateContainerOptions {
                        name: duplicate_name.as_str(),
                        platform: None,
                    }),
                    Config {
                        image: config.image,
                        cmd: Some(vec!["/bin/sh".into(), "-c".into(), "exec sleep 300".into()]),
                        labels: config.labels,
                        ..Default::default()
                    },
                ),
            )
            .await?;

        let rejection = client.inspect(&spec.unit_id).await;
        require(
            matches!(
                rejection,
                Err(RuntimeError::Protocol(message))
                    if message.contains("multiple managed containers")
            ),
            "Docker duplicate managed resources did not fail closed",
        )?;
        self.docker_call(
            "remove injected duplicate resource",
            self.docker.remove_container(
                &duplicate.id,
                Some(RemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            ),
        )
        .await?;
        client
            .remove(&specs::action("recovery-duplicate-remove", &spec))
            .await?;
        Ok(())
    }
}

async fn wait_for_provider_restart_log(
    client: &dyn RuntimeClient,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> RuntimeResult<Vec<RuntimeLogChunk>> {
    let query = RuntimeLogQuery {
        schema: RuntimeLogQuery::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        cursor: None,
        limit: 32,
        stream: None,
    };
    for _ in 0..60 {
        let chunks = client.logs(&query).await?;
        if chunks
            .iter()
            .any(|chunk| chunk.data.contains(PROVIDER_RESTART_LOG_MARKER))
        {
            return Ok(chunks);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(RuntimeError::Protocol(
        "Docker provider restart fixture emitted no recoverable log marker".into(),
    ))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}
