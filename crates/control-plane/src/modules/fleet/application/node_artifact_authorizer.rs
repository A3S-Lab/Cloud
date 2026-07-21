use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{NodeArtifactDownloadRequest, NodeArtifactUploadRequest};
use a3s_runtime::contract::{ArtifactRef, RuntimeMountSource};
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub struct NodeArtifactAuthorizer {
    commands: Arc<dyn INodeControlRepository>,
}

impl NodeArtifactAuthorizer {
    pub fn new(commands: Arc<dyn INodeControlRepository>) -> Self {
        Self { commands }
    }

    pub async fn authorize_download(
        &self,
        authenticated_node_id: NodeId,
        request: &NodeArtifactDownloadRequest,
        transferred_at: DateTime<Utc>,
    ) -> Result<ApplicationResult<ArtifactRef>, RepositoryError> {
        if let Err(error) = request.validate() {
            return Ok(Err(ApplicationError::Invalid(error)));
        }
        let command = match self
            .authorized_command(
                authenticated_node_id,
                request.node_id,
                request.command_id,
                &request.spec_digest,
                transferred_at,
            )
            .await?
        {
            Ok(command) => command,
            Err(error) => return Ok(Err(error)),
        };
        let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request: apply } =
            &command.payload
        else {
            return Ok(Err(not_authorized()));
        };
        let artifact = match request.artifact() {
            Ok(artifact) => artifact,
            Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
        };
        let authorized = apply.spec.mounts.iter().any(|mount| {
            mount.name == request.mount_name
                && mount.read_only
                && matches!(
                    &mount.source,
                    RuntimeMountSource::Artifact { artifact: expected } if expected == &artifact
                )
        });
        if !authorized {
            return Ok(Err(not_authorized()));
        }
        Ok(Ok(artifact))
    }

    pub async fn authorize_upload(
        &self,
        authenticated_node_id: NodeId,
        request: &NodeArtifactUploadRequest,
        transferred_at: DateTime<Utc>,
    ) -> Result<ApplicationResult<()>, RepositoryError> {
        if let Err(error) = request.validate() {
            return Ok(Err(ApplicationError::Invalid(error)));
        }
        let command = match self
            .authorized_command(
                authenticated_node_id,
                request.node_id,
                request.command_id,
                &request.spec_digest,
                transferred_at,
            )
            .await?
        {
            Ok(command) => command,
            Err(error) => return Ok(Err(error)),
        };
        let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request: apply } =
            &command.payload
        else {
            return Ok(Err(not_authorized()));
        };
        let authorized = apply.spec.outputs.iter().any(|output| {
            output.name == request.output_name
                && output.media_type == request.media_type
                && request.size_bytes <= output.max_bytes
        });
        if !authorized {
            return Ok(Err(not_authorized()));
        }
        Ok(Ok(()))
    }

    async fn authorized_command(
        &self,
        authenticated_node_id: NodeId,
        requested_node_id: uuid::Uuid,
        command_id: uuid::Uuid,
        spec_digest: &str,
        transferred_at: DateTime<Utc>,
    ) -> Result<
        ApplicationResult<crate::modules::fleet::domain::entities::NodeCommand>,
        RepositoryError,
    > {
        if authenticated_node_id.as_uuid() != requested_node_id {
            return Ok(Err(not_authorized()));
        }
        let Some(command) = self
            .commands
            .find_command(authenticated_node_id, NodeCommandId::from_uuid(command_id))
            .await?
        else {
            return Ok(Err(not_authorized()));
        };
        if command.node_id != authenticated_node_id || transferred_at >= command.not_after {
            return Ok(Err(not_authorized()));
        }
        let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request } = &command.payload
        else {
            return Ok(Err(not_authorized()));
        };
        let actual = request.spec.digest().map_err(RepositoryError::Storage)?;
        if actual != spec_digest {
            return Ok(Err(not_authorized()));
        }
        Ok(Ok(command))
    }
}

fn not_authorized() -> ApplicationError {
    ApplicationError::Forbidden("artifact transfer is not authorized for this node command".into())
}
