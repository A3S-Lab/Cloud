use crate::modules::identity::{ActiveHumanMembershipScope, IActiveHumanMembershipQueryPort};
use crate::modules::plugins::domain::services::{
    IPluginRegistryEnrollmentAuthorizer, PluginRegistryEnrollmentAuthorization,
    PluginRegistryEnrollmentAuthorizationError,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use async_trait::async_trait;
use std::sync::Arc;

/// The sole anti-corruption adapter from Identity's membership fact to the
/// Plugins-owned registry-enrollment policy.
#[derive(Clone)]
pub struct IdentityPluginRegistryEnrollmentAuthorizerAdapter {
    memberships: Arc<dyn IActiveHumanMembershipQueryPort>,
}

impl IdentityPluginRegistryEnrollmentAuthorizerAdapter {
    pub fn new(memberships: Arc<dyn IActiveHumanMembershipQueryPort>) -> Self {
        Self { memberships }
    }
}

#[async_trait]
impl IPluginRegistryEnrollmentAuthorizer for IdentityPluginRegistryEnrollmentAuthorizerAdapter {
    async fn authorize_enrollment(
        &self,
        organization_id: OrganizationId,
        actor_id: PrincipalId,
    ) -> Result<PluginRegistryEnrollmentAuthorization, PluginRegistryEnrollmentAuthorizationError>
    {
        let scope = ActiveHumanMembershipScope::new(organization_id, actor_id)
            .map_err(PluginRegistryEnrollmentAuthorizationError::Unavailable)?;
        let authorized = self
            .memberships
            .active_human_membership_exists(scope)
            .await
            .map_err(|error| {
                PluginRegistryEnrollmentAuthorizationError::Unavailable(error.to_string())
            })?;
        if !authorized {
            return Err(PluginRegistryEnrollmentAuthorizationError::Forbidden);
        }
        PluginRegistryEnrollmentAuthorization::new(organization_id, actor_id)
            .map_err(PluginRegistryEnrollmentAuthorizationError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::RepositoryError;
    use std::sync::Mutex;

    struct FixedIdentityMembershipQuery {
        outcome: Result<bool, RepositoryError>,
        scopes: Mutex<Vec<ActiveHumanMembershipScope>>,
    }

    impl FixedIdentityMembershipQuery {
        fn new(outcome: Result<bool, RepositoryError>) -> Self {
            Self {
                outcome,
                scopes: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl IActiveHumanMembershipQueryPort for FixedIdentityMembershipQuery {
        async fn active_human_membership_exists(
            &self,
            scope: ActiveHumanMembershipScope,
        ) -> Result<bool, RepositoryError> {
            self.scopes.lock().expect("scope lock").push(scope);
            self.outcome.clone()
        }
    }

    #[tokio::test]
    async fn adapter_projects_one_exact_identity_fact_into_plugins_evidence() {
        let organization_id = OrganizationId::new();
        let actor_id = PrincipalId::new();
        let identity = Arc::new(FixedIdentityMembershipQuery::new(Ok(true)));
        let adapter = IdentityPluginRegistryEnrollmentAuthorizerAdapter::new(identity.clone());

        let authorization = adapter
            .authorize_enrollment(organization_id, actor_id)
            .await
            .expect("authorization");

        assert_eq!(authorization.organization_id(), organization_id);
        assert_eq!(authorization.actor_id(), actor_id);
        assert_eq!(
            identity.scopes.lock().expect("scope lock").as_slice(),
            &[ActiveHumanMembershipScope::new(organization_id, actor_id).expect("scope")]
        );
    }

    #[tokio::test]
    async fn adapter_preserves_denial_and_hides_identity_outages() {
        let organization_id = OrganizationId::new();
        let actor_id = PrincipalId::new();
        let denied = IdentityPluginRegistryEnrollmentAuthorizerAdapter::new(Arc::new(
            FixedIdentityMembershipQuery::new(Ok(false)),
        ));
        let unavailable = IdentityPluginRegistryEnrollmentAuthorizerAdapter::new(Arc::new(
            FixedIdentityMembershipQuery::new(Err(RepositoryError::Storage("fixture".into()))),
        ));

        assert!(matches!(
            denied.authorize_enrollment(organization_id, actor_id).await,
            Err(PluginRegistryEnrollmentAuthorizationError::Forbidden)
        ));
        assert!(matches!(
            unavailable
                .authorize_enrollment(organization_id, actor_id)
                .await,
            Err(PluginRegistryEnrollmentAuthorizationError::Unavailable(_))
        ));
    }
}
