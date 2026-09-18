use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryMembershipProjectionMutationResult {
    pub bindings: Vec<DirectoryMembershipProjectionBinding>,
    pub replayed: bool,
}
