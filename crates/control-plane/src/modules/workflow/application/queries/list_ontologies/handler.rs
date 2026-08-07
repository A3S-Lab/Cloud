use super::ListOntologies;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::domain::{IOntologyRepository, Ontology};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListOntologiesHandler {
    repository: Arc<dyn IOntologyRepository>,
}

impl ListOntologiesHandler {
    pub fn new(repository: Arc<dyn IOntologyRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListOntologies> for ListOntologiesHandler {
    fn execute(
        &self,
        query: ListOntologies,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Ontology>>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}
