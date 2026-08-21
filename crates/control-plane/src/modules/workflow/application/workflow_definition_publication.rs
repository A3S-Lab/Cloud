use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use crate::modules::workflow::application::{
    WorkflowDefinitionMutationResult, WorkflowPayloadAcl, WorkflowSemanticContractAcls,
};
use crate::modules::workflow::domain::{
    CreateWorkflowDefinitionWrite, IWorkflowDefinitionRepository, WorkflowCompositeRegions,
    WorkflowContract, WorkflowDefinition, WorkflowDefinitionRecord, WorkflowPayload,
    WorkflowRevision, WorkflowRevisionPublished, WorkflowRevisionSemanticContracts,
    WorkflowStepDescriptorBindings, WorkflowStepDescriptorRegistry, WorkflowVariableContract,
    WorkflowVariableDefaults,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

/// Parsed, canonical input for the sole Workflow definition publication path.
///
/// Callers may compile or parse Workflow-owned ACLs, but only this application
/// port may create the aggregate, immutable initial revision, event, and
/// idempotency record. Cross-context adapters therefore never write Workflow
/// tables directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinitionPublicationRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub definition_id: WorkflowDefinitionId,
    pub revision_id: WorkflowRevisionId,
    pub definition_acl: String,
    pub payloads: Vec<WorkflowPayloadAcl>,
    pub semantic_contracts: Option<WorkflowSemanticContractAcls>,
    pub actor_principal_id: PrincipalId,
    pub idempotency_scope: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

#[async_trait]
pub trait IWorkflowDefinitionPublicationPort: Send + Sync {
    async fn publish(
        &self,
        request: WorkflowDefinitionPublicationRequest,
    ) -> ApplicationResult<WorkflowDefinitionMutationResult>;
}

pub struct WorkflowDefinitionPublicationService {
    projects: Arc<dyn IProjectRepository>,
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
}

impl WorkflowDefinitionPublicationService {
    pub fn new(
        projects: Arc<dyn IProjectRepository>,
        workflows: Arc<dyn IWorkflowDefinitionRepository>,
    ) -> Self {
        Self {
            projects,
            workflows,
        }
    }
}

#[async_trait]
impl IWorkflowDefinitionPublicationPort for WorkflowDefinitionPublicationService {
    async fn publish(
        &self,
        request: WorkflowDefinitionPublicationRequest,
    ) -> ApplicationResult<WorkflowDefinitionMutationResult> {
        match self
            .projects
            .find(request.organization_id, request.project_id)
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => return Err(ApplicationError::NotFound("project not found".into())),
            Err(error) => return Err(error.into()),
        }
        if request.definition_id.as_uuid().is_nil()
            || request.revision_id.as_uuid().is_nil()
            || request.actor_principal_id.as_uuid().is_nil()
            || request.request_id.is_nil()
        {
            return Err(ApplicationError::Invalid(
                "Workflow publication identity is invalid".into(),
            ));
        }
        let contract = WorkflowContract::parse_acl(&request.definition_acl)
            .map_err(ApplicationError::Invalid)?;
        let payloads = request
            .payloads
            .into_iter()
            .map(|value| WorkflowPayload::parse_acl(value.kind, &value.acl))
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApplicationError::Invalid)?;
        let semantic_contracts = request
            .semantic_contracts
            .map(|value| parse_semantic_contracts(&contract, value))
            .transpose()
            .map_err(ApplicationError::Invalid)?;
        let now = Utc::now();
        let revision = match semantic_contracts {
            Some(semantic_contracts) => WorkflowRevision::initial_with_semantic_contracts(
                request.organization_id,
                request.project_id,
                request.definition_id,
                request.revision_id,
                contract.clone(),
                payloads,
                semantic_contracts,
                request.actor_principal_id,
                now,
            ),
            None => WorkflowRevision::initial(
                request.organization_id,
                request.project_id,
                request.definition_id,
                request.revision_id,
                contract.clone(),
                payloads,
                request.actor_principal_id,
                now,
            ),
        }
        .map_err(ApplicationError::Invalid)?;
        let canonical = canonical_publication_request(
            request.organization_id,
            request.project_id,
            contract.digest().as_str(),
            revision.payload_set_digest.as_str(),
            revision
                .semantic_contract_set_digest()
                .map(|digest| digest.as_str()),
        )
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            request.idempotency_scope,
            request.idempotency_key,
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        let definition = WorkflowDefinition::create(
            request.organization_id,
            request.project_id,
            request.definition_id,
            contract.spec().name.clone(),
            contract.spec().description.clone(),
            revision.id,
            contract.digest().clone(),
            request.actor_principal_id,
            revision.created_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let event = WorkflowRevisionPublished::created(&definition, &revision, request.request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let result = self
            .workflows
            .create(CreateWorkflowDefinitionWrite {
                record: WorkflowDefinitionRecord {
                    definition,
                    revision,
                },
                event,
                actor_principal_id: request.actor_principal_id,
                request_id: request.request_id,
                idempotency,
            })
            .await?;
        Ok(WorkflowDefinitionMutationResult {
            record: result.value,
            replayed: result.replayed,
        })
    }
}

fn parse_semantic_contracts(
    contract: &WorkflowContract,
    value: WorkflowSemanticContractAcls,
) -> Result<WorkflowRevisionSemanticContracts, String> {
    WorkflowRevisionSemanticContracts::create_with_optional_contracts(
        contract.spec(),
        WorkflowStepDescriptorBindings::parse_acl(&value.descriptor_bindings_acl)?,
        WorkflowStepDescriptorRegistry::parse_acl(&value.descriptor_registry_acl)?,
        WorkflowVariableContract::parse_acl(&value.variable_contract_acl)?,
        value
            .variable_defaults_acl
            .as_deref()
            .map(WorkflowVariableDefaults::parse_acl)
            .transpose()?,
        value
            .composite_regions_acl
            .as_deref()
            .map(WorkflowCompositeRegions::parse_acl)
            .transpose()?,
    )
}

fn canonical_publication_request(
    organization_id: OrganizationId,
    project_id: ProjectId,
    content_digest: &str,
    payload_set_digest: &str,
    semantic_contract_set_digest: Option<&str>,
) -> Result<Vec<u8>, serde_json::Error> {
    // This deliberately retains the byte shape used by the original public
    // CreateWorkflowDefinition handler. In particular, the optional semantic
    // digest remains an explicit JSON null so deployed idempotency records stay
    // replay-compatible after publication moved behind this shared port.
    serde_json::to_vec(&serde_json::json!({
        "organizationId": organization_id,
        "projectId": project_id,
        "contentDigest": content_digest,
        "payloadSetDigest": payload_set_digest,
        "semanticContractSetDigest": semantic_contract_set_digest,
    }))
}

#[cfg(test)]
mod tests {
    use super::canonical_publication_request;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
    use uuid::Uuid;

    #[test]
    fn canonical_request_preserves_the_existing_idempotency_fingerprint_shape() {
        let canonical = canonical_publication_request(
            OrganizationId::from_uuid(
                Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("organization"),
            ),
            ProjectId::from_uuid(
                Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("project"),
            ),
            "content",
            "payload",
            None,
        )
        .expect("canonical publication request");

        assert_eq!(
            String::from_utf8(canonical).expect("UTF-8 JSON"),
            concat!(
                "{\"contentDigest\":\"content\",\"organizationId\":",
                "\"11111111-1111-1111-1111-111111111111\",",
                "\"payloadSetDigest\":\"payload\",\"projectId\":",
                "\"22222222-2222-2222-2222-222222222222\",",
                "\"semanticContractSetDigest\":null}",
            )
        );
    }
}
