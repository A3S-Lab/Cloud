use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{
    NodeArtifactDownloadRequest, NodeArtifactUploadRequest, NodeBoxBuildRequest,
    NodeCommandPayload, BOX_BUILD_OUTPUT_NAME, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
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
        let artifact = match request.artifact() {
            Ok(artifact) => artifact,
            Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
        };
        let authorized = match &command.payload {
            NodeCommandPayload::RuntimeApply { request: apply, .. } => {
                apply.spec.mounts.iter().any(|mount| {
                    mount.name == request.mount_name
                        && mount.read_only
                        && matches!(
                            &mount.source,
                            RuntimeMountSource::Artifact { artifact: expected }
                                if expected == &artifact
                        )
                })
            }
            NodeCommandPayload::BoxBuildStart { request: build } => {
                box_build_download_authorized(build, request, &artifact)
            }
            _ => false,
        };
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
        let authorized = match &command.payload {
            NodeCommandPayload::RuntimeApply { request: apply, .. } => {
                apply.spec.outputs.iter().any(|output| {
                    output.name == request.output_name
                        && output.media_type == request.media_type
                        && request.size_bytes <= output.max_bytes
                })
            }
            NodeCommandPayload::BoxBuildInspect { request: build } => {
                box_build_upload_authorized(build, request)
            }
            _ => false,
        };
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
        binding_digest: &str,
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
        let actual = artifact_binding_digest(&command.payload).map_err(RepositoryError::Storage)?;
        if actual.as_deref() != Some(binding_digest) {
            return Ok(Err(not_authorized()));
        }
        Ok(Ok(command))
    }
}

fn artifact_binding_digest(payload: &NodeCommandPayload) -> Result<Option<String>, String> {
    match payload {
        NodeCommandPayload::RuntimeApply { request, .. } => request.spec.digest().map(Some),
        NodeCommandPayload::BoxBuildStart { request }
        | NodeCommandPayload::BoxBuildInspect { request } => request.binding_digest().map(Some),
        NodeCommandPayload::ResourceClaimPrepare { .. }
        | NodeCommandPayload::RuntimeInspect { .. }
        | NodeCommandPayload::RuntimeStop { .. }
        | NodeCommandPayload::RuntimeRemove { .. }
        | NodeCommandPayload::BoxBuildCancel { .. }
        | NodeCommandPayload::BoxBuildRemove { .. }
        | NodeCommandPayload::ResourceClaimRelease { .. }
        | NodeCommandPayload::GatewaySnapshotInstall { .. }
        | NodeCommandPayload::GatewaySnapshotObserve { .. } => Ok(None),
    }
}

fn box_build_download_authorized(
    build: &NodeBoxBuildRequest,
    request: &NodeArtifactDownloadRequest,
    artifact: &ArtifactRef,
) -> bool {
    (request.mount_name == "build-source" && artifact == &build.source)
        || build.plans.iter().any(|plan| {
            plan.cache.as_ref().is_some_and(|cache| {
                request.mount_name == plan.cache_output_name() && artifact == &cache.artifact
            })
        })
}

fn box_build_upload_authorized(
    build: &NodeBoxBuildRequest,
    request: &NodeArtifactUploadRequest,
) -> bool {
    if request.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE {
        return false;
    }
    (request.output_name == BOX_BUILD_OUTPUT_NAME && request.size_bytes <= build.output_max_bytes)
        || (request.size_bytes <= build.cache_max_bytes
            && build
                .plans
                .iter()
                .any(|plan| request.output_name == plan.cache_output_name()))
}

