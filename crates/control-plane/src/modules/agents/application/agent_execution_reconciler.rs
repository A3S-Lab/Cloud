use crate::modules::agents::application::{
    AgentExecutionOperationRequest, IAgentExecutionOperationScheduler,
};
use crate::modules::agents::domain::IAgentRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
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
    operation_scheduler: Arc<dyn IAgentExecutionOperationScheduler>,
    interval: Duration,
    batch_size: usize,
}

impl AgentExecutionReconciler {
    pub fn from_operation_scheduler(
        agents: Arc<dyn IAgentRepository>,
        operation_scheduler: Arc<dyn IAgentExecutionOperationScheduler>,
    ) -> Self {
        Self {
            agents,
            operation_scheduler,
            interval: Duration::from_secs(1),
            batch_size: 100,
        }
    }

    pub fn with_operation_scheduler_and_schedule(
        agents: Arc<dyn IAgentRepository>,
        operation_scheduler: Arc<dyn IAgentExecutionOperationScheduler>,
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
            operation_scheduler,
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
            let request = AgentExecutionOperationRequest::new(
                execution.operation_id,
                execution.organization_id,
                execution.id,
                execution.requested_at,
            );
            match self.operation_scheduler.schedule(request).await {
                Ok(outcome) if outcome.replayed() => report.replayed += 1,
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
