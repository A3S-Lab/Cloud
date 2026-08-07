use super::{CreateOrganization, CreateOrganizationResult};
use crate::modules::identity::domain::entities::{Membership, Organization};
use crate::modules::identity::domain::events::{MembershipChanged, OrganizationCreated};
use crate::modules::identity::domain::repositories::{
    CreateOrganizationWrite, IOrganizationRepository,
};
use crate::modules::identity::domain::value_objects::{MembershipRole, OrganizationName};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, MembershipId, OrganizationId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateOrganizationHandler {
    repository: Arc<dyn IOrganizationRepository>,
}

impl CreateOrganizationHandler {
    pub fn new(repository: Arc<dyn IOrganizationRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<CreateOrganization> for CreateOrganizationHandler {
    fn execute(
        &self,
        command: CreateOrganization,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CreateOrganizationResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let name = match OrganizationName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({"name": name.as_str()}))
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency =
                match IdempotencyRequest::new("organizations", command.idempotency_key, &canonical)
                {
                    Ok(idempotency) => idempotency,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let organization = Organization::create(OrganizationId::new(), name, Utc::now());
            let owner_membership = Membership::create(
                MembershipId::new(),
                organization.id,
                command.actor_principal_id,
                MembershipRole::Owner,
                organization.created_at,
            );
            let organization_event =
                OrganizationCreated::envelope(&organization, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let membership_event =
                MembershipChanged::created(&owner_membership, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .create(CreateOrganizationWrite {
                    organization,
                    owner_membership,
                    events: [organization_event, membership_event],
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => result,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateOrganizationResult {
                organization: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
