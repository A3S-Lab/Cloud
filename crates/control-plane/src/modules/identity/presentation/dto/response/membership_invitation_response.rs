use crate::modules::identity::application::{
    MembershipInvitationAcceptanceResult, MembershipInvitationMutationResult,
};
use crate::modules::identity::domain::entities::MembershipInvitation;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::MembershipResponse;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipInvitationResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub principal_id: Uuid,
    pub role: String,
    pub invited_by_principal_id: Uuid,
    pub status: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_membership_id: Option<Uuid>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<MembershipInvitation> for MembershipInvitationResponse {
    fn from(invitation: MembershipInvitation) -> Self {
        Self {
            id: invitation.id.as_uuid(),
            organization_id: invitation.organization_id.as_uuid(),
            principal_id: invitation.principal_id.as_uuid(),
            role: invitation.role.as_str().to_owned(),
            invited_by_principal_id: invitation.invited_by_principal_id.as_uuid(),
            status: invitation.status_at(Utc::now()).as_str().to_owned(),
            aggregate_version: invitation.aggregate_version,
            created_at: invitation.created_at,
            updated_at: invitation.updated_at,
            expires_at: invitation.expires_at,
            accepted_membership_id: invitation.accepted_membership_id.map(|id| id.as_uuid()),
            accepted_at: invitation.accepted_at,
            revoked_at: invitation.revoked_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipInvitationMutationResponse {
    #[serde(flatten)]
    pub invitation: MembershipInvitationResponse,
    pub replayed: bool,
}

impl From<MembershipInvitationMutationResult> for MembershipInvitationMutationResponse {
    fn from(result: MembershipInvitationMutationResult) -> Self {
        Self {
            invitation: result.invitation.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipInvitationAcceptanceResponse {
    pub invitation: MembershipInvitationResponse,
    pub membership: MembershipResponse,
    pub replayed: bool,
}

impl From<MembershipInvitationAcceptanceResult> for MembershipInvitationAcceptanceResponse {
    fn from(result: MembershipInvitationAcceptanceResult) -> Self {
        Self {
            invitation: result.acceptance.invitation.into(),
            membership: result.acceptance.membership.into(),
            replayed: result.replayed,
        }
    }
}
