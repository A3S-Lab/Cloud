use super::CreateOntology;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, OntologyId, OntologyRevisionId};
use crate::modules::workflow::application::OntologyMutationResult;
use crate::modules::workflow::domain::{
    CreateOntologyWrite, IOntologyRepository, Ontology, OntologyContract, OntologyName,
    OntologyRecord, OntologyRevision, OntologyRevisionPublished,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateOntologyHandler {
    projects: Arc<dyn IProjectRepository>,
    ontologies: Arc<dyn IOntologyRepository>,
}

impl CreateOntologyHandler {
    pub fn new(
        projects: Arc<dyn IProjectRepository>,
        ontologies: Arc<dyn IOntologyRepository>,
    ) -> Self {
        Self {
            projects,
            ontologies,
        }
    }
}

impl CommandHandler<CreateOntology> for CreateOntologyHandler {
    fn execute(
        &self,
        command: CreateOntology,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<OntologyMutationResult>>>
    {
        let projects = Arc::clone(&self.projects);
        let ontologies = Arc::clone(&self.ontologies);
        Box::pin(async move {
            match projects
                .find(command.organization_id, command.project_id)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => return Ok(Err(ApplicationError::NotFound("project not found".into()))),
                Err(error) => return Ok(Err(error.into())),
            }
            let contract = match OntologyContract::parse_acl(&command.acl) {
                Ok(contract) => contract,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let name = match OntologyName::parse(contract.spec().name.clone()) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "contentDigest": contract.digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/ontologies",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let now = Utc::now();
            let ontology_id = OntologyId::new();
            let revision_id = OntologyRevisionId::new();
            let revision = OntologyRevision::initial(
                command.organization_id,
                command.project_id,
                ontology_id,
                revision_id,
                contract.clone(),
                command.actor_principal_id,
                now,
            );
            let ontology = match Ontology::create(
                command.organization_id,
                command.project_id,
                ontology_id,
                name,
                contract.spec().description.clone(),
                revision_id,
                contract.digest().clone(),
                command.actor_principal_id,
                revision.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event =
                OntologyRevisionPublished::created(&ontology, &revision, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match ontologies
                .create(CreateOntologyWrite {
                    record: OntologyRecord { ontology, revision },
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(OntologyMutationResult {
                record: result.value,
                diff: None,
                replayed: result.replayed,
            }))
        })
    }
}
