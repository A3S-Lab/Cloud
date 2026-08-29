use super::AuthorizePrivilegedAccess;
use crate::modules::identity::domain::repositories::IPrivilegedAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::PrivilegedAuthorizationDecisionRequest;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::AuthorizationDecisionRef;
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct AuthorizePrivilegedAccessHandler {
    repository: Arc<dyn IPrivilegedAuthorizationDecisionRepository>,
}

impl AuthorizePrivilegedAccessHandler {
    pub fn new(repository: Arc<dyn IPrivilegedAuthorizationDecisionRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<AuthorizePrivilegedAccess> for AuthorizePrivilegedAccessHandler {
    fn execute(
        &self,
        command: AuthorizePrivilegedAccess,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AuthorizationDecisionRef>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let request = PrivilegedAuthorizationDecisionRequest {
                principal_id: command.principal_id,
                credential_id: command.credential_id,
                platform_permission: command.platform_permission,
                support_permission: command.support_permission,
                support_grant_id: command.support_grant_id,
                action: command.action,
                scope: command.scope,
                resource_id: command.resource_id,
                request_id: command.request_id,
            };
            if let Err(error) = request.validate() {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            match repository.authorize_privileged(request).await {
                Ok(reference) => Ok(Ok(reference)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::PlatformPermission;
    use crate::modules::shared_kernel::domain::{
        ApiTokenId, InstallationId, PrincipalId, RepositoryError, ScopeContext, Sha256Digest,
    };
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use uuid::Uuid;

    struct RecordingAuthority {
        requests: Mutex<Vec<PrivilegedAuthorizationDecisionRequest>>,
        result: Result<AuthorizationDecisionRef, RepositoryError>,
    }

    #[async_trait]
    impl IPrivilegedAuthorizationDecisionRepository for RecordingAuthority {
        async fn authorize_privileged(
            &self,
            request: PrivilegedAuthorizationDecisionRequest,
        ) -> Result<AuthorizationDecisionRef, RepositoryError> {
            self.requests.lock().expect("requests").push(request);
            self.result.clone()
        }
    }

    fn command() -> AuthorizePrivilegedAccess {
        AuthorizePrivilegedAccess {
            principal_id: PrincipalId::new(),
            credential_id: ApiTokenId::new(),
            platform_permission: PlatformPermission::OperationsRead,
            support_permission: None,
            support_grant_id: None,
            action: "identity.privileged-access.test".into(),
            scope: ScopeContext::installation(InstallationId::new()).expect("scope"),
            resource_id: Uuid::now_v7(),
            request_id: Uuid::now_v7(),
        }
    }

    fn context() -> CqrsContext {
        CqrsContext::new(ModuleRef::new())
    }

    #[tokio::test]
    async fn handler_forwards_one_valid_request_to_the_identity_authority() {
        let reference = AuthorizationDecisionRef::new(
            "urn:a3s:test:privileged-decision",
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
        )
        .expect("reference");
        let authority = Arc::new(RecordingAuthority {
            requests: Mutex::new(Vec::new()),
            result: Ok(reference.clone()),
        });
        let handler = AuthorizePrivilegedAccessHandler::new(authority.clone());
        let command = command();

        let accepted = handler
            .execute(command.clone(), context())
            .await
            .expect("boot")
            .expect("application");

        assert_eq!(accepted, reference);
        let requests = authority.requests.lock().expect("requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].principal_id, command.principal_id);
        assert_eq!(requests[0].credential_id, command.credential_id);
        assert_eq!(requests[0].request_id, command.request_id);
    }

    #[tokio::test]
    async fn invalid_requests_fail_before_the_authority_is_called() {
        let authority = Arc::new(RecordingAuthority {
            requests: Mutex::new(Vec::new()),
            result: Err(RepositoryError::Forbidden("must not be called".into())),
        });
        let handler = AuthorizePrivilegedAccessHandler::new(authority.clone());
        let mut invalid = command();
        invalid.resource_id = Uuid::nil();

        let rejected = handler
            .execute(invalid, context())
            .await
            .expect("boot")
            .expect_err("invalid request");

        assert!(matches!(rejected, ApplicationError::Invalid(_)));
        assert!(authority.requests.lock().expect("requests").is_empty());
    }
}
