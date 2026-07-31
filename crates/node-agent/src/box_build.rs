use a3s_cloud_contracts::{
    NodeBoxBuildCancelResult, NodeBoxBuildInspection, NodeBoxBuildRemoveResult,
    NodeBoxBuildRequest, NodeBoxBuildStartResult, NodeCommandEnvelope,
};
use async_trait::async_trait;

#[async_trait]
pub(crate) trait NodeBoxBuildExecutor: Send + Sync {
    async fn start(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
    ) -> Result<NodeBoxBuildStartResult, NodeBoxBuildError>;

    async fn inspect(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
    ) -> Result<NodeBoxBuildInspection, NodeBoxBuildError>;

    async fn cancel(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
    ) -> Result<NodeBoxBuildCancelResult, NodeBoxBuildError>;

    async fn remove(
        &self,
        command: &NodeCommandEnvelope,
        request: &NodeBoxBuildRequest,
    ) -> Result<NodeBoxBuildRemoveResult, NodeBoxBuildError>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum NodeBoxBuildError {
    #[cfg(target_os = "linux")]
    #[error("Box build request is invalid: {0}")]
    Invalid(String),
    #[error(transparent)]
    Artifact(#[from] crate::NodeArtifactError),
    #[cfg(target_os = "linux")]
    #[error("Box native build failed: {0}")]
    Native(String),
    #[error("Box native build state is inconsistent: {0}")]
    State(String),
}

impl NodeBoxBuildError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            #[cfg(target_os = "linux")]
            Self::Invalid(_) => "invalid_box_build",
            Self::Artifact(_) => "box_build_artifact",
            #[cfg(target_os = "linux")]
            Self::Native(_) => "box_build_native",
            Self::State(_) => "box_build_state",
        }
    }

    pub(crate) fn retryable(&self) -> bool {
        match self {
            #[cfg(target_os = "linux")]
            Self::Invalid(_) => false,
            Self::State(_) => false,
            Self::Artifact(error) => error.retryable(),
            #[cfg(target_os = "linux")]
            Self::Native(_) => true,
        }
    }
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use crate::{BoxRuntimeConfig, NodeArtifactManager};
    use a3s_box_core::{platform::Platform, OperationId};
    use a3s_box_runtime::{
        assemble_recorded_build_outputs, cancel_recorded_build_plan, hydrate_recorded_build_cache,
        inspect_recorded_build_status, remove_recorded_build_plan, start_recorded_build_plan,
        BoxBuildPlan, BuildCachePolicy, BuildCacheReceipt, BuildCancellationOutcome,
        BuildOperationIdentity, BuildOutputAssembly, BuildOutputAssemblyInput,
        BuildOutputDescriptor, ImageStore, RecordedBuildCache, RecordedBuildResult,
        RecordedBuildStatus, DEFAULT_IMAGE_CACHE_SIZE,
    };
    use a3s_cloud_contracts::{
        NodeBoxBuildCacheOutput, NodeBoxBuildCacheReceipt, NodeBoxBuildCancellation,
        NodeBoxBuildDescriptor, NodeBoxBuildOperationCancellation, NodeBoxBuildOperationRemoval,
        NodeBoxBuildOutput, NodeBoxBuildPhase, NodeBoxBuildPlan, NodeBoxBuildPlatform,
        BOX_BUILD_OUTPUT_NAME,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    pub(crate) struct BoxBuildCommandExecutor {
        store: Arc<ImageStore>,
        artifacts: Arc<NodeArtifactManager>,
    }

    impl BoxBuildCommandExecutor {
        pub(crate) fn new(
            config: &BoxRuntimeConfig,
            artifacts: Arc<NodeArtifactManager>,
        ) -> Result<Self, NodeBoxBuildError> {
            let process_home = a3s_box_core::dirs_home();
            if process_home != config.home_dir {
                return Err(NodeBoxBuildError::Invalid(format!(
                    "box.home_dir {} must equal the process A3S_HOME {} so Runtime, ImageStore, and BuildCache share one authority",
                    config.home_dir.display(),
                    process_home.display()
                )));
            }
            let store = ImageStore::new(&config.home_dir.join("images"), DEFAULT_IMAGE_CACHE_SIZE)
                .map_err(native_error)?;
            Ok(Self {
                store: Arc::new(store),
                artifacts,
            })
        }

        fn prepare_plans<'a>(
            &self,
            request: &'a NodeBoxBuildRequest,
        ) -> Result<Vec<PreparedPlan<'a>>, NodeBoxBuildError> {
            request.validate().map_err(NodeBoxBuildError::Invalid)?;
            request
                .plans
                .iter()
                .map(|wire| PreparedPlan::new(wire, &request.source.digest))
                .collect()
        }

        async fn hydrate_caches(
            &self,
            command: &NodeCommandEnvelope,
            request: &NodeBoxBuildRequest,
            plans: &[PreparedPlan<'_>],
        ) -> Result<PathBuf, NodeBoxBuildError> {
            let source = self
                .artifacts
                .materialize_box_build_source(command, request)
                .await?;
            for (prepared, wire) in plans.iter().zip(&request.plans) {
                let Some(cache) = &wire.cache else {
                    continue;
                };
                let root = self
                    .artifacts
                    .materialize_box_build_cache(command, request, wire)
                    .await?
                    .ok_or_else(|| {
                        NodeBoxBuildError::State(format!(
                            "cache Artifact for operation {} disappeared after admission",
                            wire.operation_id
                        ))
                    })?;
                prepared.validate_cache(cache)?;
                let recorded = RecordedBuildCache {
                    receipt: box_cache_receipt(&cache.receipt),
                    layout_directory: root,
                };
                hydrate_recorded_build_cache(&recorded)
                    .await
                    .map_err(native_error)?;
            }
            Ok(source)
        }
    }

    #[async_trait]
    impl NodeBoxBuildExecutor for BoxBuildCommandExecutor {
        async fn start(
            &self,
            command: &NodeCommandEnvelope,
            request: &NodeBoxBuildRequest,
        ) -> Result<NodeBoxBuildStartResult, NodeBoxBuildError> {
            let plans = self.prepare_plans(request)?;
            let source = self.hydrate_caches(command, request, &plans).await?;
            let mut statuses = Vec::with_capacity(plans.len());
            for prepared in &plans {
                statuses.push(
                    start_recorded_build_plan(
                        &prepared.identity,
                        &prepared.plan,
                        &source,
                        true,
                        Arc::clone(&self.store),
                    )
                    .await
                    .map_err(native_error)?,
                );
            }
            let result = NodeBoxBuildStartResult {
                binding_digest: request
                    .binding_digest()
                    .map_err(NodeBoxBuildError::Invalid)?,
                phase: aggregate_statuses(statuses),
            };
            result
                .validate_for(request)
                .map_err(NodeBoxBuildError::State)?;
            Ok(result)
        }

        async fn inspect(
            &self,
            command: &NodeCommandEnvelope,
            request: &NodeBoxBuildRequest,
        ) -> Result<NodeBoxBuildInspection, NodeBoxBuildError> {
            let plans = self.prepare_plans(request)?;
            let binding_digest = request
                .binding_digest()
                .map_err(NodeBoxBuildError::Invalid)?;
            let mut terminal = Vec::with_capacity(plans.len());
            let mut non_success = Vec::new();
            for prepared in &plans {
                match inspect_recorded_build_status(&prepared.identity, &prepared.plan, &self.store)
                    .await
                    .map_err(native_error)?
                {
                    Some(RecordedBuildStatus::Succeeded(result)) => terminal.push(*result),
                    Some(status) => non_success.push(status),
                    None => {
                        return Err(NodeBoxBuildError::State(format!(
                            "operation {} has no Box build journal record",
                            prepared.wire.operation_id
                        )))
                    }
                }
            }
            let inspection = if non_success.is_empty() {
                let output = self
                    .publish_success(command, request, &plans, terminal)
                    .await?;
                NodeBoxBuildInspection::Succeeded {
                    binding_digest,
                    output: Box::new(output),
                }
            } else {
                match aggregate_statuses(non_success) {
                    NodeBoxBuildPhase::Running | NodeBoxBuildPhase::Succeeded => {
                        NodeBoxBuildInspection::Running { binding_digest }
                    }
                    NodeBoxBuildPhase::Cancelling => {
                        NodeBoxBuildInspection::Cancelling { binding_digest }
                    }
                    NodeBoxBuildPhase::Cancelled { message } => NodeBoxBuildInspection::Cancelled {
                        binding_digest,
                        message,
                    },
                    NodeBoxBuildPhase::Failed { message } => NodeBoxBuildInspection::Failed {
                        binding_digest,
                        message,
                    },
                }
            };
            inspection
                .validate_for(request)
                .map_err(NodeBoxBuildError::State)?;
            Ok(inspection)
        }

        async fn cancel(
            &self,
            _command: &NodeCommandEnvelope,
            request: &NodeBoxBuildRequest,
        ) -> Result<NodeBoxBuildCancelResult, NodeBoxBuildError> {
            let plans = self.prepare_plans(request)?;
            let mut operations = Vec::with_capacity(plans.len());
            for prepared in &plans {
                let outcome =
                    cancel_recorded_build_plan(&prepared.identity, &prepared.plan, &self.store)
                        .await
                        .map_err(native_error)?;
                operations.push(NodeBoxBuildOperationCancellation {
                    operation_id: prepared.wire.operation_id.clone(),
                    outcome: cloud_cancellation(outcome),
                });
            }
            let result = NodeBoxBuildCancelResult {
                binding_digest: request
                    .binding_digest()
                    .map_err(NodeBoxBuildError::Invalid)?,
                operations,
            };
            result
                .validate_for(request)
                .map_err(NodeBoxBuildError::State)?;
            Ok(result)
        }

        async fn remove(
            &self,
            _command: &NodeCommandEnvelope,
            request: &NodeBoxBuildRequest,
        ) -> Result<NodeBoxBuildRemoveResult, NodeBoxBuildError> {
            let plans = self.prepare_plans(request)?;
            let assembly_removed = match &request.assembly_reference {
                Some(reference) if self.store.get(reference).await.is_some() => {
                    self.store.remove(reference).await.map_err(native_error)?;
                    true
                }
                Some(_) | None => false,
            };
            let mut operations = Vec::with_capacity(plans.len());
            for prepared in &plans {
                let removed =
                    remove_recorded_build_plan(&prepared.identity, &prepared.plan, &self.store)
                        .await
                        .map_err(native_error)?;
                operations.push(NodeBoxBuildOperationRemoval {
                    operation_id: prepared.wire.operation_id.clone(),
                    removed,
                });
            }
            self.artifacts.cleanup_box_build(request).await?;
            let result = NodeBoxBuildRemoveResult {
                binding_digest: request
                    .binding_digest()
                    .map_err(NodeBoxBuildError::Invalid)?,
                operations,
                assembly_removed,
            };
            result
                .validate_for(request)
                .map_err(NodeBoxBuildError::State)?;
            Ok(result)
        }
    }

    impl BoxBuildCommandExecutor {
        async fn publish_success(
            &self,
            command: &NodeCommandEnvelope,
            request: &NodeBoxBuildRequest,
            plans: &[PreparedPlan<'_>],
            results: Vec<RecordedBuildResult>,
        ) -> Result<NodeBoxBuildOutput, NodeBoxBuildError> {
            if results.len() != plans.len() {
                return Err(NodeBoxBuildError::State(
                    "successful Box build result count changed during inspection".into(),
                ));
            }
            let final_output = if results.len() == 1 {
                let output = &results[0].output;
                PublishedOutput {
                    directory: output.layout_directory.clone(),
                    descriptor: cloud_descriptor(&output.descriptor),
                    platforms: vec![cloud_platform(&output.platform)],
                    manifest_count: 1,
                    content_bytes: output.content_bytes(),
                    blob_count: checked_count(output.blob_count, "output blob")?,
                    blob_inventory_digest: output.blob_inventory_digest.clone(),
                }
            } else {
                let inputs = plans
                    .iter()
                    .zip(&results)
                    .map(|(prepared, result)| {
                        BuildOutputAssemblyInput::new(prepared.plan.clone(), result.receipt.clone())
                    })
                    .collect();
                let assembly = BuildOutputAssembly::new(
                    request.assembly_reference.clone().ok_or_else(|| {
                        NodeBoxBuildError::State(
                            "multi-platform build omitted its assembly reference".into(),
                        )
                    })?,
                    request.source.digest.clone(),
                    inputs,
                )
                .map_err(native_error)?;
                let output = assemble_recorded_build_outputs(&assembly, Arc::clone(&self.store))
                    .await
                    .map_err(native_error)?;
                let content_bytes = output.content_bytes();
                PublishedOutput {
                    directory: output.layout_directory,
                    descriptor: cloud_descriptor(&output.descriptor),
                    platforms: output.platforms.iter().map(cloud_platform).collect(),
                    manifest_count: checked_count(output.manifest_count, "output manifest")?,
                    content_bytes,
                    blob_count: checked_count(output.blob_count, "output blob")?,
                    blob_inventory_digest: output.blob_inventory_digest,
                }
            };
            let artifact = self
                .artifacts
                .publish_box_build_directory(
                    command,
                    request,
                    BOX_BUILD_OUTPUT_NAME,
                    request.output_max_bytes,
                    &final_output.directory,
                )
                .await?;
            let mut caches = Vec::with_capacity(results.len());
            for ((prepared, result), wire) in plans.iter().zip(results).zip(&request.plans) {
                let cache = result.cache.ok_or_else(|| {
                    NodeBoxBuildError::State(format!(
                        "operation {} omitted its required Box-native cache",
                        prepared.wire.operation_id
                    ))
                })?;
                let cache_artifact = self
                    .artifacts
                    .publish_box_build_directory(
                        command,
                        request,
                        &wire.cache_output_name(),
                        request.cache_max_bytes,
                        &cache.layout_directory,
                    )
                    .await?;
                caches.push(NodeBoxBuildCacheOutput {
                    operation_id: prepared.wire.operation_id.clone(),
                    artifact: cache_artifact,
                    receipt: cloud_cache_receipt(&cache.receipt),
                });
            }
            Ok(NodeBoxBuildOutput {
                artifact,
                descriptor: final_output.descriptor,
                platforms: final_output.platforms,
                manifest_count: final_output.manifest_count,
                content_bytes: final_output.content_bytes,
                blob_count: final_output.blob_count,
                blob_inventory_digest: final_output.blob_inventory_digest,
                caches,
            })
        }
    }

    struct PreparedPlan<'a> {
        wire: &'a NodeBoxBuildPlan,
        plan: BoxBuildPlan,
        identity: BuildOperationIdentity,
        plan_digest: String,
    }

    impl<'a> PreparedPlan<'a> {
        fn new(wire: &'a NodeBoxBuildPlan, source_digest: &str) -> Result<Self, NodeBoxBuildError> {
            let plan = BoxBuildPlan::parse_acl(&wire.plan_acl).map_err(invalid_error)?;
            let canonical_acl = plan.canonical_acl().map_err(invalid_error)?;
            if canonical_acl != wire.plan_acl {
                return Err(NodeBoxBuildError::Invalid(format!(
                    "operation {} did not carry canonical a3s.box.build-plan.v1 ACL",
                    wire.operation_id
                )));
            }
            if plan.cache() != BuildCachePolicy::ContentAddressed {
                return Err(NodeBoxBuildError::Invalid(format!(
                    "operation {} disabled the sole Box-native cache contract",
                    wire.operation_id
                )));
            }
            let plan_digest = plan.canonical_digest().map_err(invalid_error)?;
            let operation_id =
                OperationId::new(wire.operation_id.clone()).map_err(invalid_error)?;
            let identity = BuildOperationIdentity::new(operation_id, source_digest.to_owned())
                .map_err(invalid_error)?;
            Ok(Self {
                wire,
                plan,
                identity,
                plan_digest,
            })
        }

        fn validate_cache(
            &self,
            cache: &a3s_cloud_contracts::NodeBoxBuildCacheInput,
        ) -> Result<(), NodeBoxBuildError> {
            let expected_platform = cloud_platform(self.plan.platform());
            if cache.receipt.plan_digest != self.plan_digest
                || cache.receipt.platform != expected_platform
            {
                return Err(NodeBoxBuildError::Invalid(format!(
                    "operation {} cache receipt differs from its canonical plan",
                    self.wire.operation_id
                )));
            }
            Ok(())
        }
    }

    struct PublishedOutput {
        directory: PathBuf,
        descriptor: NodeBoxBuildDescriptor,
        platforms: Vec<NodeBoxBuildPlatform>,
        manifest_count: u64,
        content_bytes: u64,
        blob_count: u64,
        blob_inventory_digest: String,
    }

    fn aggregate_statuses(statuses: Vec<RecordedBuildStatus>) -> NodeBoxBuildPhase {
        let mut running = false;
        let mut cancelling = false;
        let mut cancelled = None;
        let mut failed = None;
        for status in statuses {
            match status {
                RecordedBuildStatus::Running => running = true,
                RecordedBuildStatus::Cancelling => cancelling = true,
                RecordedBuildStatus::Cancelled { message } => {
                    cancelled.get_or_insert_with(|| bounded_message(&message));
                }
                RecordedBuildStatus::Failed { message } => {
                    failed.get_or_insert_with(|| bounded_message(&message));
                }
                RecordedBuildStatus::Succeeded(_) => {}
            }
        }
        if let Some(message) = failed {
            NodeBoxBuildPhase::Failed { message }
        } else if let Some(message) = cancelled {
            NodeBoxBuildPhase::Cancelled { message }
        } else if cancelling {
            NodeBoxBuildPhase::Cancelling
        } else if running {
            NodeBoxBuildPhase::Running
        } else {
            NodeBoxBuildPhase::Succeeded
        }
    }

    fn cloud_cancellation(outcome: BuildCancellationOutcome) -> NodeBoxBuildCancellation {
        match outcome {
            BuildCancellationOutcome::NotFound => NodeBoxBuildCancellation::NotFound,
            BuildCancellationOutcome::Requested => NodeBoxBuildCancellation::Requested,
            BuildCancellationOutcome::AlreadyRequested => {
                NodeBoxBuildCancellation::AlreadyRequested
            }
            BuildCancellationOutcome::AlreadyCancelled => {
                NodeBoxBuildCancellation::AlreadyCancelled
            }
            BuildCancellationOutcome::AlreadyTerminal => NodeBoxBuildCancellation::AlreadyTerminal,
        }
    }

    fn cloud_platform(platform: &Platform) -> NodeBoxBuildPlatform {
        NodeBoxBuildPlatform {
            os: platform.os.clone(),
            architecture: platform.architecture.clone(),
            variant: platform.variant.clone(),
        }
    }

    fn box_platform(platform: &NodeBoxBuildPlatform) -> Platform {
        Platform {
            os: platform.os.clone(),
            architecture: platform.architecture.clone(),
            variant: platform.variant.clone(),
        }
    }

    fn cloud_descriptor(descriptor: &BuildOutputDescriptor) -> NodeBoxBuildDescriptor {
        NodeBoxBuildDescriptor {
            media_type: descriptor.media_type.clone(),
            digest: descriptor.digest.clone(),
            size: descriptor.size,
        }
    }

    fn box_descriptor(descriptor: &NodeBoxBuildDescriptor) -> BuildOutputDescriptor {
        BuildOutputDescriptor {
            media_type: descriptor.media_type.clone(),
            digest: descriptor.digest.clone(),
            size: descriptor.size,
        }
    }

    fn cloud_cache_receipt(receipt: &BuildCacheReceipt) -> NodeBoxBuildCacheReceipt {
        NodeBoxBuildCacheReceipt {
            schema: receipt.schema.clone(),
            key: receipt.key.clone(),
            source_digest: receipt.source_digest.clone(),
            plan_digest: receipt.plan_digest.clone(),
            descriptor: cloud_descriptor(&receipt.descriptor),
            platform: cloud_platform(&receipt.platform),
            content_bytes: receipt.content_bytes,
            entry_count: receipt.entry_count,
            blob_count: receipt.blob_count,
            blob_inventory_digest: receipt.blob_inventory_digest.clone(),
        }
    }

    fn box_cache_receipt(receipt: &NodeBoxBuildCacheReceipt) -> BuildCacheReceipt {
        BuildCacheReceipt {
            schema: receipt.schema.clone(),
            key: receipt.key.clone(),
            source_digest: receipt.source_digest.clone(),
            plan_digest: receipt.plan_digest.clone(),
            descriptor: box_descriptor(&receipt.descriptor),
            platform: box_platform(&receipt.platform),
            content_bytes: receipt.content_bytes,
            entry_count: receipt.entry_count,
            blob_count: receipt.blob_count,
            blob_inventory_digest: receipt.blob_inventory_digest.clone(),
        }
    }

    fn checked_count(value: usize, label: &str) -> Result<u64, NodeBoxBuildError> {
        u64::try_from(value).map_err(|_| {
            NodeBoxBuildError::State(format!("Box build {label} count exceeds the wire range"))
        })
    }

    fn invalid_error(error: impl std::fmt::Display) -> NodeBoxBuildError {
        NodeBoxBuildError::Invalid(bounded_message(&error.to_string()))
    }

    fn native_error(error: impl std::fmt::Display) -> NodeBoxBuildError {
        NodeBoxBuildError::Native(bounded_message(&error.to_string()))
    }

    fn bounded_message(message: &str) -> String {
        let normalized = message.replace(['\0', '\r', '\n'], " ");
        let normalized = normalized.trim();
        if normalized.is_empty() {
            "Box native build failed without a reason".into()
        } else {
            normalized.chars().take(4 * 1024).collect()
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) use native::BoxBuildCommandExecutor;
