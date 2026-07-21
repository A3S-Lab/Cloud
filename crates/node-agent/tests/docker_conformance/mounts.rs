use super::artifacts::directory_artifact;
use super::fixture::{connect_driver, found, require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_runtime::contract::{RuntimeMount, RuntimeMountSource, RuntimeUnitSpec, RuntimeUnitState};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use bollard::errors::Error as DockerError;
use bollard::volume::RemoveVolumeOptions;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

impl DockerConformanceFixture {
    pub(crate) async fn run_mounts(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let volume_id = format!("{}-persistent-data", self.namespace);
        let token = format!("mount-token-{}", self.node_id.simple());
        let provider_restart =
            std::env::var_os("A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER").is_some();
        let mut service = specs::service_spec(
            specs::unit_id(&self.namespace, "mount-volume-service"),
            &format!(
                "set -eu; \
                 if [ -e /tmp/a3s-volume-initialized ]; then \
                   test \"$(cat /data/value)\" = '{token}'; \
                 else \
                   test ! -e /data/value; \
                   printf '%s' '{token}' >/data/value; \
                   sync; \
                   : >/tmp/a3s-volume-initialized; \
                 fi; \
                 exec sleep 300"
            ),
        );
        service.mounts = vec![RuntimeMount {
            name: "data".into(),
            source: RuntimeMountSource::Volume {
                volume_id: volume_id.clone(),
            },
            target: "/data".into(),
            read_only: false,
        }];
        let initial = client
            .apply(&specs::apply(
                "mount-volume-service-initial",
                service.clone(),
            ))
            .await?;
        let service_id = resource_id(&initial)?.to_owned();
        require(
            initial.state == RuntimeUnitState::Running,
            "Docker named-volume Service did not start",
        )?;
        self.require_single_mount_unit_container(&service, &service_id, "initial apply")
            .await?;
        let volume_name = self
            .require_single_mount_volume(None, "initial apply")
            .await?;
        self.require_container_bind(&service_id, &volume_name, "/data", "rw")
            .await?;

        let retry = client
            .apply(&specs::apply("mount-volume-service-retry", service.clone()))
            .await?;
        require(
            retry.state == RuntimeUnitState::Running && resource_id(&retry)? == service_id,
            "Docker named-volume retry did not adopt the original Service",
        )?;
        self.require_single_mount_unit_container(&service, &service_id, "caller retry")
            .await?;
        self.require_single_mount_volume(Some(&volume_name), "caller retry")
            .await?;

        if provider_restart {
            self.restart_provider().await?;
            let recovered = client
                .apply(&specs::apply(
                    "mount-volume-service-recovery",
                    service.clone(),
                ))
                .await?;
            require(
                recovered.state == RuntimeUnitState::Running
                    && resource_id(&recovered)? == service_id,
                "Docker provider restart did not adopt the original named-volume Service",
            )?;
            let inspected = found(client.inspect(&service.unit_id).await?)?;
            require(
                inspected.state == RuntimeUnitState::Running
                    && resource_id(&inspected)? == service_id,
                "Runtime inspection lost named-volume Service identity after provider restart",
            )?;
            self.require_single_mount_unit_container(&service, &service_id, "provider restart")
                .await?;
            self.require_single_mount_volume(Some(&volume_name), "provider restart")
                .await?;
            self.require_container_bind(&service_id, &volume_name, "/data", "rw")
                .await?;
        }

        let mut reader = specs::task_spec(
            specs::unit_id(&self.namespace, "mount-volume-reader"),
            &format!(
                "test \"$(cat /data/value)\" = '{token}' && ! touch /data/forbidden 2>/dev/null"
            ),
        );
        reader.mounts = vec![RuntimeMount {
            name: "data".into(),
            source: RuntimeMountSource::Volume { volume_id },
            target: "/data".into(),
            read_only: true,
        }];
        let reader_observation = client
            .apply(&specs::apply("mount-volume-reader-apply", reader.clone()))
            .await?;
        require(
            reader_observation.converges(&reader),
            "Docker volume did not persist data or enforce read-only access",
        )?;
        let reader_id = resource_id(&reader_observation)?.to_owned();
        self.require_container_bind(&reader_id, &volume_name, "/data", "ro")
            .await?;
        self.require_single_mount_volume(Some(&volume_name), "read-only verification")
            .await?;
        client
            .remove(&specs::action("mount-volume-reader-remove", &reader))
            .await?;
        self.require_container_absent(&reader_id).await?;
        client
            .remove(&specs::action("mount-volume-service-remove", &service))
            .await?;
        self.require_container_absent(&service_id).await?;

        let mut tmpfs = specs::task_spec(
            specs::unit_id(&self.namespace, "mount-tmpfs"),
            "printf '#!/bin/sh\\nexit 0\\n' >/scratch/probe && chmod +x /scratch/probe && ! /scratch/probe 2>/dev/null && test -f /scratch/probe",
        );
        tmpfs.mounts = vec![RuntimeMount {
            name: "scratch".into(),
            source: RuntimeMountSource::Tmpfs {
                size_bytes: 4 * 1024 * 1024,
            },
            target: "/scratch".into(),
            read_only: false,
        }];
        let tmpfs_observation = client
            .apply(&specs::apply("mount-tmpfs-apply", tmpfs.clone()))
            .await?;
        require(
            tmpfs_observation.converges(&tmpfs),
            "Docker tmpfs was not writable and noexec",
        )?;
        let tmpfs_id = resource_id(&tmpfs_observation)?.to_owned();
        let tmpfs_inspection = self
            .docker_call(
                "inspect tmpfs container",
                self.docker.inspect_container(&tmpfs_id, None),
            )
            .await?;
        let tmpfs_options = tmpfs_inspection
            .host_config
            .as_ref()
            .and_then(|config| config.tmpfs.as_ref())
            .and_then(|mounts| mounts.get("/scratch"))
            .cloned()
            .unwrap_or_default();
        require(
            ["noexec", "nosuid", "nodev", "size=4194304"]
                .iter()
                .all(|option| tmpfs_options.split(',').any(|value| value == *option)),
            "Docker tmpfs omitted isolation or size options",
        )?;
        client
            .remove(&specs::action("mount-tmpfs-remove", &tmpfs))
            .await?;
        self.require_container_absent(&tmpfs_id).await?;

        let archive = directory_archive("payload/value", b"immutable-input");
        let input = directory_artifact(&archive)?;
        let mut artifact = specs::task_spec(
            specs::unit_id(&self.namespace, "mount-artifact"),
            "test \"$(cat /artifact/payload/value)\" = 'immutable-input' && ! touch /artifact/forbidden 2>/dev/null",
        );
        artifact.mounts = vec![RuntimeMount {
            name: "source".into(),
            source: RuntimeMountSource::Artifact {
                artifact: input.clone(),
            },
            target: "/artifact".into(),
            read_only: true,
        }];
        self.artifacts
            .prepare_input(&artifact, "source", archive)
            .await?;
        let artifact_observation = client
            .apply(&specs::apply("mount-artifact-apply", artifact.clone()))
            .await?;
        require(
            artifact_observation.converges(&artifact),
            "Docker Artifact mount did not expose exact read-only input bytes",
        )?;
        let artifact_id = resource_id(&artifact_observation)?.to_owned();
        self.require_artifact_bind(&artifact_id, "/artifact")
            .await?;
        let restarted_driver = Arc::new(
            connect_driver(&self.namespace, self.node_id, self.artifacts.manager()).await?,
        );
        let reconstructed = found(
            self.inspect_driver(restarted_driver.as_ref(), &artifact.unit_id)
                .await?,
        )?;
        require(
            reconstructed.state == artifact_observation.state
                && reconstructed.spec_digest == artifact_observation.spec_digest
                && resource_id(&reconstructed)? == artifact_id,
            "Docker Artifact mount identity changed after driver reconstruction",
        )?;
        let restarted = self.restarted_client(restarted_driver);
        restarted
            .remove(&specs::action("mount-artifact-remove", &artifact))
            .await?;
        self.require_container_absent(&artifact_id).await?;
        require(
            self.artifacts.spec_views_absent(&artifact).await?
                && self.artifacts.blob_absent(&input).await?,
            "Docker Artifact mount removal retained its view or unreferenced blob",
        )?;

        self.require_single_mount_volume(Some(&volume_name), "pre-cleanup")
            .await?;
        self.docker_call(
            "remove mount profile volume",
            self.docker
                .remove_volume(&volume_name, Some(RemoveVolumeOptions { force: true })),
        )
        .await?;
        require(
            self.namespace_volume_names().await?.is_empty(),
            "Docker mount profile left a named volume",
        )?;
        eprintln!(
            "A3S_RUNTIME_MOUNTS_CASE_PASS case=MOUNT-VOLUME-PERSISTENCE retry=true provider_restart={provider_restart} resource_identity=true volume_identity=true mount_attachment=true"
        );
        Ok(())
    }

    async fn require_artifact_bind(&self, container_id: &str, target: &str) -> RuntimeResult<()> {
        let inspection = self
            .docker_call(
                "inspect Artifact mount container",
                self.docker.inspect_container(container_id, None),
            )
            .await?;
        let mounts = inspection.mounts.unwrap_or_default();
        require(
            mounts.len() == 1
                && mounts[0].typ == Some(bollard::models::MountPointTypeEnum::BIND)
                && mounts[0].destination.as_deref() == Some(target)
                && mounts[0].rw == Some(false)
                && mounts[0]
                    .source
                    .as_deref()
                    .is_some_and(|source| std::path::Path::new(source).is_absolute()),
            format!(
                "Docker Artifact mount did not use one exact absolute read-only bind: {mounts:?}"
            ),
        )
    }

    async fn require_single_mount_unit_container(
        &self,
        spec: &RuntimeUnitSpec,
        expected: &str,
        phase: &str,
    ) -> RuntimeResult<()> {
        let ids = self.unit_container_ids(&spec.unit_id).await?;
        require(
            ids == vec![expected.to_owned()],
            format!(
                "Docker named-volume {phase} did not preserve one provider resource: count={}, ids={ids:?}",
                ids.len()
            ),
        )
    }

    async fn require_single_mount_volume(
        &self,
        expected: Option<&str>,
        phase: &str,
    ) -> RuntimeResult<String> {
        let volumes = self.namespace_volume_names().await?;
        require(
            volumes.len() == 1,
            format!(
                "Docker named-volume {phase} expected one provider volume: count={}, names={volumes:?}",
                volumes.len()
            ),
        )?;
        if let Some(expected) = expected {
            require(
                volumes[0] == expected,
                format!(
                    "Docker named-volume {phase} changed provider volume identity: expected={expected:?}, actual={:?}",
                    volumes[0]
                ),
            )?;
        }
        Ok(volumes[0].clone())
    }

    async fn require_container_bind(
        &self,
        container_id: &str,
        volume_name: &str,
        target: &str,
        access: &str,
    ) -> RuntimeResult<()> {
        let inspection = self
            .docker_call(
                "inspect named-volume container",
                self.docker.inspect_container(container_id, None),
            )
            .await?;
        let binds = inspection
            .host_config
            .as_ref()
            .and_then(|config| config.binds.as_ref())
            .cloned()
            .unwrap_or_default();
        let expected = format!("{volume_name}:{target}:{access}");
        require(
            binds == vec![expected.clone()],
            format!(
                "Docker container did not preserve one exact named-volume bind {expected:?}: count={}, binds={binds:?}",
                binds.len()
            ),
        )
    }

    async fn require_container_absent(&self, id: &str) -> RuntimeResult<()> {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            self.docker.inspect_container(id, None),
        )
        .await
        .map_err(|_| {
            RuntimeError::ProviderUnavailable(
                "Docker container cleanup inspection timed out".into(),
            )
        })?;
        require(
            matches!(
                result,
                Err(DockerError::DockerResponseServerError {
                    status_code: 404,
                    ..
                })
            ),
            format!("Docker removed mount container {id} is still present"),
        )
    }
}

fn directory_archive(path: &str, bytes: &[u8]) -> Vec<u8> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::file());
    header.set_mode(0o444);
    header.set_size(bytes.len() as u64);
    header.set_cksum();
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_data(&mut header, path, Cursor::new(bytes))
        .expect("append Artifact mount fixture");
    builder.finish().expect("finish Artifact mount archive");
    builder.into_inner().expect("Artifact mount archive")
}
