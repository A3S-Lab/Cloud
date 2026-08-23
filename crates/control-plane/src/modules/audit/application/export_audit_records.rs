use super::audit_record_page::query_audit_record_page;
use crate::modules::audit::domain::{
    AuditExport, AuditExportDocument, AuditExportSigningError, AuditRecordFilter,
    IAuditExportSigner, IAuditRecordRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use std::sync::Arc;

type AuditExportClock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ExportAuditRecords {
    pub organization_id: OrganizationId,
    pub filter: AuditRecordFilter,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl Query for ExportAuditRecords {
    type Output = ApplicationResult<AuditExport>;
}

pub struct ExportAuditRecordsHandler {
    repository: Arc<dyn IAuditRecordRepository>,
    signer: Arc<dyn IAuditExportSigner>,
    clock: AuditExportClock,
}

impl ExportAuditRecordsHandler {
    pub fn new(
        repository: Arc<dyn IAuditRecordRepository>,
        signer: Arc<dyn IAuditExportSigner>,
    ) -> Self {
        Self {
            repository,
            signer,
            clock: Arc::new(Utc::now),
        }
    }

    pub fn with_clock(mut self, clock: AuditExportClock) -> Self {
        self.clock = clock;
        self
    }
}

impl QueryHandler<ExportAuditRecords> for ExportAuditRecordsHandler {
    fn execute(
        &self,
        query: ExportAuditRecords,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AuditExport>>> {
        let repository = Arc::clone(&self.repository);
        let signer = Arc::clone(&self.signer);
        let generated_at = (self.clock)();
        Box::pin(async move {
            let ExportAuditRecords {
                organization_id,
                mut filter,
                cursor,
                limit,
            } = query;
            filter.from = filter.from.map(canonical_timestamp);
            filter.to = filter.to.map(canonical_timestamp);
            if let Err(error) =
                crate::modules::audit::domain::AuditExportFilter::from_record_filter(&filter, limit)
            {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let page = match query_audit_record_page(
                repository.as_ref(),
                organization_id,
                &filter,
                cursor.as_deref(),
                limit,
            )
            .await
            {
                Ok(page) => page,
                Err(error) => return Ok(Err(error)),
            };
            let document = match AuditExportDocument::from_page(
                organization_id,
                &filter,
                cursor,
                limit,
                generated_at,
                page,
            ) {
                Ok(document) => document,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let (payload, pae) = match AuditExport::signable_bytes(&document) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let signature = match signer.sign(&pae).await {
                Ok(signature) => signature,
                Err(error) => return Ok(Err(map_signing_error(error))),
            };
            Ok(AuditExport::from_signature(payload, signature).map_err(ApplicationError::Internal))
        })
    }
}

pub(super) fn map_signing_error(error: AuditExportSigningError) -> ApplicationError {
    match error {
        AuditExportSigningError::Unavailable(_) => {
            ApplicationError::Unavailable("audit export signer is unavailable".into())
        }
        AuditExportSigningError::Invalid(_) | AuditExportSigningError::Rejected(_) => {
            ApplicationError::Internal("audit export signer rejected its output".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::audit::domain::{
        AuditAttributionStatus, AuditExportSigningKey, AuditRecord, VerifiedAuditExportSignature,
    };
    use crate::modules::audit::InMemoryAuditRecordRepository;
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    struct TestSigner {
        key: Ed25519KeyPair,
        calls: AtomicUsize,
    }

    impl TestSigner {
        fn new() -> Self {
            Self {
                key: Ed25519KeyPair::from_seed_unchecked(&[0x61; 32])
                    .expect("audit export signing key"),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl IAuditExportSigner for TestSigner {
        async fn sign(
            &self,
            pae: &[u8],
        ) -> Result<VerifiedAuditExportSignature, AuditExportSigningError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let public_key = self.key.public_key().as_ref();
            VerifiedAuditExportSignature::new(
                AuditExportSigningKey {
                    algorithm: "ed25519".into(),
                    key_id: format!("{:x}", Sha256::digest(public_key)),
                    public_key: STANDARD.encode(public_key),
                    key_version: None,
                },
                self.key.sign(pae).as_ref().to_vec(),
            )
        }
    }

    struct UnavailableSigner;

    #[async_trait]
    impl IAuditExportSigner for UnavailableSigner {
        async fn sign(
            &self,
            _pae: &[u8],
        ) -> Result<VerifiedAuditExportSignature, AuditExportSigningError> {
            Err(AuditExportSigningError::Unavailable(
                "private provider detail".into(),
            ))
        }
    }

    fn query(
        organization_id: OrganizationId,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> ExportAuditRecords {
        ExportAuditRecords {
            organization_id,
            filter: AuditRecordFilter {
                from: Some(from),
                to: Some(to),
                ..AuditRecordFilter::default()
            },
            cursor: None,
            limit: 50,
        }
    }

    #[tokio::test]
    async fn queries_canonical_page_once_and_returns_an_offline_verifiable_export() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let organization_id = OrganizationId::new();
        let occurred_at = "2026-08-13T01:02:03Z"
            .parse::<DateTime<Utc>>()
            .expect("occurred at");
        repository
            .register(AuditRecord {
                id: Uuid::now_v7(),
                organization_id,
                actor_principal_id: None,
                action: "identity.membership.created".into(),
                aggregate_id: Uuid::now_v7(),
                occurred_at,
                request_id: Uuid::now_v7(),
                project_id: None,
                environment_id: None,
                attribution_profile_id: None,
                attribution_status: AuditAttributionStatus::NotApplicable,
            })
            .await
            .expect("audit record");
        let signer = Arc::new(TestSigner::new());
        let generated_at = "2026-08-13T02:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("generated at");
        let handler = ExportAuditRecordsHandler::new(repository.clone(), signer.clone())
            .with_clock(Arc::new(move || generated_at));
        let export = handler
            .execute(
                query(
                    organization_id,
                    "2026-08-01T00:00:00Z".parse().expect("from"),
                    "2026-08-31T00:00:00Z".parse().expect("to"),
                ),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect("audit export");

        let payload = export.verify().expect("offline verification");
        let document: serde_json::Value = serde_json::from_slice(&payload).expect("document");
        assert_eq!(document["generatedAt"], "2026-08-13T02:00:00Z");
        assert_eq!(document["records"].as_array().map(Vec::len), Some(1));
        assert_eq!(repository.query_count(), 1);
        assert_eq!(signer.calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn rejects_invalid_and_retention_gap_windows_before_signing_and_fails_closed_when_signing_is_unavailable(
    ) {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let signer = Arc::new(TestSigner::new());
        let handler = ExportAuditRecordsHandler::new(repository.clone(), signer.clone());
        let now = Utc::now();
        let invalid = ExportAuditRecords {
            organization_id: OrganizationId::new(),
            filter: AuditRecordFilter {
                from: None,
                to: Some(now),
                ..AuditRecordFilter::default()
            },
            cursor: None,
            limit: 50,
        };
        assert!(matches!(
            handler
                .execute(invalid, CqrsContext::new(ModuleRef::new()))
                .await
                .expect("framework"),
            Err(ApplicationError::Invalid(_))
        ));
        assert_eq!(repository.query_count(), 0);
        assert_eq!(signer.calls.load(Ordering::Relaxed), 0);

        let retained_organization = OrganizationId::new();
        repository
            .register_organization(retained_organization)
            .await;
        crate::modules::audit::AuditRetentionWorker::new(
            repository.clone(),
            std::time::Duration::from_secs(24 * 60 * 60),
            std::time::Duration::from_secs(1),
            1,
            10,
        )
        .expect("retention worker")
        .run_once(now)
        .await
        .expect("retention sweep");
        let retention_gap = handler
            .execute(
                query(retained_organization, now - chrono::Duration::days(2), now),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect_err("expired export range");
        assert!(matches!(retention_gap, ApplicationError::Conflict(_)));
        assert_eq!(repository.query_count(), 1);
        assert_eq!(signer.calls.load(Ordering::Relaxed), 0);

        let unavailable =
            ExportAuditRecordsHandler::new(repository.clone(), Arc::new(UnavailableSigner));
        let error = unavailable
            .execute(
                query(OrganizationId::new(), now, now + chrono::Duration::hours(1)),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect_err("unavailable signer");
        assert_eq!(
            error,
            ApplicationError::Unavailable("audit export signer is unavailable".into())
        );
        assert!(!error.to_string().contains("private provider detail"));
        assert_eq!(repository.query_count(), 2);
    }
}
