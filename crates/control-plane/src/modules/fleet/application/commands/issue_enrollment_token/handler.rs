use super::{IssueEnrollmentToken, IssueEnrollmentTokenResult};
use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::fleet::domain::events::EnrollmentTokenIssued;
use crate::modules::fleet::domain::repositories::INodeRepository;
use crate::modules::fleet::domain::value_objects::EnrollmentTokenCredential;
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnrollmentTokenId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct IssueEnrollmentTokenHandler {
    organizations: Arc<dyn IOrganizationRepository>,
    nodes: Arc<dyn INodeRepository>,
}

impl IssueEnrollmentTokenHandler {
    pub fn new(
        organizations: Arc<dyn IOrganizationRepository>,
        nodes: Arc<dyn INodeRepository>,
    ) -> Self {
        Self {
            organizations,
            nodes,
        }
    }
}

impl CommandHandler<IssueEnrollmentToken> for IssueEnrollmentTokenHandler {
    fn execute(
        &self,
        command: IssueEnrollmentToken,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<IssueEnrollmentTokenResult>>>
    {
        let organizations = Arc::clone(&self.organizations);
        let nodes = Arc::clone(&self.nodes);
        Box::pin(async move {
            match organizations.find(command.organization_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "organization not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let credential = match EnrollmentTokenCredential::from_secret(&command.token_secret) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let token = match EnrollmentToken::new(
                EnrollmentTokenId::new(),
                command.organization_id,
                command.name,
                credential,
                command.requested_at,
                command.expires_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "name": token.name,
                "credentialDigest": token.credential.digest(),
                "expiresAt": token.expires_at,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/enrollment-tokens",
                    command.organization_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = EnrollmentTokenIssued::envelope(&token, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match nodes
                .issue_enrollment_token(token, event, idempotency)
                .await
            {
                Ok(result) => Ok(Ok(IssueEnrollmentTokenResult {
                    enrollment_token: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
