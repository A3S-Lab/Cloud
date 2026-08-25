use super::build_run_access::require_definition_build_output;
use super::prior_writer_seal::{DurableCellPriorWriterSeal, DurableCellPriorWriterSealStatus};
#[cfg(test)]
use super::provider_workload::compose_pinned_celld_service_process;
use super::provider_workload::{
    durable_cell_managed_owner_reference, validate_durable_cell_provider_workload_binding,
    validate_pinned_celld_service_projection, validate_publisher_secret_targets,
};
use crate::modules::artifacts::domain::{BuildArtifact, IBuildRunRepository};
use crate::modules::data::ObjectNamespaceProviderProfile;
use crate::modules::durable_cells::domain::{
    DurableCellDeployment, DurableCellPublisherProfile, DurableCellServiceProfile,
    IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
    DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use crate::modules::executions::application::{
    validate_bound_execution, BoundExecutionCreation, ExecutionCancellation,
    ExecutionCancellationService, ExecutionCreator,
};
use crate::modules::executions::domain::{
    Execution, ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionStatus,
    ExecutionTaskAuthority, ExecutionTaskPolicy, ExecutionTemplate, IExecutionRepository,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ExecutionId, NodeId, RepositoryError, Sha256Digest, StorageNamespaceId, WorkloadRevisionId,
};
use crate::modules::workloads::application::project_runtime_secrets;
use crate::modules::workloads::domain::entities::WorkloadReplica;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::domain::services::{
    IWorkloadPrestartGate, WorkloadPrestartGateRequest, WorkloadPrestartGateStatus,
};
use a3s_runtime::contract::{ArtifactRef, RuntimeMount, RuntimeMountSource, SecretReference};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

const PUBLICATION_EXECUTION_NAME: &[u8] = b"a3s-cloud:durable-cell:bundle-publication-execution:v1";
const PUBLICATION_REQUEST_NAME: &[u8] = b"a3s-cloud:durable-cell:bundle-publication-request:v1";
const PUBLICATION_CANCELLATION_REQUEST_NAME: &[u8] =
    b"a3s-cloud:durable-cell:bundle-publication-cancellation-request:v1";
const PUBLICATION_AUTHORITY_KIND: &str = "durable-cell.bundle-publication";
const PUBLICATION_INPUT_SCHEMA: &str = "cloud.durable-cell.bundle-publication.v1";
const OCI_IMAGE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
const OCI_IMAGE_INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";

/// Durable Cell adapter for the generic Workloads pre-start gate. It admits a
/// new writer only after the exact prior S0 seal, then composes one
/// deterministic, node-bound publication Execution and observes the existing
/// lifecycle. It owns no queue, worker, seal, or publication state.
#[derive(Clone)]
pub(crate) struct DurableCellBundlePublicationGate {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    deployments: Arc<dyn IDurableCellDeploymentRepository>,
    builds: Arc<dyn IBuildRunRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    prior_writer_seal: DurableCellPriorWriterSeal,
    executions: Arc<dyn IExecutionRepository>,
    creator: ExecutionCreator,
    cancellations: ExecutionCancellationService,
}

impl DurableCellBundlePublicationGate {
    pub(crate) fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        deployments: Arc<dyn IDurableCellDeploymentRepository>,
        builds: Arc<dyn IBuildRunRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        prior_writer_seal: DurableCellPriorWriterSeal,
        environments: Arc<dyn IEnvironmentRepository>,
        executions: Arc<dyn IExecutionRepository>,
    ) -> Self {
        Self {
            applications,
            deployments,
            builds,
            workloads,
            prior_writer_seal,
            executions: Arc::clone(&executions),
            creator: ExecutionCreator::new(environments, Arc::clone(&executions)),
            cancellations: ExecutionCancellationService::new(executions),
        }
    }

    async fn find_correlation(
        &self,
        request: &WorkloadPrestartGateRequest,
    ) -> Result<Option<DurableCellPrestartCorrelation>, RepositoryError> {
        let correlation = match self
            .deployments
            .find_by_workload_revision(request.organization_id, request.workload_revision_id)
            .await
        {
            Ok(value) => value,
            Err(RepositoryError::NotFound) => None,
            Err(error) => return Err(error),
        };
        let Some(correlation) = correlation else {
            return Ok(None);
        };
        correlation.validate().map_err(|error| {
            RepositoryError::Conflict(format!(
                "Durable Cell deployment correlation is invalid: {error}"
            ))
        })?;
        if correlation.projection.organization_id != request.organization_id
            || correlation.projection.workload_id != request.workload_id
            || correlation.projection.workload_revision_id != request.workload_revision_id
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell pre-start request changed its projection identity".into(),
            ));
        }
        let deployment = self
            .workloads
            .find_deployment(request.organization_id, request.deployment_id)
            .await?;
        if deployment.id != request.deployment_id
            || deployment.operation_id != request.operation_id
            || deployment.organization_id != request.organization_id
            || deployment.workload_id != request.workload_id
            || deployment.revision_id != request.workload_revision_id
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell pre-start request changed its Workload Deployment".into(),
            ));
        }
        let control = self
            .workloads
            .find_workload_control(request.organization_id, request.workload_id)
            .await?;
        let expected_owner = durable_cell_managed_owner_reference(&correlation.projection)
            .map_err(|error| {
                RepositoryError::Conflict(format!(
                    "could not restore Durable Cell managed owner: {error}"
                ))
            })?;
        if control.organization_id != correlation.projection.organization_id
            || control.project_id != correlation.projection.project_id
            || control.environment_id != correlation.projection.environment_id
            || control.workload_id != request.workload_id
            || control.spec.managed_owner.as_ref() != Some(&expected_owner)
            || control.spec.placement_policy.members_per_replica() != 1
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell pre-start request changed its managed Workload control".into(),
            ));
        }
        let binding = self
            .workloads
            .find_deployment_replica_binding(request.organization_id, request.deployment_id)
            .await?;
        let canonical_replica_id = WorkloadReplica::deterministic_id(request.workload_id, 0)
            .map_err(RepositoryError::Conflict)?;
        let replica = self
            .workloads
            .find_workload_replica(
                request.organization_id,
                request.workload_id,
                canonical_replica_id,
            )
            .await?;
        if binding.deployment_id != request.deployment_id
            || binding.organization_id != correlation.projection.organization_id
            || binding.project_id != correlation.projection.project_id
            || binding.environment_id != correlation.projection.environment_id
            || binding.workload_id != request.workload_id
            || binding.revision_id != request.workload_revision_id
            || binding.replica_id != canonical_replica_id
            || binding.replica_generation == 0
            || binding.runtime_generation != binding.replica_generation
            || binding.node_id != Some(request.node_id)
            || replica.id != binding.replica_id
            || replica.ordinal != 0
            || replica.revision_id != binding.revision_id
            || replica.revision_generation != correlation.provider.workload_generation
            || replica.generation != binding.replica_generation
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell pre-start request changed its canonical writer binding".into(),
            ));
        }
        Ok(Some(DurableCellPrestartCorrelation {
            deployment: correlation,
            writer_epoch: binding.replica_generation,
        }))
    }

    async fn reconcile_publication(
        &self,
        request: &WorkloadPrestartGateRequest,
        correlation: &DurableCellDeployment,
    ) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
        let execution_id = publication_execution_id(request.workload_revision_id);
        if request.cancellation_requested {
            return self
                .reconcile_cancellation(request, correlation, execution_id)
                .await;
        }

        if let Some(execution) = self
            .executions
            .find(request.organization_id, execution_id)
            .await?
        {
            let target_node_id = execution.target_node_id.ok_or_else(|| {
                RepositoryError::Conflict(
                    "Durable Cell publication Execution omitted its bound target node".into(),
                )
            })?;
            let persisted_request = WorkloadPrestartGateRequest {
                node_id: target_node_id,
                ..request.clone()
            };
            let creation = match self
                .compose(&persisted_request, correlation, execution_id)
                .await
            {
                Ok(creation) => creation,
                Err(CompositionError::Failed(reason)) => {
                    return Ok(WorkloadPrestartGateStatus::Failed { reason });
                }
                Err(CompositionError::Repository(error)) => return Err(error),
            };
            validate_bound_execution(&creation, &execution)
                .map_err(application_repository_error)?;
            if target_node_id != request.node_id && execution.status != ExecutionStatus::Succeeded {
                return Ok(WorkloadPrestartGateStatus::Failed {
                    reason: format!(
                        "Durable Cell bundle publication Execution {} is bound to a previous node and has not succeeded",
                        execution.id
                    ),
                });
            }
            return publication_status(&execution);
        }

        let creation = match self.compose(request, correlation, execution_id).await {
            Ok(creation) => creation,
            Err(CompositionError::Failed(reason)) => {
                return Ok(WorkloadPrestartGateStatus::Failed { reason })
            }
            Err(CompositionError::Repository(error)) => return Err(error),
        };
        let execution = match self.creator.create_bound_task(creation).await {
            Ok(result) => result.execution,
            Err(error) => return application_error(error),
        };
        publication_status(&execution)
    }

    async fn reconcile_cancellation(
        &self,
        request: &WorkloadPrestartGateRequest,
        correlation: &DurableCellDeployment,
        execution_id: ExecutionId,
    ) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
        let Some(execution) = self
            .executions
            .find(request.organization_id, execution_id)
            .await?
        else {
            return Ok(WorkloadPrestartGateStatus::CancellationReady {
                completed_at: request.now,
            });
        };
        validate_publication_execution(request, correlation, &execution)?;
        if execution.status.is_terminal() {
            return Ok(WorkloadPrestartGateStatus::CancellationReady {
                completed_at: execution.finished_at.unwrap_or(request.now),
            });
        }
        if !matches!(
            execution.status,
            ExecutionStatus::Cancelling | ExecutionStatus::CleanupPending
        ) {
            self.cancellations
                .cancel(ExecutionCancellation {
                    execution,
                    idempotency_key: format!("durable-cell-publication-cancel:{execution_id}"),
                    request_id: Uuid::new_v5(
                        &request.workload_revision_id.as_uuid(),
                        PUBLICATION_CANCELLATION_REQUEST_NAME,
                    ),
                    requested_at: request.now,
                })
                .await
                .map_err(application_repository_error)?;
        }
        Ok(WorkloadPrestartGateStatus::Pending {
            reason: "Durable Cell bundle publication Execution is cancelling".into(),
        })
    }

    async fn compose(
        &self,
        request: &WorkloadPrestartGateRequest,
        correlation: &DurableCellDeployment,
        execution_id: ExecutionId,
    ) -> Result<BoundExecutionCreation, CompositionError> {
        let provider_profile = correlation
            .require_storage_provider_profile()
            .map_err(CompositionError::failed)?;
        let publisher =
            DurableCellPublisherProfile::pinned_celld_v0_2_1().map_err(CompositionError::failed)?;

        let application_revision = self
            .applications
            .find_revision(
                correlation.projection.organization_id,
                correlation.projection.project_id,
                correlation.projection.environment_id,
                correlation.projection.application_id,
                correlation.projection.application_revision_id,
            )
            .await
            .map_err(CompositionError::Repository)?
            .ok_or_else(|| {
                CompositionError::failed("Durable Cell application revision was not found")
            })?;
        application_revision
            .validate()
            .map_err(CompositionError::failed)?;
        if application_revision.id != correlation.projection.application_revision_id
            || application_revision.application_id != correlation.projection.application_id
            || application_revision.revision_number
                != correlation.projection.application_revision_number
            || application_revision.definition.digest()
                != &correlation.projection.application_definition_digest
        {
            return Err(CompositionError::failed(
                "Durable Cell application revision changed from its deployment correlation",
            ));
        }
        let bundle = require_definition_build_output(
            self.builds.as_ref(),
            correlation.projection.organization_id,
            correlation.projection.project_id,
            correlation.projection.environment_id,
            &application_revision.definition,
        )
        .await
        .map_err(CompositionError::application)?;
        validate_bundle(
            &bundle,
            &application_revision.definition.spec().bundle_digest,
        )
        .map_err(CompositionError::failed)?;

        let workload_revision = self
            .workloads
            .find_revision(request.organization_id, request.workload_revision_id)
            .await
            .map_err(CompositionError::Repository)?;
        let service_template = workload_revision
            .resolved_template()
            .map_err(CompositionError::failed)?;
        let service_profile =
            DurableCellServiceProfile::pinned_celld_v0_2_1().map_err(CompositionError::failed)?;
        validate_durable_cell_provider_workload_binding(
            &correlation.provider,
            &service_profile,
            &workload_revision,
        )
        .map_err(CompositionError::failed)?;
        validate_pinned_celld_service_projection(
            &provider_profile,
            correlation.storage.storage_namespace_id,
            &service_profile,
            service_template,
            &publisher,
        )
        .map_err(CompositionError::failed)?;
        validate_publisher_secret_targets(service_template, &publisher)
            .map_err(CompositionError::failed)?;

        let authority_digest =
            publication_authority_digest(correlation, request.node_id, &bundle, &publisher)
                .map_err(CompositionError::failed)?;
        let secrets =
            project_runtime_secrets(&workload_revision).map_err(CompositionError::failed)?;
        let definition = build_publication_task_definition(
            &provider_profile,
            &publisher,
            PublicationTaskDefinitionInput {
                node_id: request.node_id,
                storage_namespace_id: correlation.storage.storage_namespace_id,
                image_media_type: service_template.artifact.media_type.clone(),
                bundle: ArtifactRef {
                    uri: bundle.uri,
                    digest: bundle.digest,
                    media_type: bundle.media_type,
                },
                secrets,
                authority: ExecutionTaskAuthority {
                    kind: PUBLICATION_AUTHORITY_KIND.into(),
                    subject_id: request.workload_revision_id.as_uuid(),
                    digest: authority_digest,
                },
                input: serde_json::json!({
                "schema": PUBLICATION_INPUT_SCHEMA,
                "applicationRevisionId": correlation.projection.application_revision_id,
                "applicationDefinitionDigest": correlation.projection.application_definition_digest,
                "bundleDigest": application_revision.definition.spec().bundle_digest,
                "storageNamespaceId": correlation.storage.storage_namespace_id,
                "storageProviderProfileDigest": correlation.storage.provider_profile_digest,
                "publisherProfileDigest": publisher.digest(),
            }),
            },
        )
        .map_err(CompositionError::failed)?;
        Ok(BoundExecutionCreation {
            organization_id: correlation.projection.organization_id,
            project_id: correlation.projection.project_id,
            environment_id: correlation.projection.environment_id,
            execution_id,
            template: definition.template,
            target_node_id: request.node_id,
            task_policy: definition.task_policy,
            idempotency_key: format!("durable-cell-publication:{execution_id}"),
            request_id: Uuid::new_v5(
                &request.workload_revision_id.as_uuid(),
                PUBLICATION_REQUEST_NAME,
            ),
            requested_at: correlation.requested_at,
        })
    }
}

