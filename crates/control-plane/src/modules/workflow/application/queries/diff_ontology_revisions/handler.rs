use super::{DiffOntologyRevisions, OntologyRevisionDiff};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{diff_ontology_contracts, IOntologyRepository};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct DiffOntologyRevisionsHandler {
    repository: Arc<dyn IOntologyRepository>,
}

impl DiffOntologyRevisionsHandler {
    pub fn new(repository: Arc<dyn IOntologyRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<DiffOntologyRevisions> for DiffOntologyRevisionsHandler {
    fn execute(
        &self,
        query: DiffOntologyRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<OntologyRevisionDiff>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let from = match repository
                .find_revision(
                    query.organization_id,
                    query.ontology_id,
                    query.from_revision_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "source Ontology revision not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let to = match repository
                .find_revision(
                    query.organization_id,
                    query.ontology_id,
                    query.to_revision_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "target Ontology revision not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(OntologyRevisionDiff {
                ontology_id: query.ontology_id,
                from_revision_id: from.id,
                to_revision_id: to.id,
                diff: diff_ontology_contracts(&from.contract, &to.contract),
            }))
        })
    }
}
