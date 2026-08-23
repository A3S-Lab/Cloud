use super::export_audit_records::map_signing_error;
use crate::modules::audit::domain::{
    AuditExport, AuditExportDocument, AuditExportManifest, AuditExportManifestBundle,
    AuditExportManifestDocument, AuditExportManifestFilter, AuditExportManifestPage,
    AuditExportSnapshot, AuditRecordCursor, AuditRecordFilter, AuditRecordPage,
    AuditRetentionPolicy, IAuditExportSigner, IAuditRecordRepository,
    MAXIMUM_AUDIT_EXPORT_MANIFEST_PAGES,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, Sha256Digest};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use chrono::{DateTime, Utc};
use std::sync::Arc;

type AuditExportManifestClock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

pub const DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone)]
pub struct ExportAuditManifest {
    pub organization_id: OrganizationId,
    pub filter: AuditRecordFilter,
    pub page_size: usize,
}

impl Query for ExportAuditManifest {
    type Output = ApplicationResult<AuditExportManifestBundle>;
}

pub struct ExportAuditManifestHandler {
    repository: Arc<dyn IAuditRecordRepository>,
    signer: Arc<dyn IAuditExportSigner>,
    retention_policy: AuditRetentionPolicy,
    clock: AuditExportManifestClock,
}

impl ExportAuditManifestHandler {
    pub fn new(
        repository: Arc<dyn IAuditRecordRepository>,
        signer: Arc<dyn IAuditExportSigner>,
        retention_policy: AuditRetentionPolicy,
    ) -> Self {
        Self {
            repository,
            signer,
            retention_policy,
            clock: Arc::new(Utc::now),
        }
    }

    pub fn with_clock(mut self, clock: AuditExportManifestClock) -> Self {
        self.clock = clock;
        self
    }
}

