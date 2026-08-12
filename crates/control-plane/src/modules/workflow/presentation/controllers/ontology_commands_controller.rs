use super::request::{
    actor_principal_id, ontology_acl, request_identity, resource_access, revision_control,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{
    with_deferred_resource_scope, DeferredResourceScope, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{OntologyId, OrganizationId, ProjectId};
use crate::modules::workflow::application::commands::create_ontology::CreateOntology;
use crate::modules::workflow::application::commands::revise_ontology::ReviseOntology;
use crate::modules::workflow::presentation::dto::OntologyMutationResponse;
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, RouteDefinition,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn ontology_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ONTOLOGY_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/ontologies",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let acl = ontology_acl(&request)?;
                    let actor_principal_id = actor_principal_id(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateOntology {
                            organization_id,
                            project_id,
                            acl,
                            actor_principal_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &OntologyMutationResponse::from(result),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/ontologies/{ontology_id}/revisions",
                move |request: BootRequest| {
                    let bus = Arc::clone(&bus);
                    async move {
                        let organization_id =
                            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                        let ontology_id =
                            OntologyId::from_uuid(request.param_as::<Uuid>("ontology_id")?);
                        let resource_access = resource_access(&request)?;
                        let acl = ontology_acl(&request)?;
                        let (expected_version, migration_rule_id) = revision_control(&request)?;
                        let actor_principal_id = actor_principal_id(&request)?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(ReviseOntology {
                                organization_id,
                                ontology_id,
                                resource_access,
                                acl,
                                expected_version,
                                migration_rule_id,
                                actor_principal_id,
                                idempotency_key,
                                request_id,
                            })
                            .await?
                        {
                            Ok(result) => BootResponse::json_with_status(
                                if result.replayed { 200 } else { 201 },
                                &OntologyMutationResponse::from(result),
                            ),
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)
}
