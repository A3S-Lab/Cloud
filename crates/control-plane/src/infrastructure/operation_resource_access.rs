use crate::access_projection::artifact_access;
use crate::access_projection::workload_access;
use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::IAgentRepository;
use crate::modules::artifacts::application::resource_access::BuildRunResourceAccess;
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::executions::application::resource_access::ExecutionResourceAccess;
use crate::modules::executions::domain::IExecutionRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::operations::application::resource_access::IOperationResourceAccess;
use crate::modules::operations::domain::value_objects::OperationSubject;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AgentExecutionId, BuildRunId, DeploymentId, ExecutionId, OrganizationId, WorkflowRunId,
    WorkloadId,
};
use crate::modules::workflow::application::resource_access::workflow_run;
use crate::modules::workflow::domain::IWorkflowRunRepository;
use crate::modules::workloads::application::WorkloadResourceResolver;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use async_trait::async_trait;
use std::sync::Arc;

/// Composition adapter for the closed set of Operation subject kinds emitted by A3S Cloud.
///
/// Each branch delegates ownership resolution to the bounded context that owns the subject. This
/// adapter is the only polymorphic dispatch point; it does not copy ownership into Operations or
/// Identity.
pub(crate) struct OperationResourceAccessResolver {
    workloads: WorkloadResourceResolver,
    builds: BuildRunResourceAccess,
    executions: ExecutionResourceAccess,
    agents: AgentResourceAccess,
    workflow_runs: Arc<dyn IWorkflowRunRepository>,
}

impl OperationResourceAccessResolver {
    pub(crate) fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        builds: Arc<dyn IBuildRunRepository>,
        executions: Arc<dyn IExecutionRepository>,
        agents: Arc<dyn IAgentRepository>,
        workflow_runs: Arc<dyn IWorkflowRunRepository>,
    ) -> Self {
        Self {
            workloads: WorkloadResourceResolver::new(workloads),
            builds: BuildRunResourceAccess::new(builds),
            executions: ExecutionResourceAccess::new(executions),
            agents: AgentResourceAccess::new(agents),
            workflow_runs,
        }
    }
}

#[async_trait]
impl IOperationResourceAccess for OperationResourceAccessResolver {
    async fn subject_is_visible(
        &self,
        organization_id: OrganizationId,
        subject: &OperationSubject,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<bool> {
        let workloads_access = workload_access(evaluator);
        match OperationSubjectKind::parse(subject.kind()) {
            Some(OperationSubjectKind::Workload) => visible(
                self.workloads
                    .workload(
                        organization_id,
                        WorkloadId::from_uuid(subject.id()),
                        &workloads_access,
                    )
                    .await,
            ),
            Some(OperationSubjectKind::Deployment) => visible(
                self.workloads
                    .deployment(
                        organization_id,
                        DeploymentId::from_uuid(subject.id()),
                        &workloads_access,
                    )
                    .await,
            ),
            Some(OperationSubjectKind::BuildRun) => {
                let access = artifact_access(evaluator);
                visible(
                    self.builds
                        .build_run(
                            organization_id,
                            BuildRunId::from_uuid(subject.id()),
                            &access,
                            "build run not found",
                        )
                        .await,
                )
            }
            Some(OperationSubjectKind::Execution) => visible(
                self.executions
                    .execution(
                        organization_id,
                        ExecutionId::from_uuid(subject.id()),
                        evaluator,
                    )
                    .await,
            ),
            Some(OperationSubjectKind::AgentExecution) => visible(
                self.agents
                    .execution(
                        organization_id,
                        AgentExecutionId::from_uuid(subject.id()),
                        evaluator,
                    )
                    .await,
            ),
            Some(OperationSubjectKind::WorkflowRun) => visible(
                workflow_run(
                    self.workflow_runs.as_ref(),
                    organization_id,
                    WorkflowRunId::from_uuid(subject.id()),
                    evaluator,
                )
                .await,
            ),
            None => Ok(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationSubjectKind {
    Workload,
    Deployment,
    BuildRun,
    Execution,
    AgentExecution,
    WorkflowRun,
}

impl OperationSubjectKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "workload" => Some(Self::Workload),
            "deployment" => Some(Self::Deployment),
            "build_run" => Some(Self::BuildRun),
            "execution" => Some(Self::Execution),
            "agent_execution" => Some(Self::AgentExecution),
            "workflow_run" => Some(Self::WorkflowRun),
            _ => None,
        }
    }
}

fn visible<T>(result: ApplicationResult<T>) -> ApplicationResult<bool> {
    match result {
        Ok(_) => Ok(true),
        Err(ApplicationError::NotFound(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_kind_dispatch_is_closed_over_every_production_owner() {
        assert_eq!(
            [
                "workload",
                "deployment",
                "build_run",
                "execution",
                "agent_execution",
                "workflow_run",
            ]
            .into_iter()
            .filter_map(OperationSubjectKind::parse)
            .collect::<Vec<_>>(),
            vec![
                OperationSubjectKind::Workload,
                OperationSubjectKind::Deployment,
                OperationSubjectKind::BuildRun,
                OperationSubjectKind::Execution,
                OperationSubjectKind::AgentExecution,
                OperationSubjectKind::WorkflowRun,
            ]
        );
        assert_eq!(OperationSubjectKind::parse("future_subject"), None);
    }

    #[test]
    fn missing_or_denied_owners_are_hidden_while_repository_failures_propagate() {
        assert_eq!(visible::<()>(Ok(())), Ok(true));
        assert_eq!(
            visible::<()>(Err(ApplicationError::NotFound("hidden".into()))),
            Ok(false)
        );
        assert!(matches!(
            visible::<()>(Err(ApplicationError::Unavailable("offline".into()))),
            Err(ApplicationError::Unavailable(_))
        ));
    }
}
