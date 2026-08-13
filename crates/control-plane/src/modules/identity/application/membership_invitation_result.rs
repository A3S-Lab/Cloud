use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::identity::domain::repositories::MembershipInvitationAcceptance;

#[derive(Debug, Clone)]
pub struct MembershipInvitationMutationResult {
    pub invitation: MembershipInvitation,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct MembershipInvitationAcceptanceResult {
    pub acceptance: MembershipInvitationAcceptance,
    pub replayed: bool,
}
