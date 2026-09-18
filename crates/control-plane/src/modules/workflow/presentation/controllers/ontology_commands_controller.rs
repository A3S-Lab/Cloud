use super::request::{
    actor_principal_id, ontology_acl, request_identity, revision_control, workflow_access,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{OntologyId, OrganizationId, ProjectId};
use crate::modules::workflow::application::commands::create_ontology::CreateOntology;
use crate::modules::workflow::application::commands::revise_ontology::ReviseOntology;
use crate::modules::workflow::presentation::dto::OntologyMutationResponse;
use crate::presentation::{with_deferred_resource_scope, DeferredResourceScope, OrganizationTenantGuard, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn ontology_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    // Nest macros own create; revise stays deferred-scoped on the builder route
    // path because deferred resource scope is not a Nest attribute today.
    let revise_bus = Arc::clone(&bus);
    Arc::new(OntologyCommandsController { bus })
        .controller()?
        .route(with_deferred_resource_scope(
            RouteDefinition::post(
                "/{organization_id}/ontologies/{ontology_id}/revisions",
                move |request: BootRequest| {
                    let bus = Arc::clone(&revise_bus);
                    async move {
                        let organization_id =
                            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                        let ontology_id =
                            OntologyId::from_uuid(request.param_as::<Uuid>("ontology_id")?);
                        let access = workflow_access(&request)?;
                        let acl = ontology_acl(&request)?;
                        let (expected_version, migration_rule_id) = revision_control(&request)?;
                        let actor_principal_id = actor_principal_id(&request)?;
                        let (idempotency_key, request_id) = request_identity(&request)?;
                        match bus
                            .execute(ReviseOntology {
                                organization_id,
                                ontology_id,
                                access,
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

#[derive(Debug, Clone)]
struct OntologyCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::ONTOLOGY_WRITE])]
impl OntologyCommandsController {
    #[post("/{organization_id}/projects/{project_id}/ontologies", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let acl = ontology_acl(&request)?;
        let access = workflow_access(&request)?;
        let actor_principal_id = actor_principal_id(&request)?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateOntology {
                organization_id,
                project_id,
                access,
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
}

#[cfg(test)]
mod nest_macro_ontology_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn ontology_commands_controller_registers_create_via_nest_macros() {
        let controller = ontology_commands_controller(Arc::new(CommandBus::new()))
            .expect("ontology commands nest controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/ontologies"
        );
        assert_eq!(routes[1].method(), HttpMethod::Post);
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/ontologies/{ontology_id}/revisions"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::ONTOLOGY_WRITE])
        );
    }
}
