use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{AgentExecutionEvent, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use serde::Serialize;
use std::sync::Arc;

pub const MAX_AGENT_EXECUTION_TRAJECTORY_PAGE_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct GetAgentExecutionTrajectory {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub resource_access: ResourceAccessEvaluator,
    pub after_sequence: Option<u64>,
    pub through_sequence: Option<u64>,
    pub limit: usize,
}

impl Query for GetAgentExecutionTrajectory {
    type Output = ApplicationResult<AgentExecutionTrajectoryPage>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentExecutionTrajectoryPage {
    pub execution_id: AgentExecutionId,
    pub records: Vec<AgentExecutionEvent>,
    pub next_after_sequence: Option<u64>,
}

pub struct GetAgentExecutionTrajectoryHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl GetAgentExecutionTrajectoryHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<GetAgentExecutionTrajectory> for GetAgentExecutionTrajectoryHandler {
    fn execute(
        &self,
        query: GetAgentExecutionTrajectory,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AgentExecutionTrajectoryPage>>,
    > {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if query.limit == 0 || query.limit > MAX_AGENT_EXECUTION_TRAJECTORY_PAGE_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "Agent execution trajectory limit must be between 1 and {MAX_AGENT_EXECUTION_TRAJECTORY_PAGE_LIMIT}"
                ))));
            }
            if query.after_sequence == Some(0)
                || query.through_sequence == Some(0)
                || query
                    .after_sequence
                    .zip(query.through_sequence)
                    .is_some_and(|(after, through)| after >= through)
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent execution trajectory sequence range is invalid".into(),
                )));
            }
            if let Err(error) = AgentResourceAccess::new(Arc::clone(&agents))
                .execution(
                    query.organization_id,
                    query.execution_id,
                    &query.resource_access,
                )
                .await
            {
                return Ok(Err(error));
            }
            let fetch_limit = query.limit.checked_add(1).ok_or_else(|| {
                a3s_boot::BootError::Internal(
                    "Agent execution trajectory page limit overflowed".into(),
                )
            })?;
            let mut records = match agents
                .list_execution_trajectory_events(
                    query.organization_id,
                    query.execution_id,
                    query.after_sequence,
                    query.through_sequence,
                    fetch_limit,
                )
                .await
            {
                Ok(records) => records,
                Err(error) => return Ok(Err(error.into())),
            };
            let has_more = records.len() > query.limit;
            records.truncate(query.limit);
            let next_after_sequence = if has_more {
                records.last().map(|event| event.sequence)
            } else {
                None
            };
            Ok(Ok(AgentExecutionTrajectoryPage {
                execution_id: query.execution_id,
                records,
                next_after_sequence,
            }))
        })
    }
}
