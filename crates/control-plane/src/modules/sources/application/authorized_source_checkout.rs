use super::{
    ISourceRepositoryCredentialProvider, SourceRepositoryCredentialError,
    SourceRepositoryCredentialRequest,
};
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::modules::sources::domain::{
    CheckedOutSource, ISourceCheckout, SourceCheckoutError, SourceCheckoutRequest,
};
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

/// Sources-owned public/private checkout boundary.
///
/// Consumers provide an exact canonical checkout request and never receive a
/// provider credential. Public access is attempted first; only the one
/// authoritative organization installation may supply the fallback token.
#[async_trait]
pub trait IAuthorizedSourceCheckout: Send + Sync {
    async fn checkout(
        &self,
        organization_id: OrganizationId,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError>;

    /// Revalidate an existing checkout strictly without credential issuance or
    /// provider acquisition.
    async fn replay(
        &self,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError>;

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError>;
}

pub struct AuthorizedSourceCheckoutService {
    checkout: Arc<dyn ISourceCheckout>,
    credentials: Arc<dyn ISourceRepositoryCredentialProvider>,
}

impl AuthorizedSourceCheckoutService {
    pub fn new(
        checkout: Arc<dyn ISourceCheckout>,
        credentials: Arc<dyn ISourceRepositoryCredentialProvider>,
    ) -> Self {
        Self {
            checkout,
            credentials,
        }
    }

    async fn validate_checkout(
        &self,
        request: &SourceCheckoutRequest,
        checked_out: CheckedOutSource,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        if let Err(message) = checked_out.validate_for(request) {
            if let Err(error) = self.checkout.remove(request.checkout_id).await {
                tracing::warn!(
                    checkout_id = %request.checkout_id,
                    error = %error,
                    "invalid source checkout cleanup failed"
                );
            }
            return Err(SourceCheckoutError::Integrity(message));
        }
        Ok(checked_out)
    }
}

#[async_trait]
impl IAuthorizedSourceCheckout for AuthorizedSourceCheckoutService {
    async fn checkout(
        &self,
        organization_id: OrganizationId,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        if organization_id.as_uuid().is_nil() {
            return Err(SourceCheckoutError::Invalid(
                "source checkout organization is invalid".into(),
            ));
        }
        request.validate().map_err(SourceCheckoutError::Invalid)?;
        match self.checkout.checkout(request, None).await {
            Ok(checked_out) => return self.validate_checkout(request, checked_out).await,
            Err(SourceCheckoutError::Unavailable(_)) => {}
            Err(error) => return Err(error),
        }

        let credential = self
            .credentials
            .issue(&SourceRepositoryCredentialRequest {
                organization_id,
                repository: request.repository.clone(),
            })
            .await
            .map_err(map_credential_error)?;
        let checked_out = self.checkout.checkout(request, Some(&credential)).await?;
        self.validate_checkout(request, checked_out).await
    }

    async fn replay(
        &self,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        request.validate().map_err(SourceCheckoutError::Invalid)?;
        let checked_out = self.checkout.replay(request).await?;
        self.validate_checkout(request, checked_out).await
    }

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError> {
        if checkout_id.is_nil() {
            return Err(SourceCheckoutError::Invalid(
                "source checkout ID cannot be nil".into(),
            ));
        }
        self.checkout.remove(checkout_id).await
    }
}

fn private_source_unavailable() -> SourceCheckoutError {
    SourceCheckoutError::Unavailable("source repository is unavailable".into())
}

fn map_credential_error(error: SourceRepositoryCredentialError) -> SourceCheckoutError {
    match error {
        SourceRepositoryCredentialError::Invalid(message) => SourceCheckoutError::Invalid(message),
        SourceRepositoryCredentialError::Unavailable => private_source_unavailable(),
        SourceRepositoryCredentialError::Integrity(message) => {
            SourceCheckoutError::Integrity(message)
        }
        SourceRepositoryCredentialError::Storage(message) => SourceCheckoutError::Storage(message),
    }
}

#[cfg(test)]
#[path = "authorized_source_checkout_tests.rs"]
mod tests;
