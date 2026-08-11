use super::request::request_id;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, ProjectId,
};
use crate::modules::workflow::application::queries::diff_ontology_revisions::DiffOntologyRevisions;
use crate::modules::workflow::application::queries::get_ontology::GetOntology;
use crate::modules::workflow::application::queries::get_ontology_revision::GetOntologyRevision;
use crate::modules::workflow::application::queries::list_ontologies::ListOntologies;
use crate::modules::workflow::application::queries::list_ontology_revisions::ListOntologyRevisions;
use crate::modules::workflow::presentation::dto::{
    OntologyDiffResponse, OntologyResponse, OntologyRevisionResponse,
    OntologyRevisionSummaryResponse,
};
use crate::presentation::application_error_response;
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn ontology_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    let get_bus = Arc::clone(&bus);
    let revisions_bus = Arc::clone(&bus);
    let revision_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/ontologies",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListOntologies {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(values) => BootResponse::json(
                            &values
                                .into_iter()
                                .map(OntologyResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/ontologies/{ontology_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetOntology {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            ontology_id: OntologyId::from_uuid(
                                request.param_as::<Uuid>("ontology_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&OntologyResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/ontologies/{ontology_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&revisions_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListOntologyRevisions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            ontology_id: OntologyId::from_uuid(
                                request.param_as::<Uuid>("ontology_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(values) => BootResponse::json(
                            &values
                                .into_iter()
                                .map(OntologyRevisionSummaryResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/ontologies/{ontology_id}/revisions/{revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&revision_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetOntologyRevision {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            ontology_id: OntologyId::from_uuid(
                                request.param_as::<Uuid>("ontology_id")?,
                            ),
                            revision_id: OntologyRevisionId::from_uuid(
                                request.param_as::<Uuid>("revision_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&OntologyRevisionResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/ontologies/{ontology_id}/revisions/{from_revision_id}/diff/{to_revision_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(DiffOntologyRevisions {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            ontology_id: OntologyId::from_uuid(
                                request.param_as::<Uuid>("ontology_id")?,
                            ),
                            from_revision_id: OntologyRevisionId::from_uuid(
                                request.param_as::<Uuid>("from_revision_id")?,
                            ),
                            to_revision_id: OntologyRevisionId::from_uuid(
                                request.param_as::<Uuid>("to_revision_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(value) => BootResponse::json(&OntologyDiffResponse::from(value)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
