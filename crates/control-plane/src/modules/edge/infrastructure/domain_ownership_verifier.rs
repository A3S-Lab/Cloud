use crate::modules::edge::domain::services::{
    DomainOwnershipVerificationError, DomainOwnershipVerificationRequest, IDomainOwnershipVerifier,
};
use async_trait::async_trait;
use hickory_resolver::{error::ResolveErrorKind, TokioAsyncResolver};
use std::sync::Arc;
use std::time::Duration;
use subtle::ConstantTimeEq;

const MAX_TXT_RECORDS: usize = 64;
const MAX_TXT_RECORD_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalDomainOwnershipVerifier;

#[async_trait]
impl IDomainOwnershipVerifier for LocalDomainOwnershipVerifier {
    async fn verify(
        &self,
        request: DomainOwnershipVerificationRequest,
    ) -> Result<(), DomainOwnershipVerificationError> {
        validate_request(&request, "local")?;
        if !proof_matches(&request.expected_value, request.presented_proof.as_bytes()) {
            return Err(DomainOwnershipVerificationError::Rejected(
                "presented proof does not match the issued challenge".into(),
            ));
        }
        Ok(())
    }
}

pub struct DnsDomainOwnershipVerifier {
    resolver: Arc<dyn DnsTxtResolver>,
    lookup_timeout: Duration,
}

impl DnsDomainOwnershipVerifier {
    pub fn from_system_config(
        lookup_timeout: Duration,
    ) -> Result<Self, DomainOwnershipVerificationError> {
        let resolver = TokioAsyncResolver::tokio_from_system_conf().map_err(|_| {
            DomainOwnershipVerificationError::Unavailable(
                "system DNS resolver configuration is unavailable".into(),
            )
        })?;
        Self::with_resolver(Arc::new(SystemDnsTxtResolver { resolver }), lookup_timeout)
    }

    fn with_resolver(
        resolver: Arc<dyn DnsTxtResolver>,
        lookup_timeout: Duration,
    ) -> Result<Self, DomainOwnershipVerificationError> {
        if lookup_timeout.is_zero() || lookup_timeout > Duration::from_secs(60) {
            return Err(DomainOwnershipVerificationError::Invalid(
                "DNS ownership lookup timeout must be between 1 millisecond and 60 seconds".into(),
            ));
        }
        Ok(Self {
            resolver,
            lookup_timeout,
        })
    }
}

#[async_trait]
impl IDomainOwnershipVerifier for DnsDomainOwnershipVerifier {
    async fn verify(
        &self,
        request: DomainOwnershipVerificationRequest,
    ) -> Result<(), DomainOwnershipVerificationError> {
        validate_request(&request, "DNS")?;
        if !proof_matches(&request.expected_value, request.presented_proof.as_bytes()) {
            return Err(DomainOwnershipVerificationError::Rejected(
                "presented proof does not match the issued challenge".into(),
            ));
        }
        let records = tokio::time::timeout(
            self.lookup_timeout,
            self.resolver.lookup_txt(&request.challenge_dns_name),
        )
        .await
        .map_err(|_| dns_unavailable())?
        .map_err(|_| dns_unavailable())?;
        if records.len() > MAX_TXT_RECORDS
            || records
                .iter()
                .any(|record| record.len() > MAX_TXT_RECORD_BYTES)
        {
            return Err(dns_unavailable());
        }
        if records
            .iter()
            .any(|record| proof_matches(&request.expected_value, record))
        {
            return Ok(());
        }
        Err(DomainOwnershipVerificationError::NotReady(
            "expected DNS TXT challenge is not observable".into(),
        ))
    }
}

#[async_trait]
trait DnsTxtResolver: Send + Sync {
    async fn lookup_txt(&self, dns_name: &str) -> Result<Vec<Vec<u8>>, String>;
}

struct SystemDnsTxtResolver {
    resolver: TokioAsyncResolver,
}

#[async_trait]
impl DnsTxtResolver for SystemDnsTxtResolver {
    async fn lookup_txt(&self, dns_name: &str) -> Result<Vec<Vec<u8>>, String> {
        match self.resolver.txt_lookup(dns_name).await {
            Ok(lookup) => Ok(lookup.iter().map(join_txt_fragments).collect()),
            Err(error) if matches!(error.kind(), ResolveErrorKind::NoRecordsFound { .. }) => {
                Ok(Vec::new())
            }
            Err(_) => Err("DNS TXT lookup failed".into()),
        }
    }
}

fn join_txt_fragments(record: &hickory_resolver::proto::rr::rdata::TXT) -> Vec<u8> {
    record
        .txt_data()
        .iter()
        .flat_map(|fragment| fragment.iter().copied())
        .collect()
}

