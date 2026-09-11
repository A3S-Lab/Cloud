use crate::modules::notifications::NotificationAccess;
use crate::modules::shared_kernel::domain::{
    MembershipId, OrganizationId, PrincipalId, RepositoryError,
};
use async_trait::async_trait;

/// Notifications-owned port for Identity evidence required by outbox projection.
///
/// Membership and resource-grant repositories stay behind this anti-corruption
/// boundary. Alert authorization returns a synthesized [`NotificationAccess`].
#[async_trait]
pub trait INotificationOutboxIdentityAccess: Send + Sync {
    /// Returns whether a membership lifecycle inbox fact should be projected.
    ///
    /// `Ok(true)` projects, `Ok(false)` skips inactive memberships, and `Err`
    /// signals missing or inconsistent Identity evidence.
    async fn membership_inbox_is_projectable(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
        expected_principal_id: PrincipalId,
    ) -> Result<bool, RepositoryError>;

    /// Returns synthesized notification visibility for an active member, or
    /// `None` when the principal has no active membership.
    async fn alert_access_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<NotificationAccess>, RepositoryError>;
}
