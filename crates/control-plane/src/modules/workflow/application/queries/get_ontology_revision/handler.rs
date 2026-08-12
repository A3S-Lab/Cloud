use super::GetOntologyRevision;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{IOntologyRepository, OntologyRevision};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetOntologyRevisionHandler {
    repository: Arc<dyn IOntologyRepository>,
}

impl GetOntologyRevisionHandler {
    pub fn new(repository: Arc<dyn IOntologyRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetOntologyRevision> for GetOntologyRevisionHandler {
    fn execute(
        &self,
        query: GetOntologyRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<OntologyRevision>>> {
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
            Ok(
                match repository
                    .find_revision(query.organization_id, query.ontology_id, query.revision_id)
                    .await
                {
                    Ok(Some(value)) => Ok(value),
                    Ok(None) => Err(ApplicationError::NotFound(
                        "Ontology revision not found".into(),
                    )),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}
