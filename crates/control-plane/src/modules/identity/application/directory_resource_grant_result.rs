use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryResourceGrantMutationResult {
    pub directory_resource_grant: DirectoryResourceGrant,
    pub replayed: bool,
}
