use super::ReviseOntology;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, OntologyRevisionId};
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::application::OntologyMutationResult;
use crate::modules::workflow::domain::{
    diff_ontology_contracts, resolve_migration_policy, IOntologyRepository, OntologyContract,
    OntologyName, OntologyRecord, OntologyRevision, OntologyRevisionPublished, ReviseOntologyWrite,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct ReviseOntologyHandler {
    ontologies: Arc<dyn IOntologyRepository>,
}

impl ReviseOntologyHandler {
    pub fn new(ontologies: Arc<dyn IOntologyRepository>) -> Self {
        Self { ontologies }
    }
}

impl CommandHandler<ReviseOntology> for ReviseOntologyHandler {
    fn execute(
        &self,
        command: ReviseOntology,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<OntologyMutationResult>>>
    {
        let ontologies = Arc::clone(&self.ontologies);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Ontology version must be positive".into(),
                )));
            }
            let target = match OntologyContract::parse_acl(&command.acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let current = match resource_access::ontology(
                ontologies.as_ref(),
                command.organization_id,
                command.ontology_id,
                &command.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let revisions = match ontologies
                .list_revisions(command.organization_id, command.ontology_id)
                .await
            {
                Ok(values) => values,
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(parent) = revisions
                .into_iter()
                .find(|revision| revision.revision_number == command.expected_version)
            else {
                return Ok(Err(ApplicationError::Conflict(
                    "expected Ontology version is not available in this lineage".into(),
                )));
            };
            let diff = diff_ontology_contracts(&parent.contract, &target);
            let migration_policy = match resolve_migration_policy(
                &target,
                &diff,
                command.migration_rule_id.as_deref(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "ontologyId": command.ontology_id,
                "expectedVersion": command.expected_version,
                "contentDigest": target.digest(),
                "migrationPolicy": migration_policy,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/ontologies/{}/revisions",
                    command.organization_id, command.ontology_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let name = match OntologyName::parse(target.spec().name.clone()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let now = Utc::now();
            let revision = match OntologyRevision::successor(
                &parent,
                OntologyRevisionId::new(),
                target.clone(),
                migration_policy,
                command.actor_principal_id,
                now,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let base = current.at_revision(&parent).map_err(|error| {
                BootError::Internal(format!("stored Ontology revision is invalid: {error}"))
            })?;
            let ontology = match base.advance(
                command.expected_version,
                name,
                target.spec().description.clone(),
                revision.id,
                target.digest().clone(),
                revision.created_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event =
                OntologyRevisionPublished::revised(&ontology, &revision, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match ontologies
                .revise(ReviseOntologyWrite {
                    record: OntologyRecord { ontology, revision },
                    expected_version: command.expected_version,
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
                diff: Some(diff),
                replayed: result.replayed,
            }))
        })
    }
}