struct PublicationTaskDefinitionInput {
    node_id: NodeId,
    storage_namespace_id: StorageNamespaceId,
    image_media_type: String,
    bundle: ArtifactRef,
    secrets: Vec<SecretReference>,
    authority: ExecutionTaskAuthority,
    input: serde_json::Value,
}

struct PublicationTaskDefinition {
    template: ExecutionTemplate,
    task_policy: ExecutionTaskPolicy,
}

/// The sole provider-adapter translation from Cloud-owned publication intent
/// into one ordinary Execution Task. Both the Workload pre-start gate and the
/// retained real-provider test use this constructor so command, S0 namespace,
/// mount, Secret, resource, and network semantics cannot drift independently.
fn build_publication_task_definition(
    provider_profile: &ObjectNamespaceProviderProfile,
    publisher: &DurableCellPublisherProfile,
    definition: PublicationTaskDefinitionInput,
) -> Result<PublicationTaskDefinition, String> {
    provider_profile.validate()?;
    publisher.validate()?;
    if provider_profile.spec().virtual_hosted_style {
        return Err("celld v0.2.1 publication requires path-style S0 addressing".into());
    }
    if !matches!(
        definition.image_media_type.as_str(),
        OCI_IMAGE_MANIFEST_MEDIA_TYPE | OCI_IMAGE_INDEX_MEDIA_TYPE
    ) {
        return Err("Durable Cell publisher image media type is not OCI".into());
    }
    a3s_cloud_contracts::validate_cloud_artifact(&definition.bundle)?;
    if definition.bundle.media_type != DURABLE_CELL_BUNDLE_MEDIA_TYPE {
        return Err("Durable Cell publisher input is not the typed bundle artifact".into());
    }
    let namespace_prefix = provider_profile.namespace_prefix(definition.storage_namespace_id)?;
    let template = ExecutionTemplate {
        artifact: ExecutionArtifact {
            uri: publisher.image_uri().into(),
            digest: publisher.image_digest().to_string(),
            media_type: definition.image_media_type,
        },
        process: ExecutionProcess {
            command: publisher.command().to_vec(),
            args: vec![
                "deploy".into(),
                publisher.bundle_mount().into(),
                "--bucket".into(),
                format!(
                    "s3://{}/{}",
                    provider_profile.spec().bucket,
                    namespace_prefix
                ),
                "--endpoint".into(),
                provider_profile.spec().endpoint.clone(),
                "--region".into(),
                provider_profile.spec().region.clone(),
            ],
            working_directory: Some(publisher.bundle_mount().into()),
            environment: BTreeMap::new(),
        },
        input: definition.input,
        resources: ExecutionResources {
            cpu_millis: publisher.cpu_millis(),
            memory_bytes: publisher.memory_bytes(),
            pids: publisher.pids(),
            ephemeral_storage_bytes: Some(publisher.ephemeral_storage_bytes()),
            timeout_ms: publisher.timeout_ms(),
        },
    };
    let task_policy = ExecutionTaskPolicy {
        authority: definition.authority,
        mounts: vec![RuntimeMount {
            name: "durable-cell-application".into(),
            source: RuntimeMountSource::Artifact {
                artifact: definition.bundle,
            },
            target: publisher.bundle_mount().into(),
            read_only: true,
        }],
        secrets: definition.secrets,
        semantics_profile_digest: publisher.digest().clone(),
    };
    task_policy.validate(definition.node_id, &template)?;
    Ok(PublicationTaskDefinition {
        template,
        task_policy,
    })
}

