use super::GetOntology;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
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
            Ok(
                match repository
                    .find(query.organization_id, query.ontology_id)
                    .await
                {
                    Ok(Some(value)) => Ok(value),
                    Ok(None) => Err(ApplicationError::NotFound("Ontology not found".into())),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
