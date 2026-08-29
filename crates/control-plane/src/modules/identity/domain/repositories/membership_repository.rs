use crate::modules::identity::domain::entities::{IdentityPrincipal, Membership};
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, MembershipId, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipRecord {
    pub principal: IdentityPrincipal,
    pub membership: Membership,
}

#[derive(Debug, Clone)]
pub struct CreateMembershipWrite {
    pub principal: IdentityPrincipal,
    pub membership: Membership,
    pub events: [DomainEventEnvelope; 2],
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ChangeMembershipRoleWrite {
    pub organization_id: OrganizationId,
    pub membership_id: MembershipId,
    pub role: MembershipRole,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub changed_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct RevokeMembershipWrite {
    pub organization_id: OrganizationId,
    pub membership_id: MembershipId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IMembershipRepository: Send + Sync {
    async fn create_membership(
        &self,
        write: CreateMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError>;

    async fn find_membership(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
    ) -> Result<Option<MembershipRecord>, RepositoryError>;

    async fn list_memberships(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<MembershipRecord>, RepositoryError>;

    async fn find_active_membership_by_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<Membership>, RepositoryError>;

    async fn change_membership_role(
        &self,
        write: ChangeMembershipRoleWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError>;

    async fn revoke_membership(
        &self,
        write: RevokeMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError>;
}
