use super::build_artifact_port::{DurableCellBuildArtifact, IDurableCellBuildArtifactPort};
use super::build_run_access::require_definition_build_output;
use super::execution_port::{
    DurableCellExecution, DurableCellExecutionArtifactMount, DurableCellExecutionAuthority,
    DurableCellExecutionCancellationRequest, DurableCellExecutionRequest,
    DurableCellExecutionStatus, DurableCellExecutionTaskPolicy, DurableCellExecutionTemplate,
    IDurableCellExecutionPort,
};
use super::prior_writer_seal::{DurableCellPriorWriterSeal, DurableCellPriorWriterSealStatus};
#[cfg(test)]
use super::provider_workload::compose_pinned_celld_service_process;
use super::provider_workload::{
    validate_durable_cell_provider_workload_projection,
    validate_pinned_celld_service_template_payload_projection,
};
use super::storage_port::{
    DurableCellStorageProviderProfileProjection, DurableCellStorageProviderProfileRequest,
    IDurableCellStoragePort,
};
use super::workload_port::{
    DurableCellWorkloadPrestartProjection, DurableCellWorkloadPrestartRequest,
    IDurableCellWorkloadPort,
};
use crate::modules::durable_cells::domain::{
    DurableCellDeployment, DurableCellPublisherProfile, DurableCellServiceProfile,
    IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
    DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ExecutionId, RepositoryError, Sha256Digest, StorageNamespaceId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::services::{
    IWorkloadPrestartGate, WorkloadPrestartGateRequest, WorkloadPrestartGateStatus,
};
use a3s_runtime::contract::{ArtifactRef, ResourceLimits, RuntimeProcessSpec, SecretReference};
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
    builds: Arc<dyn IDurableCellBuildArtifactPort>,
    workloads: Arc<dyn IDurableCellWorkloadPort>,
    storage: Arc<dyn IDurableCellStoragePort>,
    prior_writer_seal: DurableCellPriorWriterSeal,
    executions: Arc<dyn IDurableCellExecutionPort>,
}

