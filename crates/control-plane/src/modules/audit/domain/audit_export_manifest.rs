use super::{
    AuditAttributionStatus, AuditExport, AuditExportDocument, AuditExportDsseEnvelope,
    AuditExportDsseSignature, AuditExportFilter, AuditExportSigningKey, AuditRecord,
    AuditRecordCursor, AuditRecordFilter, AuditRetentionPolicy, AuditRetentionState,
    VerifiedAuditExportSignature, MAXIMUM_AUDIT_EXPORT_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, dsse_pae_bounded, EnvironmentId, OrganizationId,
    PrincipalId, ProjectAttributionProfileId, ProjectId, Sha256Digest,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

pub const AUDIT_EXPORT_MANIFEST_SCHEMA: &str = "a3s.cloud.audit-export-manifest.v1";
pub const AUDIT_EXPORT_MANIFEST_PAYLOAD_TYPE: &str =
    "application/vnd.a3s.cloud.audit-export-manifest.v1+json";
pub const MAXIMUM_AUDIT_EXPORT_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAXIMUM_AUDIT_EXPORT_MANIFEST_PAGES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditExportSnapshot {
    pub retention_state: AuditRetentionState,
    pub records: Vec<AuditRecord>,
}

impl AuditExportSnapshot {
    pub fn validate(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        maximum_records: usize,
    ) -> Result<(), String> {
        self.retention_state.validate()?;
        if self.retention_state.organization_id != organization_id
            || maximum_records == 0
            || self.records.len() > maximum_records
        {
            return Err("audit export snapshot identity or bounds are invalid".into());
        }
        filter.validate()?;
        for record in &self.records {
            record.validate()?;
            if record.organization_id != organization_id
                || !filter.matches(record)
                || self
                    .retention_state
                    .records_available_from
                    .is_some_and(|boundary| record.occurred_at < boundary)
            {
                return Err("audit export snapshot record does not match its selection".into());
            }
        }
        if self.records.windows(2).any(|pair| {
            pair[0].occurred_at < pair[1].occurred_at
                || (pair[0].occurred_at == pair[1].occurred_at && pair[0].id <= pair[1].id)
        }) {
            return Err("audit export snapshot is not in strict descending keyset order".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportManifestFilter {
    pub actor_principal_id: Option<PrincipalId>,
    pub action: Option<String>,
    pub aggregate_id: Option<Uuid>,
    pub request_id: Option<Uuid>,
    pub project_id: Option<ProjectId>,
    pub environment_id: Option<EnvironmentId>,
    pub attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub attribution_status: Option<AuditAttributionStatus>,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub page_size: usize,
}

impl AuditExportManifestFilter {
    pub fn from_record_filter(
        filter: &AuditRecordFilter,
        page_size: usize,
    ) -> Result<Self, String> {
        let filter = AuditExportFilter::from_record_filter(filter, page_size)?;
        Ok(Self {
            actor_principal_id: filter.actor_principal_id,
            action: filter.action,
            aggregate_id: filter.aggregate_id,
            request_id: filter.request_id,
            project_id: filter.project_id,
            environment_id: filter.environment_id,
            attribution_profile_id: filter.attribution_profile_id,
            attribution_status: filter.attribution_status,
            from: filter.from,
            to: filter.to,
            page_size: filter.limit,
        })
    }

    fn record_filter(&self) -> AuditRecordFilter {
        AuditRecordFilter {
            actor_principal_id: self.actor_principal_id,
            action: self.action.clone(),
            aggregate_id: self.aggregate_id,
            request_id: self.request_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            attribution_profile_id: self.attribution_profile_id,
            attribution_status: self.attribution_status,
            from: Some(self.from),
            to: Some(self.to),
        }
    }

    fn page_filter(&self) -> Result<AuditExportFilter, String> {
        AuditExportFilter::from_record_filter(&self.record_filter(), self.page_size)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportManifestRetention {
    pub retention_ms: u64,
    pub policy_digest: Sha256Digest,
    pub applied_policy_digest: Option<Sha256Digest>,
    pub current_policy_applied: bool,
    pub records_available_from: Option<DateTime<Utc>>,
    pub records_deleted_before: Option<DateTime<Utc>>,
    pub version: u64,
}

impl AuditExportManifestRetention {
    fn from_policy_and_state(
        policy: &AuditRetentionPolicy,
        state: &AuditRetentionState,
    ) -> Result<Self, String> {
        state.validate()?;
        let retention = Self {
            retention_ms: policy.retention_ms(),
            policy_digest: policy.digest().clone(),
            applied_policy_digest: state.applied_policy_digest.clone(),
            current_policy_applied: state.applied_policy_digest.as_ref() == Some(policy.digest()),
            records_available_from: state.records_available_from,
            records_deleted_before: state.records_deleted_before,
            version: state.version,
        };
        retention.validate()?;
        Ok(retention)
    }

    fn validate(&self) -> Result<(), String> {
        let policy = AuditRetentionPolicy::new(Duration::from_millis(self.retention_ms))?;
        if policy.digest() != &self.policy_digest
            || self.current_policy_applied
                != (self.applied_policy_digest.as_ref() == Some(&self.policy_digest))
            || self.records_available_from.is_some() != self.applied_policy_digest.is_some()
            || self.records_deleted_before.is_some() && self.records_available_from.is_none()
            || self
                .records_deleted_before
                .zip(self.records_available_from)
                .is_some_and(|(deleted, available)| deleted > available)
            || [self.records_available_from, self.records_deleted_before]
                .into_iter()
                .flatten()
                .any(|timestamp| canonical_timestamp(timestamp) != timestamp)
        {
            return Err("audit export manifest retention snapshot is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportManifestPage {
    pub index: usize,
    pub cursor: Option<String>,
    pub next_cursor: Option<String>,
    pub record_count: usize,
    pub signing_key_id: String,
    pub payload_sha256: Sha256Digest,
}

impl AuditExportManifestPage {
    fn validate(&self, page_size: usize) -> Result<(), String> {
        if self.index == 0
            || self.record_count == 0
            || self.record_count > page_size
            || !valid_key_id(&self.signing_key_id)
        {
            return Err("audit export manifest page identity or bounds are invalid".into());
        }
        self.cursor
            .as_deref()
            .map(AuditRecordCursor::parse)
            .transpose()?;
        self.next_cursor
            .as_deref()
            .map(AuditRecordCursor::parse)
            .transpose()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportManifestDocument {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub filter: AuditExportManifestFilter,
    pub generated_at: DateTime<Utc>,
    pub retention: AuditExportManifestRetention,
    pub record_count: usize,
    pub page_count: usize,
    pub pages: Vec<AuditExportManifestPage>,
}

impl AuditExportManifestDocument {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        page_size: usize,
        generated_at: DateTime<Utc>,
        policy: &AuditRetentionPolicy,
        retention_state: &AuditRetentionState,
        record_count: usize,
        pages: Vec<AuditExportManifestPage>,
    ) -> Result<Self, String> {
        if retention_state.organization_id != organization_id {
            return Err("audit export manifest retention organization does not match".into());
        }
        let document = Self {
            schema: AUDIT_EXPORT_MANIFEST_SCHEMA.into(),
            organization_id,
            filter: AuditExportManifestFilter::from_record_filter(filter, page_size)?,
            generated_at: canonical_timestamp(generated_at),
            retention: AuditExportManifestRetention::from_policy_and_state(
                policy,
                retention_state,
            )?,
            record_count,
            page_count: pages.len(),
            pages,
        };
        document.validate()?;
        Ok(document)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_EXPORT_MANIFEST_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.generated_at != canonical_timestamp(self.generated_at)
            || self.page_count != self.pages.len()
            || self.page_count > MAXIMUM_AUDIT_EXPORT_MANIFEST_PAGES
        {
            return Err("audit export manifest identity or page bounds are invalid".into());
        }
        self.filter.page_filter()?;
        self.retention.validate()?;
        let maximum_records = self
            .filter
            .page_size
            .checked_mul(MAXIMUM_AUDIT_EXPORT_MANIFEST_PAGES)
            .ok_or_else(|| "audit export manifest record bound overflowed".to_owned())?;
        if self.record_count > maximum_records
            || (self.pages.is_empty() != (self.record_count == 0))
        {
            return Err("audit export manifest record bounds are invalid".into());
        }
        let mut record_count = 0_usize;
        let mut expected_cursor: Option<&str> = None;
        for (offset, page) in self.pages.iter().enumerate() {
            page.validate(self.filter.page_size)?;
            if page.index != offset + 1 || page.cursor.as_deref() != expected_cursor {
                return Err("audit export manifest page cursor chain is invalid".into());
            }
            let is_last = offset + 1 == self.pages.len();
            if (!is_last && page.record_count != self.filter.page_size)
                || (is_last && page.next_cursor.is_some())
                || (!is_last && page.next_cursor.is_none())
            {
                return Err("audit export manifest page partition is invalid".into());
            }
            record_count = record_count
                .checked_add(page.record_count)
                .ok_or_else(|| "audit export manifest record count overflowed".to_owned())?;
            expected_cursor = page.next_cursor.as_deref();
        }
        if record_count != self.record_count {
            return Err("audit export manifest record count is inconsistent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportManifest {
    pub envelope: AuditExportDsseEnvelope,
    pub signing_key: AuditExportSigningKey,
}

impl AuditExportManifest {
    pub fn signable_bytes(
        document: &AuditExportManifestDocument,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        document.validate()?;
        let payload = canonical_json_bounded(
            document,
            MAXIMUM_AUDIT_EXPORT_MANIFEST_BYTES,
            "audit export manifest document",
        )?;
        let pae = dsse_pae_bounded(
            AUDIT_EXPORT_MANIFEST_PAYLOAD_TYPE,
            &payload,
            MAXIMUM_AUDIT_EXPORT_MANIFEST_BYTES,
        )?;
        Ok((payload, pae))
    }

    pub fn from_signature(
        payload: Vec<u8>,
        signature: VerifiedAuditExportSignature,
    ) -> Result<Self, String> {
        let manifest = Self {
            envelope: AuditExportDsseEnvelope {
                payload_type: AUDIT_EXPORT_MANIFEST_PAYLOAD_TYPE.into(),
                payload: STANDARD.encode(payload),
                signatures: vec![AuditExportDsseSignature {
                    key_id: signature.key.key_id.clone(),
                    signature: STANDARD.encode(signature.signature),
                }],
            },
            signing_key: signature.key,
        };
        manifest.verify()?;
        Ok(manifest)
    }

    pub fn verify(&self) -> Result<Vec<u8>, String> {
        self.signing_key.validate()?;
        if self.envelope.payload_type != AUDIT_EXPORT_MANIFEST_PAYLOAD_TYPE
            || self.envelope.signatures.len() != 1
        {
            return Err("audit export manifest DSSE envelope shape is invalid".into());
        }
        let payload = STANDARD
            .decode(&self.envelope.payload)
            .map_err(|_| "audit export manifest payload is not canonical base64".to_owned())?;
        if payload.len() > MAXIMUM_AUDIT_EXPORT_MANIFEST_BYTES
            || STANDARD.encode(&payload) != self.envelope.payload
        {
            return Err("audit export manifest payload is invalid".into());
        }
        let signature = &self.envelope.signatures[0];
        if signature.key_id != self.signing_key.key_id {
            return Err("audit export manifest signature changed its key ID".into());
        }
        let signature_bytes = STANDARD
            .decode(&signature.signature)
            .map_err(|_| "audit export manifest signature is not canonical base64".to_owned())?;
        if signature_bytes.len() != 64 || STANDARD.encode(&signature_bytes) != signature.signature {
            return Err("audit export manifest signature is invalid".into());
        }
        let pae = dsse_pae_bounded(
            &self.envelope.payload_type,
            &payload,
            MAXIMUM_AUDIT_EXPORT_MANIFEST_BYTES,
        )?;
        UnparsedPublicKey::new(&ED25519, self.signing_key.public_key_bytes()?)
            .verify(&pae, &signature_bytes)
            .map_err(|_| {
                "audit export manifest DSSE Ed25519 signature failed verification".to_owned()
            })?;
        Ok(payload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportManifestBundle {
    pub manifest: AuditExportManifest,
    pub pages: Vec<AuditExport>,
}

impl AuditExportManifestBundle {
    pub fn verify(&self) -> Result<AuditExportManifestDocument, String> {
        let manifest_payload = self.manifest.verify()?;
        let document: AuditExportManifestDocument = serde_json::from_slice(&manifest_payload)
            .map_err(|_| "audit export manifest document is invalid JSON".to_owned())?;
        document.validate()?;
        if canonical_json_bounded(
            &document,
            MAXIMUM_AUDIT_EXPORT_MANIFEST_BYTES,
            "audit export manifest document",
        )? != manifest_payload
            || document.pages.len() != self.pages.len()
        {
            return Err("audit export manifest document is not canonical or complete".into());
        }
        let expected_page_filter = document.filter.page_filter()?;
        let mut record_count = 0_usize;
        for (index, (entry, page)) in document.pages.iter().zip(&self.pages).enumerate() {
            if page.signing_key != self.manifest.signing_key
                || entry.index != index + 1
                || entry.signing_key_id != page.signing_key.key_id
            {
                return Err("audit export manifest bundle changed its signing key".into());
            }
            let payload = page.verify()?;
            if Sha256Digest::from_bytes(&payload) != entry.payload_sha256 {
                return Err("audit export manifest page payload digest does not match".into());
            }
            let page_document: AuditExportDocument = serde_json::from_slice(&payload)
                .map_err(|_| "audit export page document is invalid JSON".to_owned())?;
            page_document.validate()?;
            if canonical_json_bounded(
                &page_document,
                MAXIMUM_AUDIT_EXPORT_BYTES,
                "audit export document",
            )? != payload
                || page_document.organization_id != document.organization_id
                || page_document.filter != expected_page_filter
                || page_document.generated_at != document.generated_at
                || page_document.cursor != entry.cursor
                || page_document.next_cursor != entry.next_cursor
                || page_document.records.len() != entry.record_count
            {
                return Err("audit export manifest page does not match its signed entry".into());
            }
            record_count = record_count
                .checked_add(page_document.records.len())
                .ok_or_else(|| "audit export manifest record count overflowed".to_owned())?;
        }
        if record_count != document.record_count {
            return Err("audit export manifest bundle record count is inconsistent".into());
        }
        Ok(document)
    }
}

fn valid_key_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
