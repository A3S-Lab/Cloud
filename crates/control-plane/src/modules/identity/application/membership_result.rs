use crate::modules::identity::domain::repositories::MembershipRecord;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MembershipMutationResult {
    pub membership: MembershipRecord,
    pub replayed: bool,
}
