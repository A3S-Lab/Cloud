use super::ReconcileGithubConnectionLifecycle;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::domain::{
    GithubConnectionLifecycleAcceptance, IGithubConnectionRepository,
    ReconcileGithubConnectionLifecycle as PersistGithubConnectionLifecycle,
};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct ReconcileGithubConnectionLifecycleHandler {
    connections: Arc<dyn IGithubConnectionRepository>,
}

impl ReconcileGithubConnectionLifecycleHandler {
    pub fn new(connections: Arc<dyn IGithubConnectionRepository>) -> Self {
        Self { connections }
    }
}

impl CommandHandler<ReconcileGithubConnectionLifecycle>
    for ReconcileGithubConnectionLifecycleHandler
{
    fn execute(
        &self,
        command: ReconcileGithubConnectionLifecycle,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<GithubConnectionLifecycleAcceptance>>,
    > {
        let connections = Arc::clone(&self.connections);
        Box::pin(async move {
            match connections
                .reconcile_lifecycle(PersistGithubConnectionLifecycle {
                    lifecycle: command.lifecycle,
                    correlation_id: command.request_id,
                    received_at: command.received_at,
                })
                .await
            {
                Ok(acceptance) => Ok(Ok(acceptance)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
