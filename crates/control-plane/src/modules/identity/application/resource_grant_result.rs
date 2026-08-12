use crate::modules::identity::domain::entities::ResourceGrant;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ResourceGrantMutationResult {
    pub resource_grant: ResourceGrant,
    pub replayed: bool,
}