#[async_trait]
impl IWorkloadPrestartGate for DurableCellBundlePublicationGate {
    async fn reconcile(
        &self,
        request: &WorkloadPrestartGateRequest,
    ) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
        let Some(correlation) = self.find_correlation(request).await? else {
            return Ok(if request.cancellation_requested {
                WorkloadPrestartGateStatus::CancellationReady {
                    completed_at: request.now,
                }
            } else {
                WorkloadPrestartGateStatus::Ready {
                    completed_at: request.now,
                }
            });
        };
        if request.cancellation_requested {
            return if correlation
                .deployment
                .storage_provider_profile_acl
                .is_none()
            {
                Ok(WorkloadPrestartGateStatus::CancellationReady {
                    completed_at: request.now,
                })
            } else {
                self.reconcile_publication(request, &correlation.deployment)
                    .await
            };
        }
        match self
            .prior_writer_seal
            .reconcile(&correlation.deployment, correlation.writer_epoch)
            .await?
        {
            DurableCellPriorWriterSealStatus::Ready { .. } => {}
            DurableCellPriorWriterSealStatus::Pending { reason } => {
                return Ok(WorkloadPrestartGateStatus::Pending { reason });
            }
            DurableCellPriorWriterSealStatus::Failed { reason } => {
                return Ok(WorkloadPrestartGateStatus::Failed { reason });
            }
        }
        if correlation
            .deployment
            .storage_provider_profile_acl
            .is_none()
        {
            return Ok(WorkloadPrestartGateStatus::Ready {
                completed_at: request.now,
            });
        }
        self.reconcile_publication(request, &correlation.deployment)
            .await
    }
}

