use super::fixture::{require, resource_id, DockerConformanceFixture};
use super::specs;
use a3s_runtime::contract::{RuntimeMount, RuntimeMountSource};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use bollard::errors::Error as DockerError;
use bollard::volume::RemoveVolumeOptions;
use std::time::Duration;

impl DockerConformanceFixture {
    pub(crate) async fn run_mounts(&self, client: &dyn RuntimeClient) -> RuntimeResult<()> {
        let volume_id = format!("{}-persistent-data", self.namespace);
        let token = format!("mount-token-{}", self.node_id.simple());
        let mut writer = specs::task_spec(
            specs::unit_id(&self.namespace, "mount-volume-writer"),
            &format!("printf '{token}' >/data/value && sync"),
        );
        writer.mounts = vec![RuntimeMount {
            name: "data".into(),
            source: RuntimeMountSource::Volume {
                volume_id: volume_id.clone(),
            },
            target: "/data".into(),
            read_only: false,
        }];
        let writer_observation = client
            .apply(&specs::apply("mount-volume-writer-apply", writer.clone()))
            .await?;
        let writer_id = resource_id(&writer_observation)?.to_owned();
        let writer_inspection = self
            .docker_call(
                "inspect volume writer",
                self.docker.inspect_container(&writer_id, None),
            )
            .await?;
        let writer_binds = writer_inspection
            .host_config
            .as_ref()
            .and_then(|config| config.binds.as_ref())
            .cloned()
            .unwrap_or_default();
        require(
            writer_binds.iter().any(|bind| bind.contains(":/data:rw")),
            "Docker volume writer was not mounted read-write",
        )?;
        client
            .remove(&specs::action("mount-volume-writer-remove", &writer))
            .await?;
        self.require_container_absent(&writer_id).await?;

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
        let reader_inspection = self
            .docker_call(
                "inspect volume reader",
                self.docker.inspect_container(&reader_id, None),
            )
            .await?;
        let reader_binds = reader_inspection
            .host_config
            .as_ref()
            .and_then(|config| config.binds.as_ref())
            .cloned()
            .unwrap_or_default();
        require(
            reader_binds.iter().any(|bind| bind.contains(":/data:ro")),
            "Docker volume reader was not mounted read-only",
        )?;
        client
            .remove(&specs::action("mount-volume-reader-remove", &reader))
            .await?;
        self.require_container_absent(&reader_id).await?;

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

        let volumes = self.namespace_volume_names().await?;
        require(
            volumes.len() == 1,
            format!("Docker mount profile expected one named volume, found {volumes:?}"),
        )?;
        self.docker_call(
            "remove mount profile volume",
            self.docker
                .remove_volume(&volumes[0], Some(RemoveVolumeOptions { force: true })),
        )
        .await?;
        require(
            self.namespace_volume_names().await?.is_empty(),
            "Docker mount profile left a named volume",
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