fn not_authorized() -> ApplicationError {
    ApplicationError::Forbidden("artifact transfer is not authorized for this node command".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        artifact_uri, NodeBoxBuildCacheInput, NodeBoxBuildCacheReceipt, NodeBoxBuildDescriptor,
        NodeBoxBuildPlan, NodeBoxBuildPlatform,
    };
    use sha2::{Digest, Sha256};

    fn digest(fill: char) -> String {
        format!("sha256:{}", fill.to_string().repeat(64))
    }

    fn artifact(fill: char) -> ArtifactRef {
        let digest = digest(fill);
        ArtifactRef {
            uri: artifact_uri(&digest).expect("Artifact URI"),
            digest,
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
        }
    }

    fn box_build_request() -> NodeBoxBuildRequest {
        let source = artifact('a');
        let plan_acl = concat!(
            "build \"oci\" {\n",
            "  cache = \"content-addressed\"\n",
            "  context = \".\"\n",
            "  file = \"Dockerfile\"\n",
            "  network = \"none\"\n",
            "  platform = \"linux/amd64\"\n",
            "  schema = \"a3s.box.build-plan.v1\"\n",
            "}\n",
        )
        .to_owned();
        let plan_digest = format!("sha256:{:x}", Sha256::digest(plan_acl.as_bytes()));
        NodeBoxBuildRequest {
            schema: NodeBoxBuildRequest::SCHEMA.into(),
            generation: 1,
            source: source.clone(),
            plans: vec![NodeBoxBuildPlan {
                operation_id: "build-1-linux-amd64".into(),
                plan_acl,
                cache: Some(NodeBoxBuildCacheInput {
                    artifact: artifact('b'),
                    receipt: NodeBoxBuildCacheReceipt {
                        schema: NodeBoxBuildCacheReceipt::SCHEMA.into(),
                        key: digest('c'),
                        source_digest: source.digest,
                        plan_digest,
                        descriptor: NodeBoxBuildDescriptor {
                            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                            digest: digest('e'),
                            size: 512,
                        },
                        platform: NodeBoxBuildPlatform {
                            os: "linux".into(),
                            architecture: "amd64".into(),
                            variant: None,
                        },
                        content_bytes: 1024,
                        entry_count: 1,
                        blob_count: 2,
                        blob_inventory_digest: digest('f'),
                    },
                }),
            }],
            assembly_reference: None,
            output_max_bytes: 4096,
            cache_max_bytes: 2048,
        }
    }

    #[test]
    fn box_build_downloads_are_closed_to_the_admitted_source_and_cache() {
        let build = box_build_request();
        build.validate().expect("Box build request");
        let binding_digest = build.binding_digest().expect("binding digest");
        let command_id = uuid::Uuid::now_v7();
        let node_id = uuid::Uuid::now_v7();
        let source = NodeArtifactDownloadRequest::new(
            node_id,
            command_id,
            &binding_digest,
            "build-source",
            &build.source,
        )
        .expect("source download");
        assert!(box_build_download_authorized(
            &build,
            &source,
            &source.artifact().expect("source Artifact")
        ));

        let plan = &build.plans[0];
        let cache = plan.cache.as_ref().expect("cache input");
        let cache_download = NodeArtifactDownloadRequest::new(
            node_id,
            command_id,
            &binding_digest,
            plan.cache_output_name(),
            &cache.artifact,
        )
        .expect("cache download");
        assert!(box_build_download_authorized(
            &build,
            &cache_download,
            &cache_download.artifact().expect("cache Artifact")
        ));

        let mut changed_name = source.clone();
        changed_name.mount_name = "source".into();
        assert!(!box_build_download_authorized(
            &build,
            &changed_name,
            &changed_name.artifact().expect("changed source Artifact")
        ));
        assert!(!box_build_download_authorized(
            &build,
            &source,
            &artifact('1')
        ));
    }

    #[test]
    fn box_build_uploads_are_closed_to_the_inspection_outputs_and_bounds() {
        let build = box_build_request();
        let binding_digest = build.binding_digest().expect("binding digest");
        let node_id = uuid::Uuid::now_v7();
        let command_id = uuid::Uuid::now_v7();
        let output = NodeArtifactUploadRequest::new(
            node_id,
            command_id,
            &binding_digest,
            BOX_BUILD_OUTPUT_NAME,
            digest('1'),
            NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
            build.output_max_bytes,
        )
        .expect("output upload");
        assert!(box_build_upload_authorized(&build, &output));

        let cache = NodeArtifactUploadRequest::new(
            node_id,
            command_id,
            &binding_digest,
            build.plans[0].cache_output_name(),
            digest('2'),
            NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
            build.cache_max_bytes,
        )
        .expect("cache upload");
        assert!(box_build_upload_authorized(&build, &cache));

        let mut oversized = output.clone();
        oversized.size_bytes += 1;
        assert!(!box_build_upload_authorized(&build, &oversized));
        let mut unknown = cache;
        unknown.output_name = "build-cache-unadmitted".into();
        assert!(!box_build_upload_authorized(&build, &unknown));
    }

    #[test]
    fn artifact_binding_has_one_digest_source_and_no_cleanup_transfer_path() {
        let build = box_build_request();
        let expected = build.binding_digest().expect("binding digest");
        let start = NodeCommandPayload::BoxBuildStart {
            request: Box::new(build.clone()),
        };
        let inspect = NodeCommandPayload::BoxBuildInspect {
            request: Box::new(build.clone()),
        };
        let cancel = NodeCommandPayload::BoxBuildCancel {
            request: Box::new(build),
        };
        assert_eq!(
            artifact_binding_digest(&start).expect("start binding"),
            Some(expected.clone())
        );
        assert_eq!(
            artifact_binding_digest(&inspect).expect("inspect binding"),
            Some(expected)
        );
        assert_eq!(
            artifact_binding_digest(&cancel).expect("cancel binding"),
            None
        );
    }
}
