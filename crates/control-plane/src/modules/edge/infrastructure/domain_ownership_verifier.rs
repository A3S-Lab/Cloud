use crate::modules::edge::domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest, IDomainOwnershipVerifier,
};
use async_trait::async_trait;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalDomainOwnershipVerifier;

#[async_trait]
impl IDomainOwnershipVerifier for LocalDomainOwnershipVerifier {
    async fn verify(
        &self,
        request: DomainOwnershipVerificationRequest,
    ) -> Result<(), DomainOwnershipVerificationError> {
        if request.challenge_dns_name != request.pattern.challenge_dns_name()
            || request.expected_value.len() < 32
            || request.expected_value.len() > 512
            || request.presented_proof.len() > 512
            || request.presented_proof.contains(['\0', '\r', '\n'])
        {
            return Err(DomainOwnershipVerificationError::Invalid(
                "local domain ownership challenge is invalid".into(),
            ));
        }
        if request.expected_value.as_bytes().len() != request.presented_proof.as_bytes().len()
            || !bool::from(
                request
                    .expected_value
                    .as_bytes()
                    .ct_eq(request.presented_proof.as_bytes()),
            )
        {
            return Err(DomainOwnershipVerificationError::Rejected(
                "presented proof does not match the issued challenge".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableDomainOwnershipVerifier;

#[async_trait]
impl IDomainOwnershipVerifier for UnavailableDomainOwnershipVerifier {
    async fn verify(
        &self,
        _request: DomainOwnershipVerificationRequest,
    ) -> Result<(), DomainOwnershipVerificationError> {
        Err(DomainOwnershipVerificationError::Unavailable(
            "a production DNS verifier is not configured".into(),
        ))
    }
}
