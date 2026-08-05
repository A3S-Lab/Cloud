use crate::modules::agents::domain::IAgentRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::RepositoryError;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub const AGENT_EXECUTION_WORKFLOW_NAME: &str = "cloud.agent-execution";
pub const AGENT_EXECUTION_WORKFLOW_VERSION: &str = "1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentExecutionReconcileReport {
    pub started: usize,
    pub replayed: usize,
    pub failures: Vec<String>,
}

pub struct AgentExecutionReconciler {
    agents: Arc<dyn IAgentRepository>,
    operations: Arc<dyn IOperationRepository>,
    interval: Duration,
    batch_size: usize,
}

impl AgentExecutionReconciler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self {
            agents,
            operations,
            interval: Duration::from_secs(1),
            batch_size: 100,
        }
    }

    pub fn with_schedule(
        agents: Arc<dyn IAgentRepository>,
        operations: Arc<dyn IOperationRepository>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 {
            return Err(
                "Agent execution reconciliation requires a positive interval and batch size".into(),
            );
        }
        Ok(Self {
            agents,
            operations,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        limit: usize,
    ) -> Result<AgentExecutionReconcileReport, RepositoryError> {
        let pending = self.agents.pending_operation_starts(limit.max(1)).await?;
        let mut report = AgentExecutionReconcileReport::default();
        for execution in pending {
            let operation = OperationRequest::new(
                execution.operation_id,
                execution.organization_id,
                OperationSubject::new("agent_execution", execution.id.as_uuid())
                    .map_err(RepositoryError::Storage)?,
                WorkflowIdentity::new(
                    AGENT_EXECUTION_WORKFLOW_NAME,
                    AGENT_EXECUTION_WORKFLOW_VERSION,
                )
                .map_err(RepositoryError::Storage)?,
                json!({
                    "organizationId": execution.organization_id,
                    "executionId": execution.id,
                }),
                execution.requested_at,
            );
            match self.operations.enqueue(operation).await {
                Ok(write) if write.replayed => report.replayed += 1,
                Ok(_) => report.started += 1,
                Err(error) => report.failures.push(format!(
                    "could not enqueue Agent execution {} operation: {error}",
                    execution.id
                )),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(self.batch_size).await {
                        Ok(report) => {
                            for error in report.failures {
                                tracing::warn!(error = %error, "Agent execution reconciliation failed");
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Agent execution reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }
}
