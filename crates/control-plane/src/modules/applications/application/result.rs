use crate::modules::applications::domain::ApplicationRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationMutationResult {
    pub record: ApplicationRecord,
    pub replayed: bool,
}
