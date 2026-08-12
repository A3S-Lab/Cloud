use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, OperationId, RepositoryError,
};
use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, Workload, WorkloadDesiredState, WorkloadReplica,
    WorkloadReplicaLifecycle, WorkloadReplicaMember, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    IWorkloadReplicaDeploymentRepository, ReplicaDeploymentCandidate,
    ReplicaDeploymentMaterialization,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const DEPLOYMENT_ID_DOMAIN: &str = "a3s.cloud.replica-deployment.v1";
const OPERATION_ID_DOMAIN: &str = "a3s.cloud.replica-deployment-operation.v1";
const CORRELATION_ID_DOMAIN: &str = "a3s.cloud.replica-deployment-materialization.v1";

pub(crate) struct ReplicaDeploymentWrite {
    pub deployment: Deployment,
    pub operation: OperationRequest,
    pub binding: DeploymentReplicaBinding,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplicaDeploymentMaterializationReport {
    pub candidates: usize,
    pub created: usize,
    pub replayed: usize,
    pub skipped: usize,
    pub failures: Vec<ReplicaDeploymentMaterializationFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaDeploymentMaterializationFailure {
    pub candidate: ReplicaDeploymentCandidate,
    pub message: String,
}

pub struct ReplicaDeploymentMaterializer {
    repository: Arc<dyn IWorkloadReplicaDeploymentRepository>,
    reconcile_interval: Duration,
    batch_size: usize,
}

impl ReplicaDeploymentMaterializer {
    pub fn new(
        repository: Arc<dyn IWorkloadReplicaDeploymentRepository>,
        reconcile_interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if reconcile_interval.is_zero() || batch_size == 0 || batch_size > 10_000 {
            return Err("replica deployment materialization policy is invalid".into());
        }
        Ok(Self {
            repository,
            reconcile_interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<ReplicaDeploymentMaterializationReport, RepositoryError> {
        let candidates = self
            .repository
            .pending_replica_deployments(self.batch_size)
            .await?;
        let mut report = ReplicaDeploymentMaterializationReport {
            candidates: candidates.len(),
            ..ReplicaDeploymentMaterializationReport::default()
        };
        for candidate in candidates {
            match self
                .repository
                .materialize_replica_deployment(candidate, now)
                .await
            {
                Ok(Some(materialization)) if materialization.created => report.created += 1,
                Ok(Some(_)) => report.replayed += 1,
                Ok(None) => report.skipped += 1,
                Err(error) => report
                    .failures
                    .push(ReplicaDeploymentMaterializationFailure {
                        candidate,
                        message: error.to_string(),
                    }),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.reconcile_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(report) => {
                            for failure in &report.failures {
                                tracing::warn!(
                                    workload_id = %failure.candidate.workload_id,
                                    replica_id = %failure.candidate.replica_id,
                                    replica_generation = failure.candidate.replica_generation,
                                    error = %failure.message,
                                    "replica deployment materialization failed"
                                );
                            }
                            tracing::debug!(
                                candidates = report.candidates,
                                created = report.created,
                                replayed = report.replayed,
                                skipped = report.skipped,
                                failures = report.failures.len(),
                                "replica deployment materialization cycle completed"
                            );
                        }
                        Err(error) => tracing::error!(error = %error, "replica deployment materialization cycle failed"),
                    }
                }
            }
        }
    }
}

pub(crate) fn build_replica_deployment_write(
    candidate: ReplicaDeploymentCandidate,
    workload: &Workload,
    revision: &WorkloadRevision,
    replica: &WorkloadReplica,
    member: &WorkloadReplicaMember,
    requested_at: DateTime<Utc>,
) -> Result<ReplicaDeploymentWrite, String> {
    validate_materialization_context(candidate, workload, revision, replica, member)?;
    let requested_at = canonical_timestamp(
        requested_at
            .max(workload.updated_at)
            .max(revision.created_at)
            .max(replica.updated_at)
            .max(member.updated_at),
    );
    let deployment = Deployment::create(
        replica_deployment_id(candidate),
        workload.organization_id,
        workload.id,
        revision.id,
        replica_operation_id(candidate),
        requested_at,
    );
    let operation = replica_operation(&deployment)?;
    let binding = DeploymentReplicaBinding::create(&deployment, revision, replica, member)?;
    let event = DeploymentRequested::envelope(
        &deployment,
        revision,
        replica_materialization_correlation_id(candidate),
    )
    .map_err(|error| error.to_string())?;
    Ok(ReplicaDeploymentWrite {
        deployment,
        operation,
        binding,
        event,
    })
}

pub(crate) fn materialization_from_existing(
    candidate: ReplicaDeploymentCandidate,
    deployment: Deployment,
) -> Result<ReplicaDeploymentMaterialization, String> {
    if deployment.id != replica_deployment_id(candidate)
        || deployment.operation_id != replica_operation_id(candidate)
        || deployment.organization_id != candidate.organization_id
        || deployment.workload_id != candidate.workload_id
        || deployment.revision_id != candidate.revision_id
    {
        return Err("stored replica deployment does not match its deterministic identity".into());
    }
    let operation = replica_operation(&deployment)?;
    Ok(ReplicaDeploymentMaterialization {
        candidate,
        deployment,
        operation,
        created: false,
    })
}

pub(crate) fn created_materialization(
    candidate: ReplicaDeploymentCandidate,
    write: ReplicaDeploymentWrite,
) -> ReplicaDeploymentMaterialization {
    ReplicaDeploymentMaterialization {
        candidate,
        deployment: write.deployment,
        operation: write.operation,
        created: true,
    }
}

fn validate_materialization_context(
    candidate: ReplicaDeploymentCandidate,
    workload: &Workload,
    revision: &WorkloadRevision,
    replica: &WorkloadReplica,
    member: &WorkloadReplicaMember,
) -> Result<(), String> {
    replica.validate()?;
    member.validate()?;
    if candidate.organization_id != workload.organization_id
        || candidate.workload_id != workload.id
        || candidate.replica_id != replica.id
        || candidate.replica_ordinal != replica.ordinal
        || candidate.revision_id != revision.id
        || candidate.revision_generation != revision.generation
        || candidate.replica_generation != replica.generation
        || workload.desired_state != WorkloadDesiredState::Running
        || revision.workload_id != workload.id
        || replica.organization_id != workload.organization_id
        || replica.project_id != workload.project_id
        || replica.environment_id != workload.environment_id
        || replica.workload_id != workload.id
        || replica.revision_id != revision.id
        || replica.revision_generation != revision.generation
        || replica.lifecycle != WorkloadReplicaLifecycle::Desired
        || member.organization_id != workload.organization_id
        || member.project_id != workload.project_id
        || member.environment_id != workload.environment_id
        || member.workload_id != workload.id
        || member.replica_id != replica.id
        || member.node_id.is_some()
    {
        return Err("replica deployment materialization context is inconsistent".into());
    }
    Ok(())
}

fn replica_operation(deployment: &Deployment) -> Result<OperationRequest, String> {
    Ok(OperationRequest::new(
        deployment.operation_id,
        deployment.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)?,
        serde_json::json!({
            "deploymentId": deployment.id,
            "organizationId": deployment.organization_id,
            "revisionId": deployment.revision_id,
            "workloadId": deployment.workload_id,
        }),
        deployment.requested_at,
    ))
}

fn replica_deployment_id(candidate: ReplicaDeploymentCandidate) -> DeploymentId {
    let name = format!("{DEPLOYMENT_ID_DOMAIN}:{}", candidate.replica_generation);
    DeploymentId::from_uuid(Uuid::new_v5(
        &candidate.replica_id.as_uuid(),
        name.as_bytes(),
    ))
}

fn replica_operation_id(candidate: ReplicaDeploymentCandidate) -> OperationId {
    let deployment_id = replica_deployment_id(candidate);
    OperationId::from_uuid(Uuid::new_v5(
        &deployment_id.as_uuid(),
        OPERATION_ID_DOMAIN.as_bytes(),
    ))
}

fn replica_materialization_correlation_id(candidate: ReplicaDeploymentCandidate) -> Uuid {
    let deployment_id = replica_deployment_id(candidate);
    Uuid::new_v5(&deployment_id.as_uuid(), CORRELATION_ID_DOMAIN.as_bytes())
}
