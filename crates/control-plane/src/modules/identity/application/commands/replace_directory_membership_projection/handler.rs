use super::ReplaceDirectoryMembershipProjection;
use crate::modules::identity::application::DirectoryMembershipProjectionMutationResult;
use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use crate::modules::identity::domain::events::DirectoryMembershipProjectionChanged;
use crate::modules::identity::domain::repositories::{
    IDirectoryMembershipProjectionRepository, ReplaceDirectoryMembershipProjectionWrite,
};
use crate::modules::identity::domain::value_objects::DirectoryGrantSubjectRef;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, PrincipalId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct ReplaceDirectoryMembershipProjectionHandler {
    repository: Arc<dyn IDirectoryMembershipProjectionRepository>,
}

impl ReplaceDirectoryMembershipProjectionHandler {
    pub fn new(repository: Arc<dyn IDirectoryMembershipProjectionRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<ReplaceDirectoryMembershipProjection>
    for ReplaceDirectoryMembershipProjectionHandler
{
    fn execute(
        &self,
        command: ReplaceDirectoryMembershipProjection,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DirectoryMembershipProjectionMutationResult>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let subject = match DirectoryGrantSubjectRef::parse(command.subject_ref) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let mut principal_ids = command
                .principal_ids
                .into_iter()
                .map(PrincipalId::from_uuid)
                .collect::<Vec<_>>();
            principal_ids.sort();
            principal_ids.dedup();
            let now = Utc::now();
            let bindings = principal_ids
                .iter()
                .copied()
                .map(|principal_id| {
                    DirectoryMembershipProjectionBinding::new(
                        command.organization_id,
                        subject.clone(),
                        principal_id,
                        now,
                    )
                })
                .collect::<Vec<_>>();
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "subjectRef": subject.format_ref(),
                "principalIds": principal_ids
                    .iter()
                    .map(|id| id.as_uuid())
                    .collect::<Vec<_>>(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/directory-membership-projections",
                    command.organization_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = DirectoryMembershipProjectionChanged::replaced(
                command.organization_id,
                &subject,
                &principal_ids,
                now,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .replace_bindings(ReplaceDirectoryMembershipProjectionWrite {
                    organization_id: command.organization_id,
                    subject,
                    bindings,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(DirectoryMembershipProjectionMutationResult {
                bindings: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