impl DurableCellBundlePublicationGate {
    pub(crate) fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        deployments: Arc<dyn IDurableCellDeploymentRepository>,
        builds: Arc<dyn IDurableCellBuildArtifactPort>,
        workloads: Arc<dyn IDurableCellWorkloadPort>,
        prior_writer_seal: DurableCellPriorWriterSeal,
        executions: Arc<dyn IDurableCellExecutionPort>,
    ) -> Self {
        Self {
            applications,
            deployments,
            builds,
            workloads,
            storage: prior_writer_seal.storage_port(),
            prior_writer_seal,
            executions,
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
        let workload_request = DurableCellWorkloadPrestartRequest::new(
            correlation.projection.organization_id,
            correlation.projection.project_id,
            correlation.projection.environment_id,
            correlation.projection.application_id,
            correlation.projection.application_revision_id,
            correlation.projection.application_revision_number,
            correlation.projection.application_definition_digest.clone(),
            correlation.projection.workload_id,
            correlation.projection.workload_revision_id,
            correlation.provider.workload_generation,
            request.deployment_id,
            request.operation_id,
            request.node_id,
        );
        let workload = self
            .workloads
            .load_prestart_publication(&workload_request)
            .await
            .map_err(application_repository_error)?;
        workload
            .validate_against(&workload_request)
            .map_err(RepositoryError::Conflict)?;
        Ok(Some(DurableCellPrestartCorrelation {
            deployment: correlation,
            workload,
        }))
    }

    async fn reconcile_publication(
        &self,
        request: &WorkloadPrestartGateRequest,
        correlation: &DurableCellPrestartCorrelation,
    ) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
        let execution_id = publication_execution_id(request.workload_revision_id);
        if request.cancellation_requested {
            return self
                .reconcile_cancellation(request, &correlation.deployment, execution_id)
                .await;
        }

        if let Some(execution) = self
            .executions
            .find_bound_task(request.organization_id, execution_id)
            .await
            .map_err(application_repository_error)?
        {
            let target_node_id = execution.target_node_id;
            let persisted_request = WorkloadPrestartGateRequest {
                node_id: target_node_id,
                ..request.clone()
            };
            let creation = match self
                .compose(
                    &persisted_request,
                    &correlation.deployment,
                    &correlation.workload,
                    execution_id,
                )
                .await
            {
                Ok(creation) => creation,
                Err(CompositionError::Failed(reason)) => {
                    return Ok(WorkloadPrestartGateStatus::Failed { reason });
                }
                Err(CompositionError::Repository(error)) => return Err(error),
            };
            let execution = self
                .executions
                .ensure_bound_task(&creation)
                .await
                .map_err(application_repository_error)?;
            if target_node_id != request.node_id
                && execution.status != DurableCellExecutionStatus::Succeeded
            {
                return Ok(WorkloadPrestartGateStatus::Failed {
                    reason: format!(
                        "Durable Cell bundle publication Execution {} is bound to a previous node and has not succeeded",
                        execution.id
                    ),
                });
            }
            return publication_status(&execution);
        }

        let creation = match self
            .compose(
                request,
                &correlation.deployment,
                &correlation.workload,
                execution_id,
            )
            .await
        {
            Ok(creation) => creation,
            Err(CompositionError::Failed(reason)) => {
                return Ok(WorkloadPrestartGateStatus::Failed { reason })
            }
            Err(CompositionError::Repository(error)) => return Err(error),
        };
        let execution = match self.executions.ensure_bound_task(&creation).await {
            Ok(execution) => execution,
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
            .find_bound_task(request.organization_id, execution_id)
            .await
            .map_err(application_repository_error)?
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
            DurableCellExecutionStatus::Cancelling | DurableCellExecutionStatus::CleanupPending
        ) {
            self.executions
                .cancel_bound_task(&DurableCellExecutionCancellationRequest {
                    organization_id: request.organization_id,
                    project_id: correlation.projection.project_id,
                    environment_id: correlation.projection.environment_id,
                    execution_id,
                    authority_kind: PUBLICATION_AUTHORITY_KIND.into(),
                    authority_subject_id: request.workload_revision_id.as_uuid(),
                    idempotency_key: format!("durable-cell-publication-cancel:{execution_id}"),
                    request_id: Uuid::new_v5(
                        &request.workload_revision_id.as_uuid(),
                        PUBLICATION_CANCELLATION_REQUEST_NAME,
                    ),
                    requested_at: canonical_timestamp(request.now),
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
        workload: &DurableCellWorkloadPrestartProjection,
        execution_id: ExecutionId,
    ) -> Result<DurableCellExecutionRequest, CompositionError> {
        let provider_profile = correlation
            .storage_provider_profile_acl
            .as_deref()
            .ok_or_else(|| {
                CompositionError::failed(
                    "Durable Cell publication requires the bound S0 provider profile",
                )
            })?;
        let provider_profile = self
            .storage
            .project_provider_profile(
                &DurableCellStorageProviderProfileRequest::new(
                    provider_profile,
                    correlation.storage.provider_profile_digest.clone(),
                )
                .map_err(CompositionError::failed)?,
            )
            .await
            .map_err(CompositionError::application)?;
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

        let service_profile =
            DurableCellServiceProfile::pinned_celld_v0_2_1().map_err(CompositionError::failed)?;
        validate_durable_cell_provider_workload_projection(
            &correlation.provider,
            &service_profile,
            &workload.provider_workload,
        )
        .map_err(CompositionError::failed)?;
        let image_media_type = validate_pinned_celld_service_template_payload_projection(
            &provider_profile,
            correlation.storage.storage_namespace_id,
            &service_profile,
            &workload.service_template,
            &publisher,
        )
        .map_err(CompositionError::failed)?;

        let authority_digest =
            publication_authority_digest(correlation, request.node_id, &bundle, &publisher)
                .map_err(CompositionError::failed)?;
        let definition = build_publication_task_definition(
            &provider_profile,
            &publisher,
            PublicationTaskDefinitionInput {
                storage_namespace_id: correlation.storage.storage_namespace_id,
                image_media_type,
                bundle: ArtifactRef {
                    uri: bundle.uri,
                    digest: bundle.digest,
                    media_type: bundle.media_type,
                },
                secrets: workload.runtime_secrets.clone(),
                authority: DurableCellExecutionAuthority {
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
        Ok(DurableCellExecutionRequest {
            organization_id: correlation.projection.organization_id,
            project_id: correlation.projection.project_id,
            environment_id: correlation.projection.environment_id,
            execution_id,
            template: definition.template,
            target_node_id: request.node_id,
            task_policy: definition.task_policy,
            authority_subject_id: request.workload_revision_id.as_uuid(),
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
    storage_namespace_id: StorageNamespaceId,
    image_media_type: String,
    bundle: ArtifactRef,
    secrets: Vec<SecretReference>,
    authority: DurableCellExecutionAuthority,
    input: serde_json::Value,
}

struct PublicationTaskDefinition {
    template: DurableCellExecutionTemplate,
    task_policy: DurableCellExecutionTaskPolicy,
}

/// The sole provider-adapter translation from Cloud-owned publication intent
/// into one ordinary Execution Task. Both the Workload pre-start gate and the
/// retained real-provider test use this constructor so command, S0 namespace,
/// mount, Secret, resource, and network semantics cannot drift independently.
fn build_publication_task_definition(
    provider_profile: &DurableCellStorageProviderProfileProjection,
    publisher: &DurableCellPublisherProfile,
    definition: PublicationTaskDefinitionInput,
) -> Result<PublicationTaskDefinition, String> {
    provider_profile.validate()?;
    publisher.validate()?;
    if provider_profile.virtual_hosted_style {
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
    let template = DurableCellExecutionTemplate {
        artifact: ArtifactRef {
            uri: publisher.image_uri().into(),
            digest: publisher.image_digest().to_string(),
            media_type: definition.image_media_type,
        },
        process: RuntimeProcessSpec {
            command: publisher.command().to_vec(),
            args: vec![
                "deploy".into(),
                publisher.bundle_mount().into(),
                "--bucket".into(),
                format!("s3://{}/{}", provider_profile.bucket, namespace_prefix),
                "--endpoint".into(),
                provider_profile.endpoint.clone(),
                "--region".into(),
                provider_profile.region.clone(),
            ],
            working_directory: Some(publisher.bundle_mount().into()),
            environment: BTreeMap::new(),
        },
        input: definition.input,
        resources: ResourceLimits {
            cpu_millis: publisher.cpu_millis(),
            memory_bytes: publisher.memory_bytes(),
            pids: publisher.pids(),
            ephemeral_storage_bytes: Some(publisher.ephemeral_storage_bytes()),
            execution_timeout_ms: Some(publisher.timeout_ms()),
        },
    };
    let ArtifactRef {
        uri,
        digest,
        media_type,
    } = definition.bundle;
    let mount = DurableCellExecutionArtifactMount {
        name: "durable-cell-application".into(),
        artifact: ArtifactRef {
            uri,
            digest,
            media_type,
        },
        target: publisher.bundle_mount().into(),
    };
    let task_policy = DurableCellExecutionTaskPolicy {
        authority: definition.authority,
        mounts: vec![mount],
        secrets: definition.secrets,
        semantics_profile_digest: publisher.digest().clone(),
    };
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
                self.reconcile_publication(request, &correlation).await
            };
        }
        match self
            .prior_writer_seal
            .reconcile(&correlation.deployment, correlation.workload.writer_epoch)
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
        self.reconcile_publication(request, &correlation).await
    }
}

struct DurableCellPrestartCorrelation {
    deployment: DurableCellDeployment,
    workload: DurableCellWorkloadPrestartProjection,
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
    bundle: &DurableCellBuildArtifact,
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

fn validate_bundle(
    bundle: &DurableCellBuildArtifact,
    expected: &Sha256Digest,
) -> Result<(), String> {
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
    execution: &DurableCellExecution,
) -> Result<(), RepositoryError> {
    if execution.organization_id != correlation.projection.organization_id
        || execution.project_id != correlation.projection.project_id
        || execution.environment_id != correlation.projection.environment_id
        || execution.id != publication_execution_id(request.workload_revision_id)
        || execution.target_node_id.as_uuid().is_nil()
        || execution.authority_kind != PUBLICATION_AUTHORITY_KIND
        || execution.authority_subject_id != request.workload_revision_id.as_uuid()
    {
        return Err(RepositoryError::Conflict(
            "Durable Cell publication Execution changed its immutable identity".into(),
        ));
    }
    Ok(())
}

fn publication_status(
    execution: &DurableCellExecution,
) -> Result<WorkloadPrestartGateStatus, RepositoryError> {
    match execution.status {
        DurableCellExecutionStatus::Succeeded => Ok(WorkloadPrestartGateStatus::Ready {
            completed_at: execution.finished_at.ok_or_else(|| {
                RepositoryError::Conflict(
                    "successful Durable Cell publication omitted its completion time".into(),
                )
            })?,
        }),
        DurableCellExecutionStatus::Failed => Ok(WorkloadPrestartGateStatus::Failed {
            reason: format!(
                "Durable Cell bundle publication Execution {} failed",
                execution.id
            ),
        }),
        DurableCellExecutionStatus::Cancelled => Ok(WorkloadPrestartGateStatus::Failed {
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
