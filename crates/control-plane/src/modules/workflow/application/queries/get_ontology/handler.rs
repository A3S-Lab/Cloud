use super::GetOntology;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{IOntologyRepository, Ontology};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetOntologyHandler {
    repository: Arc<dyn IOntologyRepository>,
}

impl GetOntologyHandler {
    pub fn new(repository: Arc<dyn IOntologyRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetOntology> for GetOntologyHandler {
    fn execute(
        &self,
        query: GetOntology,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Ontology>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(resource_access::ontology(
                repository.as_ref(),
                query.organization_id,
                query.ontology_id,
                &query.resource_access,
            )
            .await)
        })
    }
}