impl QueryHandler<ExportAuditManifest> for ExportAuditManifestHandler {
    fn execute(
        &self,
        query: ExportAuditManifest,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AuditExportManifestBundle>>>
    {
        let repository = Arc::clone(&self.repository);
        let signer = Arc::clone(&self.signer);
        let retention_policy = self.retention_policy.clone();
        let generated_at = canonical_timestamp((self.clock)());
        Box::pin(async move {
            let ExportAuditManifest {
                organization_id,
                mut filter,
                page_size,
            } = query;
            filter.from = filter.from.map(canonical_timestamp);
            filter.to = filter.to.map(canonical_timestamp);
            if let Err(error) = AuditExportManifestFilter::from_record_filter(&filter, page_size) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let capacity = match page_size.checked_mul(MAXIMUM_AUDIT_EXPORT_MANIFEST_PAGES) {
                Some(capacity) => capacity,
                None => {
                    return Ok(Err(ApplicationError::Invalid(
                        "audit export manifest capacity overflowed".into(),
                    )))
                }
            };
            let capture_limit = match capacity.checked_add(1) {
                Some(limit) => limit,
                None => {
                    return Ok(Err(ApplicationError::Invalid(
                        "audit export manifest capacity overflowed".into(),
                    )))
                }
            };
            let snapshot = match repository
                .capture_export_snapshot(organization_id, &filter, capture_limit)
                .await
            {
                Ok(snapshot) => snapshot,
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = snapshot.validate(organization_id, &filter, capture_limit) {
                return Ok(Err(ApplicationError::Internal(error)));
            }
            if snapshot.records.len() > capacity {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "audit export manifest exceeds {MAXIMUM_AUDIT_EXPORT_MANIFEST_PAGES} pages; narrow the window or add exact filters"
                ))));
            }

            let AuditExportSnapshot {
                retention_state,
                records,
            } = snapshot;
            let page_count = records.len().div_ceil(page_size);
            let mut cursor = None;
            let mut expected_signing_key = None;
            let mut pages = Vec::with_capacity(page_count);
            let mut entries = Vec::with_capacity(page_count);
            for (offset, records) in records.chunks(page_size).enumerate() {
                let next_cursor = (offset + 1 < page_count).then(|| {
                    AuditRecordCursor::after(records.last().expect("non-empty page chunk")).encode()
                });
                let document = match AuditExportDocument::from_page(
                    organization_id,
                    &filter,
                    cursor.clone(),
                    page_size,
                    generated_at,
                    AuditRecordPage {
                        records: records.to_vec(),
                        next_cursor: next_cursor.clone(),
                    },
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
                if expected_signing_key
                    .as_ref()
                    .is_some_and(|key| key != &signature.key)
                {
                    return Ok(Err(ApplicationError::Internal(
                        "audit export signer changed key during one manifest bundle".into(),
                    )));
                }
                expected_signing_key.get_or_insert_with(|| signature.key.clone());
                let entry = AuditExportManifestPage {
                    index: offset + 1,
                    cursor: cursor.clone(),
                    next_cursor: next_cursor.clone(),
                    record_count: records.len(),
                    signing_key_id: signature.key.key_id.clone(),
                    payload_sha256: Sha256Digest::from_bytes(&payload),
                };
                let page = match AuditExport::from_signature(payload, signature) {
                    Ok(page) => page,
                    Err(error) => return Ok(Err(ApplicationError::Internal(error))),
                };
                entries.push(entry);
                pages.push(page);
                cursor = next_cursor;
            }

            let manifest_document = match AuditExportManifestDocument::new(
                organization_id,
                &filter,
                page_size,
                generated_at,
                &retention_policy,
                &retention_state,
                records.len(),
                entries,
            ) {
                Ok(document) => document,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let (payload, pae) = match AuditExportManifest::signable_bytes(&manifest_document) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let signature = match signer.sign(&pae).await {
                Ok(signature) => signature,
                Err(error) => return Ok(Err(map_signing_error(error))),
            };
            if expected_signing_key
                .as_ref()
                .is_some_and(|key| key != &signature.key)
            {
                return Ok(Err(ApplicationError::Internal(
                    "audit export signer changed key during one manifest bundle".into(),
                )));
            }
            let manifest = match AuditExportManifest::from_signature(payload, signature) {
                Ok(manifest) => manifest,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let bundle = AuditExportManifestBundle { manifest, pages };
            if let Err(error) = bundle.verify() {
                return Ok(Err(ApplicationError::Internal(error)));
            }
            Ok(Ok(bundle))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::audit::{
        AuditAttributionStatus, AuditExportSigningError, AuditExportSigningKey, AuditRecord,
        InMemoryAuditRecordRepository, VerifiedAuditExportSignature,
    };
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use uuid::Uuid;

    struct TestSigner {
        first: Ed25519KeyPair,
        second: Ed25519KeyPair,
        rotate_after_first: bool,
        calls: AtomicUsize,
    }

    impl TestSigner {
        fn stable() -> Self {
            Self::new(false)
        }

        fn rotating() -> Self {
            Self::new(true)
        }

        fn new(rotate_after_first: bool) -> Self {
            Self {
                first: Ed25519KeyPair::from_seed_unchecked(&[0x71; 32])
                    .expect("first audit manifest key"),
                second: Ed25519KeyPair::from_seed_unchecked(&[0x72; 32])
                    .expect("second audit manifest key"),
                rotate_after_first,
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
            let call = self.calls.fetch_add(1, Ordering::Relaxed);
            let key = if self.rotate_after_first && call > 0 {
                &self.second
            } else {
                &self.first
            };
            let public_key = key.public_key().as_ref();
            VerifiedAuditExportSignature::new(
                AuditExportSigningKey {
                    algorithm: "ed25519".into(),
                    key_id: format!("{:x}", Sha256::digest(public_key)),
                    public_key: STANDARD.encode(public_key),
                    key_version: None,
                },
                key.sign(pae).as_ref().to_vec(),
            )
        }
    }

    fn policy() -> AuditRetentionPolicy {
        AuditRetentionPolicy::new(Duration::from_secs(90 * 24 * 60 * 60)).expect("retention policy")
    }

    fn query(
        organization_id: OrganizationId,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        page_size: usize,
    ) -> ExportAuditManifest {
        ExportAuditManifest {
            organization_id,
            filter: AuditRecordFilter {
                from: Some(from),
                to: Some(to),
                ..AuditRecordFilter::default()
            },
            page_size,
        }
    }

    async fn register_records(
        repository: &InMemoryAuditRecordRepository,
        organization_id: OrganizationId,
        occurred_at: DateTime<Utc>,
        count: usize,
    ) {
        repository.register_organization(organization_id).await;
        for offset in 0..count {
            repository
                .register(AuditRecord {
                    id: Uuid::now_v7(),
                    organization_id,
                    actor_principal_id: None,
                    action: "identity.membership.created".into(),
                    aggregate_id: Uuid::now_v7(),
                    occurred_at: occurred_at + chrono::Duration::seconds(offset as i64),
                    request_id: Uuid::now_v7(),
                    project_id: None,
                    environment_id: None,
                    attribution_profile_id: None,
                    attribution_status: AuditAttributionStatus::NotApplicable,
                })
                .await
                .expect("audit record");
        }
    }

    #[tokio::test]
    async fn captures_once_and_returns_a_complete_same_key_manifest_bundle() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let organization_id = OrganizationId::new();
        let occurred_at = "2026-08-24T01:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("occurred at");
        register_records(repository.as_ref(), organization_id, occurred_at, 3).await;
        let signer = Arc::new(TestSigner::stable());
        let generated_at = "2026-08-24T02:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("generated at");
        let handler = ExportAuditManifestHandler::new(repository.clone(), signer.clone(), policy())
            .with_clock(Arc::new(move || generated_at));
        let bundle = handler
            .execute(
                query(
                    organization_id,
                    occurred_at - chrono::Duration::hours(1),
                    occurred_at + chrono::Duration::hours(1),
                    2,
                ),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect("manifest bundle");

        let document = bundle.verify().expect("offline bundle verification");
        assert_eq!(document.page_count, 2);
        assert_eq!(document.record_count, 3);
        assert_eq!(document.filter.page_size, 2);
        assert_eq!(document.generated_at, generated_at);
        assert_eq!(document.pages[0].cursor, None);
        assert_eq!(document.pages[0].next_cursor, document.pages[1].cursor);
        assert_eq!(document.pages[1].next_cursor, None);
        assert_eq!(repository.query_count(), 1);
        assert_eq!(signer.calls.load(Ordering::Relaxed), 3);

        let mut page_tampered = bundle.clone();
        page_tampered.pages[0].envelope.payload = STANDARD.encode(b"{}");
        assert!(page_tampered.verify().is_err());
        let mut manifest_tampered = bundle;
        manifest_tampered.manifest.envelope.signatures[0].signature = STANDARD.encode([0_u8; 64]);
        assert!(manifest_tampered.verify().is_err());
    }

    #[tokio::test]
    async fn signs_an_empty_manifest_but_rejects_a_ninth_page_before_signing() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let organization_id = OrganizationId::new();
        repository.register_organization(organization_id).await;
        let now = canonical_timestamp(Utc::now());
        let signer = Arc::new(TestSigner::stable());
        let handler = ExportAuditManifestHandler::new(repository.clone(), signer.clone(), policy());
        let empty = handler
            .execute(
                query(organization_id, now, now + chrono::Duration::hours(1), 1),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect("empty manifest");
        assert!(empty.pages.is_empty());
        assert_eq!(empty.verify().expect("empty verification").record_count, 0);
        assert_eq!(signer.calls.load(Ordering::Relaxed), 1);

        register_records(repository.as_ref(), organization_id, now, 8).await;
        let exact_capacity_signer = Arc::new(TestSigner::stable());
        let exact_capacity = ExportAuditManifestHandler::new(
            repository.clone(),
            exact_capacity_signer.clone(),
            policy(),
        )
        .execute(
            query(
                organization_id,
                now - chrono::Duration::seconds(1),
                now + chrono::Duration::seconds(10),
                1,
            ),
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("framework")
        .expect("exact eight-page manifest");
        assert_eq!(exact_capacity.pages.len(), 8);
        assert_eq!(
            exact_capacity
                .verify()
                .expect("exact-capacity verification")
                .record_count,
            8
        );
        assert_eq!(exact_capacity_signer.calls.load(Ordering::Relaxed), 9);

        register_records(
            repository.as_ref(),
            organization_id,
            now + chrono::Duration::seconds(9),
            1,
        )
        .await;
        let overflow_signer = Arc::new(TestSigner::stable());
        let overflow =
            ExportAuditManifestHandler::new(repository.clone(), overflow_signer.clone(), policy())
                .execute(
                    query(
                        organization_id,
                        now - chrono::Duration::seconds(1),
                        now + chrono::Duration::seconds(10),
                        1,
                    ),
                    CqrsContext::new(ModuleRef::new()),
                )
                .await
                .expect("framework")
                .expect_err("ninth page");
        assert!(matches!(overflow, ApplicationError::Invalid(_)));
        assert!(overflow.to_string().contains("exceeds 8 pages"));
        assert_eq!(overflow_signer.calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn rejects_signing_key_drift_without_returning_a_partial_bundle() {
        let repository = Arc::new(InMemoryAuditRecordRepository::new());
        let organization_id = OrganizationId::new();
        let now = canonical_timestamp(Utc::now());
        register_records(repository.as_ref(), organization_id, now, 2).await;
        let signer = Arc::new(TestSigner::rotating());
        let error = ExportAuditManifestHandler::new(repository, signer.clone(), policy())
            .execute(
                query(
                    organization_id,
                    now - chrono::Duration::seconds(1),
                    now + chrono::Duration::seconds(2),
                    1,
                ),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("framework")
            .expect_err("key drift");
        assert!(matches!(error, ApplicationError::Internal(_)));
        assert_eq!(signer.calls.load(Ordering::Relaxed), 2);
    }
}