fn validate_request(
    request: &DomainOwnershipVerificationRequest,
    provider: &str,
) -> Result<(), DomainOwnershipVerificationError> {
    if request.challenge_dns_name != request.pattern.challenge_dns_name()
        || request.expected_value.len() < 32
        || request.expected_value.len() > MAX_TXT_RECORD_BYTES
        || request.presented_proof.len() > MAX_TXT_RECORD_BYTES
        || request.presented_proof.contains(['\0', '\r', '\n'])
    {
        return Err(DomainOwnershipVerificationError::Invalid(format!(
            "{provider} domain ownership challenge is invalid"
        )));
    }
    Ok(())
}

fn proof_matches(expected: &str, presented: &[u8]) -> bool {
    expected.len() == presented.len() && bool::from(expected.as_bytes().ct_eq(presented))
}

fn dns_unavailable() -> DomainOwnershipVerificationError {
    DomainOwnershipVerificationError::Unavailable(
        "DNS TXT verification is temporarily unavailable".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::DomainNamePattern;
    use crate::modules::shared_kernel::domain::DomainClaimId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    struct FixedTxtResolver {
        records: Vec<Vec<u8>>,
        unavailable: bool,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl DnsTxtResolver for FixedTxtResolver {
        async fn lookup_txt(&self, _dns_name: &str) -> Result<Vec<Vec<u8>>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.unavailable {
                return Err("provider detail must not escape".into());
            }
            Ok(self.records.clone())
        }
    }

    #[tokio::test]
    async fn production_dns_verifier_requires_the_exact_proof_and_observed_txt_record() {
        let expected = challenge();
        let resolver = Arc::new(FixedTxtResolver {
            records: vec![b"unrelated".to_vec(), expected.as_bytes().to_vec()],
            unavailable: false,
            calls: AtomicUsize::new(0),
        });
        let verifier =
            DnsDomainOwnershipVerifier::with_resolver(resolver.clone(), Duration::from_millis(100))
                .expect("DNS verifier");

        verifier
            .verify(request(&expected, &expected))
            .await
            .expect("matching TXT record");
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn production_dns_verifier_joins_txt_fragments_in_wire_order() {
        let record = hickory_resolver::proto::rr::rdata::TXT::new(vec![
            "a3s-cloud-verification=".into(),
            "fragmented-proof".into(),
        ]);

        assert_eq!(
            join_txt_fragments(&record),
            b"a3s-cloud-verification=fragmented-proof"
        );
    }

    #[tokio::test]
    async fn production_dns_verifier_keeps_unobserved_proof_retryable() {
        let expected = challenge();
        let resolver = Arc::new(FixedTxtResolver {
            records: vec![b"stale-record".to_vec()],
            unavailable: false,
            calls: AtomicUsize::new(0),
        });
        let verifier =
            DnsDomainOwnershipVerifier::with_resolver(resolver, Duration::from_millis(100))
                .expect("DNS verifier");

        let error = verifier
            .verify(request(&expected, &expected))
            .await
            .expect_err("TXT record is not observable");
        assert!(matches!(
            error,
            DomainOwnershipVerificationError::NotReady(_)
        ));
    }

    #[tokio::test]
    async fn production_dns_verifier_rejects_wrong_proof_without_dns_and_sanitizes_failures() {
        let expected = challenge();
        let resolver = Arc::new(FixedTxtResolver {
            records: Vec::new(),
            unavailable: true,
            calls: AtomicUsize::new(0),
        });
        let verifier =
            DnsDomainOwnershipVerifier::with_resolver(resolver.clone(), Duration::from_millis(100))
                .expect("DNS verifier");

        assert!(matches!(
            verifier
                .verify(request(&expected, "wrong"))
                .await
                .expect_err("wrong proof"),
            DomainOwnershipVerificationError::Rejected(_)
        ));
        assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);

        let error = verifier
            .verify(request(&expected, &expected))
            .await
            .expect_err("resolver is unavailable");
        assert!(matches!(
            error,
            DomainOwnershipVerificationError::Unavailable(ref message)
                if !message.contains("provider detail")
        ));
    }

    fn request(expected: &str, presented: &str) -> DomainOwnershipVerificationRequest {
        let pattern = DomainNamePattern::parse("example.com").expect("domain pattern");
        DomainOwnershipVerificationRequest {
            claim_id: DomainClaimId::new(),
            challenge_dns_name: pattern.challenge_dns_name(),
            pattern,
            expected_value: expected.into(),
            presented_proof: presented.into(),
        }
    }

    fn challenge() -> String {
        format!("a3s-cloud-verification={}", "a".repeat(43))
    }
}
