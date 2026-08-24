use super::UserFileContentReference;

/// Domain evidence that the exact admitted UserFile reference became durable.
///
/// Provider keys, buckets, streams, and storage errors deliberately do not
/// enter this value. `replayed` records idempotent application behavior; it
/// does not create another lifecycle state in the UserFile aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFileObjectWrite {
    reference: UserFileContentReference,
    replayed: bool,
}

impl UserFileObjectWrite {
    pub(in crate::modules::files) fn stored(
        reference: UserFileContentReference,
        replayed: bool,
    ) -> Self {
        Self {
            reference,
            replayed,
        }
    }

    pub const fn reference(&self) -> &UserFileContentReference {
        &self.reference
    }

    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}