struct DurableCellPrestartCorrelation {
    deployment: DurableCellDeployment,
    writer_epoch: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicationAuthorityIdentity<'a> {
    deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
    workload_revision_id: WorkloadRevisionId,
    node_id: crate::modules::shared_kernel::domain::NodeId,
    application_definition_digest: &'a str,
    bundle_digest: &'a str,
    storage_namespace_id: crate::modules::shared_kernel::domain::StorageNamespaceId,
    storage_provider_profile_digest: &'a str,
    credential_binding_digest: &'a str,
    publisher_profile_digest: &'a str,
}

fn publication_authority_digest(
    correlation: &DurableCellDeployment,
    node_id: crate::modules::shared_kernel::domain::NodeId,
    bundle: &BuildArtifact,
    publisher: &DurableCellPublisherProfile,
) -> Result<Sha256Digest, String> {
    let bytes = serde_json::to_vec(&PublicationAuthorityIdentity {
        deployment_id: correlation.projection.deployment_id,
        workload_revision_id: correlation.projection.workload_revision_id,
        node_id,
        application_definition_digest: correlation
            .projection
            .application_definition_digest
            .as_str(),
        bundle_digest: &bundle.digest,
        storage_namespace_id: correlation.storage.storage_namespace_id,
        storage_provider_profile_digest: correlation.storage.provider_profile_digest.as_str(),
        credential_binding_digest: correlation.storage.credential_binding_digest.as_str(),
        publisher_profile_digest: publisher.digest().as_str(),
    })
    .map_err(|error| format!("could not encode Durable Cell publication authority: {error}"))?;
    Ok(Sha256Digest::from_bytes(&bytes))
}

