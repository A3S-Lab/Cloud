use crate::modules::identity::domain::entities::{
    RecipientContactRecord, RecipientContactVerification,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientContactVerificationRequestResult {
    pub contact: RecipientContactRecord,
    pub verification: RecipientContactVerification,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientContactMutationResult {
    pub contact: RecipientContactRecord,
    pub replayed: bool,
}
