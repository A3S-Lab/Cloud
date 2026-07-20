use super::fixture::{found, require, resource_id, DockerConformanceFixture};
use super::specs::{self, BUSYBOX_DIGEST};
use a3s_runtime::contract::RuntimeUnitState;
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use bollard::models::HostConfig;
use std::collections::HashMap;
use uuid::Uuid;

impl DockerConformanceFixture {
    pub(crate) async fn run_security(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        self.verify_digest_namespace_and_least_privilege(client)
            .await?;
        self.verify_hostile_identity_is_not_a_provider_name(client)
            .await?;
        self.verify_metadata_tamper_fails_closed(client).await?;
        self.verify_secret_nondisclosure_retry_and_recovery(client)
            .await
    }

    async fn verify_digest_namespace_and_least_privilege(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let spec = specs::service_spec(
            specs::unit_id(&self.namespace, "security-binding"),
            "exec sleep 300",
        );
        let digest = spec.digest().map_err(RuntimeError::InvalidRequest)?;
        let decoy_name = format!(
            "a3s-other-namespace-{}",
            &Uuid::now_v7().simple().to_string()[..12]
        );
        let decoy_labels = HashMap::from([
            ("a3s.cloud.managed".to_owned(), "true".to_owned()),
            (
                "a3s.cloud.namespace".to_owned(),
                format!("other-{}", self.namespace),
            ),
            ("a3s.cloud.node-id".to_owned(), self.node_id.to_string()),
            ("a3s.runtime.unit-id".to_owned(), spec.unit_id.clone()),
            (
                "a3s.runtime.generation".to_owned(),
                spec.generation.to_string(),
            ),
            ("a3s.runtime.spec-digest".to_owned(), digest.clone()),
        ]);
        let decoy = self
            .docker_call(
                "create cross-namespace decoy",
                self.docker.create_container(
                    Some(CreateContainerOptions {
                        name: decoy_name.as_str(),
                        platform: None,
                    }),
                    Config {
                        image: Some(format!("docker.io/library/busybox@{BUSYBOX_DIGEST}")),
                        cmd: Some(vec!["sleep".into(), "300".into()]),
                        labels: Some(decoy_labels),
                        host_config: Some(HostConfig {
                            privileged: Some(false),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                ),
            )
            .await?;

        let execution = async {
            let observation = client
                .apply(&specs::apply("security-binding-apply", spec.clone()))
                .await?;
            require(
                resource_id(&observation)? != decoy.id,
                "Docker driver adopted a container from another namespace",
            )?;
            let inspection = self
                .docker_call(
                    "inspect security binding",
                    self.docker
                        .inspect_container(resource_id(&observation)?, None),
                )
                .await?;
            let labels = inspection
                .config
                .as_ref()
                .and_then(|config| config.labels.as_ref())
                .ok_or_else(|| {
                    RuntimeError::Protocol("Docker security container omitted labels".into())
                })?;
            require(
                labels.get("a3s.cloud.namespace") == Some(&self.namespace)
                    && labels.get("a3s.runtime.spec-digest") == Some(&digest),
                "Docker labels did not bind namespace and canonical spec digest",
            )?;
            let host = inspection.host_config.as_ref().ok_or_else(|| {
                RuntimeError::Protocol("Docker security inspection omitted HostConfig".into())
            })?;
            require(
                host.privileged == Some(false) && host.init == Some(true),
                "Docker Runtime container was privileged or omitted its init process",
            )?;
            require(
                host.cap_drop
                    .as_ref()
                    .is_some_and(|values| values.iter().any(|value| value == "ALL")),
                "Docker Runtime container did not drop all ambient capabilities",
            )?;
            require(
                host.security_opt.as_ref().is_some_and(|values| {
                    values
                        .iter()
                        .any(|value| value.starts_with("no-new-privileges"))
                }),
                "Docker Runtime container omitted no-new-privileges",
            )?;
            let image_id = inspection.image.as_deref().ok_or_else(|| {
                RuntimeError::Protocol("Docker security inspection omitted image ID".into())
            })?;
            let image = self
                .docker_call(
                    "inspect digest-pinned image",
                    self.docker.inspect_image(image_id),
                )
                .await?;
            require(
                image.repo_digests.as_ref().is_some_and(|digests| {
                    digests
                        .iter()
                        .any(|repo_digest| repo_digest.ends_with(&format!("@{BUSYBOX_DIGEST}")))
                }),
                "Docker provider image was not bound to the requested immutable digest",
            )?;
            client
                .remove(&specs::action("security-binding-remove", &spec))
                .await?;
            Ok(())
        }
        .await;

        let decoy_cleanup = self
            .docker_call(
                "remove cross-namespace decoy",
                self.docker.remove_container(
                    &decoy.id,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: false,
                        link: false,
                    }),
                ),
            )
            .await;
        decoy_cleanup?;
        execution
    }

    async fn verify_hostile_identity_is_not_a_provider_name(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let hostile_id = format!(
            "{}/../../hostile:unit/{}",
            self.namespace,
            &Uuid::now_v7().simple().to_string()[..8]
        );
        let spec = specs::service_spec(hostile_id.clone(), "exec sleep 300");
        let observation = client
            .apply(&specs::apply("security-hostile-apply", spec.clone()))
            .await?;
        let inspection = self
            .docker_call(
                "inspect hostile identity container",
                self.docker
                    .inspect_container(resource_id(&observation)?, None),
            )
            .await?;
        let name = inspection.name.as_deref().unwrap_or_default();
        require(
            !name.contains("..") && !name.contains(&hostile_id),
            "hostile Runtime unit identity entered the Docker resource name",
        )?;
        client
            .remove(&specs::action("security-hostile-remove", &spec))
            .await?;
        Ok(())
    }

    async fn verify_metadata_tamper_fails_closed(
        &self,
        client: &dyn RuntimeClient,
    ) -> RuntimeResult<()> {
        let spec = specs::service_spec(
            specs::unit_id(&self.namespace, "security-tamper"),
            "exec sleep 300",
        );
        let first = client
            .apply(&specs::apply("security-tamper-initial", spec.clone()))
            .await?;
        let original = self
            .docker_call(
                "inspect metadata tamper source",
                self.docker.inspect_container(resource_id(&first)?, None),
            )
            .await?;
        let name = original
            .name
            .as_deref()
            .and_then(|name| name.strip_prefix('/'))
            .ok_or_else(|| {
                RuntimeError::Protocol("Docker metadata source omitted its name".into())
            })?
            .to_owned();
        let mut config = original.config.ok_or_else(|| {
            RuntimeError::Protocol("Docker metadata source omitted configuration".into())
        })?;
        let labels = config.labels.as_mut().ok_or_else(|| {
            RuntimeError::Protocol("Docker metadata source omitted labels".into())
        })?;
        labels.insert(
            "a3s.runtime.spec-digest".into(),
            format!("sha256:{}", "f".repeat(64)),
        );
        self.docker_call(
            "remove metadata tamper source",
            self.docker.remove_container(
                resource_id(&first)?,
                Some(RemoveContainerOptions {
                    force: true,
                    v: false,
                    link: false,
                }),
            ),
        )
        .await?;
        let impostor = self
            .docker_call(
                "create metadata-tampered impostor",
                self.docker.create_container(
                    Some(CreateContainerOptions {
                        name: name.as_str(),
                        platform: None,
                    }),
                    Config {
                        image: config.image,
                        cmd: Some(vec!["sleep".into(), "300".into()]),
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
                    if message.contains("spec-digest")
                        && message.contains("durable identity")
            ),
            "metadata-tampered Docker resource did not fail closed",
        )?;
        self.docker_call(
            "remove metadata-tampered impostor",
            self.docker.remove_container(
                &impostor.id,
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
            "tampered resource removal did not converge to unknown",
        )?;
        client
            .remove(&specs::action("security-tamper-remove", &spec))
            .await?;
        Ok(())
    }
}
