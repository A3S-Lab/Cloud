use crate::modules::secrets::domain::SecretEncryptionError;
use crate::modules::shared_kernel::application::ApplicationError;

pub(crate) fn encryption_error(error: SecretEncryptionError) -> ApplicationError {
    match error {
        SecretEncryptionError::InvalidInput(message) => ApplicationError::Invalid(message),
        SecretEncryptionError::Rejected(message) | SecretEncryptionError::Unavailable(message) => {
            ApplicationError::Internal(message)
        }
    }
}
