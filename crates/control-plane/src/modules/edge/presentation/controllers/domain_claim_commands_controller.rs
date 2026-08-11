use crate::modules::edge::application::{CreateDomainClaim, RevokeDomainClaim, VerifyDomainClaim};
use crate::modules::edge::presentation::dto::{
    CreateDomainClaimRequest, DomainClaimMutationResponse, RevokeDomainClaimRequest,
    VerifyDomainClaimRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn domain_claim_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let verify_bus = Arc::clone(&bus);
    let revoke_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ROUTE_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/domain-claims",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: CreateDomainClaimRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateDomainClaim {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            pattern: body.pattern,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 201 },
                            &DomainClaimMutationResponse::new(result.claim, result.replayed),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/domain-claims/{claim_id}/verify",
            move |request: BootRequest| {
                let bus = Arc::clone(&verify_bus);
                async move {
                    let body: VerifyDomainClaimRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(VerifyDomainClaim {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            claim_id: DomainClaimId::from_uuid(
                                request.param_as::<Uuid>("claim_id")?,
                            ),
                            proof: body.proof,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 202 },
                            &DomainClaimMutationResponse::new(result.claim, result.replayed),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/domain-claims/{claim_id}/revoke",
            move |request: BootRequest| {
                let bus = Arc::clone(&revoke_bus);
                async move {
                    let body: RevokeDomainClaimRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(RevokeDomainClaim {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            claim_id: DomainClaimId::from_uuid(
                                request.param_as::<Uuid>("claim_id")?,
                            ),
                            reason: body.reason,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json_with_status(
                            if result.replayed { 200 } else { 202 },
                            &DomainClaimMutationResponse::new(result.claim, result.replayed),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

use super::request::request_identity;
