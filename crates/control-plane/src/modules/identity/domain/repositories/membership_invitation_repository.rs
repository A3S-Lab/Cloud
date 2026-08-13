use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, MembershipId, MembershipInvitationId, OrganizationId,
    PrincipalId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::MembershipRecord;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipInvitationAcceptance {
    pub invitation: MembershipInvitation,
    pub membership: MembershipRecord,
}

#[derive(Debug, Clone)]
pub struct CreateMembershipInvitationWrite {
    pub invitation: MembershipInvitation,
    pub event: DomainEventEnvelope,
    pub actor_is_platform_admin: bool,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct AcceptMembershipInvitationWrite {
    pub invitation_id: MembershipInvitationId,
    pub expected_version: u64,
    pub membership_id: MembershipId,
    pub actor_principal_id: PrincipalId,
    pub accepted_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct RevokeMembershipInvitationWrite {
    pub organization_id: OrganizationId,
    pub invitation_id: MembershipInvitationId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub actor_is_platform_admin: bool,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IMembershipInvitationRepository: Send + Sync {
    async fn create_membership_invitation(
        &self,
        write: CreateMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitation>, RepositoryError>;

    async fn find_membership_invitation(
        &self,
        organization_id: OrganizationId,
        invitation_id: MembershipInvitationId,
    ) -> Result<Option<MembershipInvitation>, RepositoryError>;

    async fn list_membership_invitations(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<MembershipInvitation>, RepositoryError>;

    async fn list_membership_invitations_for_principal(
        &self,
        principal_id: PrincipalId,
    ) -> Result<Vec<MembershipInvitation>, RepositoryError>;

    async fn accept_membership_invitation(
        &self,
        write: AcceptMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitationAcceptance>, RepositoryError>;

    async fn revoke_membership_invitation(
        &self,
        write: RevokeMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitation>, RepositoryError>;
}
