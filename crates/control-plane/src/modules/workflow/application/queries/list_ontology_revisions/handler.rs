use super::ListOntologyRevisions;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{IOntologyRepository, OntologyRevision};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListOntologyRevisionsHandler {
    repository: Arc<dyn IOntologyRepository>,
}

impl ListOntologyRevisionsHandler {
    pub fn new(repository: Arc<dyn IOntologyRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListOntologyRevisions> for ListOntologyRevisionsHandler {
    fn execute(
        &self,
        query: ListOntologyRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<OntologyRevision>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if let Err(error) = resource_access::ontology(
                repository.as_ref(),
                query.organization_id,
                query.ontology_id,
                &query.resource_access,
            )
            .await
            {
                return Ok(Err(error));
            }
            Ok(repository
                .list_revisions(query.organization_id, query.ontology_id)
                .await
                .map_err(Into::into))
        })
    }
}
