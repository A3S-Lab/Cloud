use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    WorkflowRunId,
};
use crate::modules::workflow::domain::{
    execution_failure_output, WorkflowRun, WorkflowStepKind, WorkflowStepProjection,
    WorkflowStepProjectionStatus,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunRecord {
    pub run: WorkflowRun,
    pub steps: Vec<WorkflowStepProjection>,
}

impl WorkflowRunRecord {
    pub fn validate(&self) -> Result<(), String> {
        self.run.validate()?;
        if self.steps.len() != self.run.execution_input.plan.steps.len() {
            return Err("WorkflowRun step projection count does not match its plan".into());
        }
        let resolved_steps = self.run.execution_input.resolved_steps()?;
        for planned in &self.run.execution_input.plan.steps {
            let projected = self
                .steps
                .iter()
                .find(|step| step.step_id == planned.id)
                .ok_or_else(|| {
                    format!(
                        "WorkflowRun is missing the projection for step {:?}",
                        planned.id
                    )
                })?;
            projected.validate()?;
            if projected.organization_id != self.run.organization_id
                || projected.project_id != self.run.project_id
                || projected.workflow_run_id != self.run.id
                || projected.kind != planned.kind
                || projected.last_flow_sequence > self.run.last_flow_sequence
            {
                return Err(format!(
                    "WorkflowRun step {:?} projection drifted from its run",
                    planned.id
                ));
            }
            if let Some(evidence) = projected.default_output_evidence.as_ref() {
                let resolved = resolved_steps
                    .iter()
                    .find(|step| step.plan.id == planned.id)
                    .ok_or_else(|| {
                        format!(
                            "WorkflowRun step {:?} lost its resolved default-output authority",
                            planned.id
                        )
                    })?;
                evidence.validate(resolved)?;
                let material = resolved
                    .policy
                    .as_ref()
                    .and_then(|policy| policy.default_output.as_ref())
                    .ok_or_else(|| {
                        format!(
                            "WorkflowRun step {:?} default-output projection lost policy material",
                            planned.id
                        )
                    })?;
                if projected.result.as_ref() != Some(&material.value)
                    || projected.result_digest.as_ref() != Some(&material.digest)
                {
                    return Err(format!(
                        "WorkflowRun step {:?} default-output projection drifted from its exact policy",
                        planned.id
                    ));
                }
            }
            if let Some(handle) = projected.selected_handle.as_deref() {
                let declared = self.run.execution_input.plan.edges.iter().any(|edge| {
                    edge.source == planned.id && edge.source_handle.as_deref() == Some(handle)
                });
                if !declared {
                    return Err(format!(
                        "WorkflowRun step {:?} projection selected an undeclared plan handle",
                        planned.id
                    ));
                }
                if planned.kind == WorkflowStepKind::Execution {
                    let expected = planned
                        .failure
                        .as_ref()
                        .ok_or_else(|| {
                            format!(
                                "WorkflowRun Execution step {:?} selected a handle without failure semantics",
                                planned.id
                            )
                        })
                        .and_then(execution_failure_output)?;
                    if projected.status != WorkflowStepProjectionStatus::Failed
                        || expected.name != handle
                    {
                        return Err(format!(
                            "WorkflowRun Execution step {:?} failure projection drifted from its plan",
                            planned.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CreateWorkflowRunWrite {
    pub record: WorkflowRunRecord,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct CancelWorkflowRunWrite {
    pub record: WorkflowRunRecord,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkflowRunWriteReference {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
}

#[async_trait]
pub trait IWorkflowRunRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRunRecord>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
    ) -> Result<Option<WorkflowRunRecord>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<WorkflowRunRecord>, RepositoryError>;

    async fn request_cancellation(
        &self,
        write: CancelWorkflowRunWrite,
    ) -> Result<IdempotentWrite<WorkflowRunRecord>, RepositoryError>;

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<WorkflowRunRecord>, RepositoryError>;

    async fn pending_reconciliation(
        &self,
        limit: usize,
    ) -> Result<Vec<WorkflowRunRecord>, RepositoryError>;

    async fn save_projection(
        &self,
        record: WorkflowRunRecord,
        expected_version: u64,
    ) -> Result<WorkflowRunRecord, RepositoryError>;
}
