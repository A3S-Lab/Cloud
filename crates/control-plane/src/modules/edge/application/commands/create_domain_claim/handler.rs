use super::{CreateDomainClaim, CreateDomainClaimResult};
use crate::modules::edge::application::{EdgeEnvironmentScope, IEdgeEnvironmentAccess};
use crate::modules::edge::domain::events::DomainClaimChanged;
use crate::modules::edge::domain::repositories::{CreateDomainClaimWrite, IEdgeRepository};
use crate::modules::edge::domain::{DomainClaim, DomainNamePattern};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{DomainClaimId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use std::sync::Arc;

pub struct CreateDomainClaimHandler {
    environments: Arc<dyn IEdgeEnvironmentAccess>,
    edge: Arc<dyn IEdgeRepository>,
}

impl CreateDomainClaimHandler {
    pub fn new(
        environments: Arc<dyn IEdgeEnvironmentAccess>,
        edge: Arc<dyn IEdgeRepository>,
    ) -> Self {
        Self { environments, edge }
    }
}

impl CommandHandler<CreateDomainClaim> for CreateDomainClaimHandler {
    fn execute(
        &self,
        command: CreateDomainClaim,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CreateDomainClaimResult>>>
    {
        let environments = Arc::clone(&self.environments);
        let edge = Arc::clone(&self.edge);
        Box::pin(async move {
            if !command
                .access
                .environment_is_visible(command.project_id, command.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "domain claims not found".into(),
                )));
            }
            let environment_scope = match EdgeEnvironmentScope::new(
                command.organization_id,
                command.project_id,
                command.environment_id,
            ) {
                Ok(scope) => scope,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match environments.environment_exists(environment_scope).await {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let pattern = match DomainNamePattern::parse(command.pattern) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "project_id": command.project_id,
                "environment_id": command.environment_id,
                "pattern": pattern.as_str(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/domain-claims",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let mut random = [0_u8; 32];
            getrandom::fill(&mut random).map_err(|error| {
                BootError::Internal(format!(
                    "could not generate domain ownership challenge: {error}"
                ))
            })?;
            let challenge = format!("a3s-cloud-verification={}", URL_SAFE_NO_PAD.encode(random));
            let claim = match DomainClaim::create(
                DomainClaimId::new(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                pattern,
                challenge,
                command.requested_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = DomainClaimChanged::envelope(&claim, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let write = match edge
                .create_domain_claim(CreateDomainClaimWrite {
                    claim,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateDomainClaimResult {
                claim: write.value,
                replayed: write.replayed,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::application::{EdgeAccess, EdgeAccessScope};
    use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, RepositoryError,
    };
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use uuid::Uuid;

    struct TrackingEnvironmentAccess {
        called: AtomicBool,
    }

    #[async_trait]
    impl IEdgeEnvironmentAccess for TrackingEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: EdgeEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            self.called.store(true, Ordering::SeqCst);
            Ok(true)
        }
    }

    #[tokio::test]
    async fn create_domain_claim_fails_closed_before_environment_lookup_without_environment_visibility()
     {
        let environments = Arc::new(TrackingEnvironmentAccess {
            called: AtomicBool::new(false),
        });
        let handler = CreateDomainClaimHandler::new(
            Arc::clone(&environments) as Arc<dyn IEdgeEnvironmentAccess>,
            Arc::new(InMemoryEdgeRepository::new()),
        );
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let result = handler
            .execute(
                CreateDomainClaim {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id,
                    access: EdgeAccess::restricted([EdgeAccessScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    }]),
                    pattern: "example.com".into(),
                    idempotency_key: "deny-claim".into(),
                    request_id: Uuid::now_v7(),
                    requested_at: Utc::now(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "domain claims not found"
        ));
        assert!(!environments.called.load(Ordering::SeqCst));
    }
}