fn publication_execution_id(revision_id: WorkloadRevisionId) -> ExecutionId {
    ExecutionId::from_uuid(Uuid::new_v5(
        &revision_id.as_uuid(),
        PUBLICATION_EXECUTION_NAME,
    ))
}

fn validate_bundle(bundle: &BuildArtifact, expected: &Sha256Digest) -> Result<(), String> {
    bundle.validate()?;
    let artifact = ArtifactRef {
        uri: bundle.uri.clone(),
        digest: bundle.digest.clone(),
        media_type: bundle.media_type.clone(),
    };
    a3s_cloud_contracts::validate_cloud_artifact(&artifact)?;
    if bundle.digest != expected.as_str() || bundle.media_type != DURABLE_CELL_BUNDLE_MEDIA_TYPE {
        return Err("Durable Cell bundle artifact changed from its application definition".into());
    }
    Ok(())
}

fn validate_publication_execution(
    request: &WorkloadPrestartGateRequest,
    correlation: &DurableCellDeployment,
    execution: &Execution,
) -> Result<(), RepositoryError> {
    let task_policy = execution.task_policy.as_ref();
    if execution.organization_id != correlation.projection.organization_id
        || execution.project_id != correlation.projection.project_id
        || execution.environment_id != correlation.projection.environment_id
        || execution.id != publication_execution_id(request.workload_revision_id)
        || execution.target_node_id.is_none()
        || execution.workflow.is_some()
        || task_policy.is_none_or(|policy| {
            policy.authority.kind != PUBLICATION_AUTHORITY_KIND
                || policy.authority.subject_id != request.workload_revision_id.as_uuid()
        })
    {
        return Err(RepositoryError::Conflict(
            "Durable Cell publication Execution changed its immutable identity".into(),
        ));
    }
    Ok(())
}

