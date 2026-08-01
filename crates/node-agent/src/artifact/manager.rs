use super::{LocalArtifactReader, NodeArtifactCache, NodeArtifactError, NodeArtifactTransport};
use crate::ArtifactConfig;
use a3s_cloud_contracts::{
    NodeArtifactDownloadRequest, NodeArtifactUploadRequest, NodeCommandEnvelope,
    NodeCommandPayload, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
#[cfg(target_os = "linux")]
use a3s_cloud_contracts::{NodeBoxBuildPlan, NodeBoxBuildRequest};
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
        let NodeCommandPayload::RuntimeApply { request, .. } = &command.payload else {
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
        let NodeCommandPayload::RuntimeApply { request, .. } = &command.payload else {
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

    pub async fn mount_path(
        &self,
        spec: &RuntimeUnitSpec,
        mount: &RuntimeMount,
    ) -> Result<PathBuf, NodeArtifactError> {
        let spec_digest = spec.digest().map_err(NodeArtifactError::Invalid)?;
        self.cache.mount_path(&spec_digest, mount).await
    }

    pub async fn capture_output(
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

    #[cfg(any(target_os = "linux", test))]
    pub(crate) async fn capture_output_directory(
        &self,
        spec: &RuntimeUnitSpec,
        output: &RuntimeOutputSpec,
        source: &Path,
    ) -> Result<RuntimeOutputArtifact, NodeArtifactError> {
        validate_output_spec(output)?;
        let spec_digest = spec.digest().map_err(NodeArtifactError::Invalid)?;
        self.cache
            .capture_output_directory(&spec_digest, output, source)
            .await
    }

    pub async fn cleanup_spec(&self, spec_digest: &str) -> Result<(), NodeArtifactError> {
        self.cache.cleanup_spec(spec_digest).await
    }

    #[cfg(target_os = "linux")]
    pub(crate) async fn materialize_box_build_source(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
    ) -> Result<PathBuf, NodeArtifactError> {
        self.validate_box_build_command(command, request, BoxBuildArtifactAction::Download)?;
        let binding_digest = request
            .binding_digest()
            .map_err(NodeArtifactError::Invalid)?;
        let transfer = NodeArtifactDownloadRequest::new(
            self.node_id,
            command.command_id,
            binding_digest,
            "build-source",
            &request.source,
        )
        .map_err(NodeArtifactError::Invalid)?;
        self.cache
            .materialize(self.transport.as_ref(), &transfer)
            .await
    }

    #[cfg(target_os = "linux")]
    pub(crate) async fn materialize_box_build_cache(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
        plan: &NodeBoxBuildPlan,
    ) -> Result<Option<PathBuf>, NodeArtifactError> {
        self.validate_box_build_command(command, request, BoxBuildArtifactAction::Download)?;
        let Some(cache) = &plan.cache else {
            return Ok(None);
        };
        let binding_digest = request
            .binding_digest()
            .map_err(NodeArtifactError::Invalid)?;
        let transfer = NodeArtifactDownloadRequest::new(
            self.node_id,
            command.command_id,
            binding_digest,
            plan.cache_output_name(),
            &cache.artifact,
        )
        .map_err(NodeArtifactError::Invalid)?;
        self.cache
            .materialize(self.transport.as_ref(), &transfer)
            .await
            .map(Some)
    }

    #[cfg(target_os = "linux")]
    pub(crate) async fn publish_box_build_directory(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
        output_name: &str,
        maximum_bytes: u64,
        source: &Path,
    ) -> Result<RuntimeOutputArtifact, NodeArtifactError> {
        self.validate_box_build_command(command, request, BoxBuildArtifactAction::Upload)?;
        let binding_digest = request
            .binding_digest()
            .map_err(NodeArtifactError::Invalid)?;
        let output = RuntimeOutputSpec {
            name: output_name.to_owned(),
            path: format!("/a3s-box-build/{output_name}"),
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            max_bytes: maximum_bytes,
        };
        validate_output_spec(&output)?;
        let captured = self
            .cache
            .capture_output_directory(&binding_digest, &output, source)
            .await?;
        let blob = self.cache.output_blob(&binding_digest, &captured).await?;
        let transfer = NodeArtifactUploadRequest::new(
            self.node_id,
            command.command_id,
            binding_digest,
            captured.name.clone(),
            captured.artifact.digest.clone(),
            captured.artifact.media_type.clone(),
            captured.size_bytes,
        )
        .map_err(NodeArtifactError::Invalid)?;
        let receipt = self.transport.upload(&transfer, &blob).await?;
        receipt
            .validate_against(&transfer)
            .map_err(NodeArtifactError::Integrity)?;
        Ok(receipt.artifact)
    }

    #[cfg(target_os = "linux")]
    pub(crate) async fn cleanup_box_build(
        &self,
        request: &NodeBoxBuildRequest,
    ) -> Result<(), NodeArtifactError> {
        let binding_digest = request
            .binding_digest()
            .map_err(NodeArtifactError::Invalid)?;
        self.cache.cleanup_spec(&binding_digest).await
    }

    #[cfg(target_os = "linux")]
    fn validate_box_build_command(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
        action: BoxBuildArtifactAction,
    ) -> Result<(), NodeArtifactError> {
        command.validate().map_err(NodeArtifactError::Invalid)?;
        if command.node_id != self.node_id {
            return Err(NodeArtifactError::Invalid(
                "Box build Artifact command belongs to a different node".into(),
            ));
        }
        let admitted = match (&command.payload, action) {
            (NodeCommandPayload::BoxBuildStart { request }, BoxBuildArtifactAction::Download)
            | (NodeCommandPayload::BoxBuildInspect { request }, BoxBuildArtifactAction::Upload) => {
                request.as_ref()
            }
            _ => {
                return Err(NodeArtifactError::Invalid(
                    "Box build Artifact action does not match its node command".into(),
                ))
            }
        };
        if admitted != request {
            return Err(NodeArtifactError::Invalid(
                "Box build Artifact request changed after command admission".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg(target_os = "linux")]
enum BoxBuildArtifactAction {
    Download,
    Upload,
}

fn validate_output_spec(output: &RuntimeOutputSpec) -> Result<(), NodeArtifactError> {
    if output.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE {
        return Err(NodeArtifactError::Invalid(
            "Runtime Task outputs require the supported directory archive media type".into(),
        ));
    }
    Ok(())
}
