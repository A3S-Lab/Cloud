use super::{LocalArtifactReader, NodeArtifactCache, NodeArtifactError, NodeArtifactTransport};
use crate::ArtifactConfig;
use a3s_cloud_contracts::{
    NodeArtifactDownloadRequest, NodeArtifactUploadRequest, NodeCommandEnvelope,
    NodeCommandPayload, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{
    RuntimeMount, RuntimeMountSource, RuntimeObservation, RuntimeOutputArtifact, RuntimeOutputSpec,
    RuntimeUnitSpec, RuntimeUnitState,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub struct NodeArtifactManager {
    node_id: Uuid,
    transport: Arc<dyn NodeArtifactTransport>,
    cache: NodeArtifactCache,
}

impl NodeArtifactManager {
    pub fn new(
        state_dir: impl AsRef<Path>,
        config: ArtifactConfig,
        node_id: Uuid,
        transport: Arc<dyn NodeArtifactTransport>,
    ) -> Result<Self, String> {
        if node_id.is_nil() {
            return Err("node artifact manager requires a non-nil node ID".into());
        }
        let cache = NodeArtifactCache::new(state_dir.as_ref().join("artifacts"), config)?;
        Ok(Self {
            node_id,
            transport,
            cache,
        })
    }

    pub async fn prepare_command(
        &self,
        command: &NodeCommandEnvelope,
    ) -> Result<(), NodeArtifactError> {
        command.validate().map_err(NodeArtifactError::Invalid)?;
        if command.node_id != self.node_id {
            return Err(NodeArtifactError::Invalid(
                "artifact command belongs to a different node".into(),
            ));
        }
        let NodeCommandPayload::RuntimeApply { request } = &command.payload else {
            return Ok(());
        };
        let spec_digest = request.spec.digest().map_err(NodeArtifactError::Invalid)?;
        for output in &request.spec.outputs {
            validate_output_spec(output)?;
        }
        for mount in &request.spec.mounts {
            let RuntimeMountSource::Artifact { artifact } = &mount.source else {
                continue;
            };
            if !mount.read_only {
                return Err(NodeArtifactError::Invalid(
                    "artifact mounts must be read-only".into(),
                ));
            }
            let transfer = NodeArtifactDownloadRequest::new(
                self.node_id,
                command.command_id,
                spec_digest.clone(),
                mount.name.clone(),
                artifact,
            )
            .map_err(NodeArtifactError::Invalid)?;
            self.cache
                .materialize(self.transport.as_ref(), &transfer)
                .await?;
        }
        Ok(())
    }

    pub async fn publish_command_outputs(
        &self,
        command: &NodeCommandEnvelope,
        observation: &RuntimeObservation,
    ) -> Result<RuntimeObservation, NodeArtifactError> {
        command.validate().map_err(NodeArtifactError::Invalid)?;
        if command.node_id != self.node_id {
            return Err(NodeArtifactError::Invalid(
                "artifact command belongs to a different node".into(),
            ));
        }
        let NodeCommandPayload::RuntimeApply { request } = &command.payload else {
            return Ok(observation.clone());
        };
        observation
            .validate_against(&request.spec)
            .map_err(NodeArtifactError::Integrity)?;
        if observation.state != RuntimeUnitState::Succeeded || observation.outputs.is_empty() {
            return Ok(observation.clone());
        }
        let spec_digest = request.spec.digest().map_err(NodeArtifactError::Invalid)?;
        let mut published = Vec::with_capacity(observation.outputs.len());
        for output in &observation.outputs {
            let source = self.cache.output_blob(&spec_digest, output).await?;
            let transfer = NodeArtifactUploadRequest::new(
                self.node_id,
                command.command_id,
                spec_digest.clone(),
                output.name.clone(),
                output.artifact.digest.clone(),
                output.artifact.media_type.clone(),
                output.size_bytes,
            )
            .map_err(NodeArtifactError::Invalid)?;
            let receipt = self.transport.upload(&transfer, &source).await?;
            receipt
                .validate_against(&transfer)
                .map_err(NodeArtifactError::Integrity)?;
            published.push(receipt.artifact);
        }
        let mut observation = observation.clone();
        observation.outputs = published;
        observation
            .validate_against(&request.spec)
            .map_err(NodeArtifactError::Integrity)?;
        Ok(observation)
    }

    pub(crate) async fn mount_path(
        &self,
        spec: &RuntimeUnitSpec,
        mount: &RuntimeMount,
    ) -> Result<PathBuf, NodeArtifactError> {
        let spec_digest = spec.digest().map_err(NodeArtifactError::Invalid)?;
        self.cache.mount_path(&spec_digest, mount).await
    }

    pub(crate) async fn capture_output(
        &self,
        spec: &RuntimeUnitSpec,
        output: &RuntimeOutputSpec,
        reader: LocalArtifactReader,
    ) -> Result<RuntimeOutputArtifact, NodeArtifactError> {
        validate_output_spec(output)?;
        let spec_digest = spec.digest().map_err(NodeArtifactError::Invalid)?;
        self.cache
            .capture_output(&spec_digest, output, reader)
            .await
    }

    pub(crate) async fn cleanup_spec(&self, spec_digest: &str) -> Result<(), NodeArtifactError> {
        self.cache.cleanup_spec(spec_digest).await
    }
}

fn validate_output_spec(output: &RuntimeOutputSpec) -> Result<(), NodeArtifactError> {
    if output.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE {
        return Err(NodeArtifactError::Invalid(
            "Docker Task outputs require the supported directory archive media type".into(),
        ));
    }
    Ok(())
}