fn publication_status(
    execution: &Execution,
) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
    match execution.status {
        ExecutionStatus::Succeeded => Ok(WorkloadPrestartGateStatus::Ready {
            completed_at: execution.finished_at.ok_or_else(|| {
                RepositoryError::Conflict(
                    "successful Durable Cell publication omitted its completion time".into(),
                )
            })?,
        }),
        ExecutionStatus::Failed => Ok(WorkloadPrestartGateStatus::Failed {
            reason: format!(
                "Durable Cell bundle publication Execution {} failed",
                execution.id
            ),
        }),
        ExecutionStatus::Cancelled => Ok(WorkloadPrestartGateStatus::Failed {
            reason: format!(
                "Durable Cell bundle publication Execution {} was cancelled",
                execution.id
            ),
        }),
        status => Ok(WorkloadPrestartGateStatus::Pending {
            reason: format!(
                "Durable Cell bundle publication Execution {} is {}",
                execution.id,
                status.as_str()
            ),
        }),
    }
}

fn application_error(
    error: ApplicationError,
) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
    match error {
        ApplicationError::Internal(reason) | ApplicationError::Unavailable(reason) => {
            Err(RepositoryError::Storage(reason))
        }
        error => Ok(WorkloadPrestartGateStatus::Failed {
            reason: format!("Durable Cell publication admission failed: {error}"),
        }),
    }
}

fn application_repository_error(error: ApplicationError) -> RepositoryError {
    match error {
        ApplicationError::Internal(reason) | ApplicationError::Unavailable(reason) => {
            RepositoryError::Storage(reason)
        }
        ApplicationError::NotFound(reason) => RepositoryError::Conflict(reason),
        ApplicationError::Invalid(reason)
        | ApplicationError::Conflict(reason)
        | ApplicationError::Forbidden(reason) => RepositoryError::Conflict(reason),
    }
}

enum CompositionError {
    Failed(String),
    Repository(RepositoryError),
}

impl CompositionError {
    fn failed(error: impl ToString) -> Self {
        Self::Failed(error.to_string())
    }

    fn application(error: ApplicationError) -> Self {
        match error {
            ApplicationError::Internal(reason) | ApplicationError::Unavailable(reason) => {
                Self::Repository(RepositoryError::Storage(reason))
            }
            error => Self::Failed(error.to_string()),
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "bundle_publication/real_conformance.rs"]
mod real_conformance;

#[cfg(test)]
#[path = "bundle_publication/tests.rs"]
mod tests;
