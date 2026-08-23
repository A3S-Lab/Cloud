use super::{
    AuditAttributionStatus, AuditRecord, AuditRecordCursor, AuditRecordFilter, AuditRecordPage,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, dsse_pae_bounded, EnvironmentId, OrganizationId,
    PrincipalId, ProjectAttributionProfileId, ProjectId,
};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use uuid::Uuid;

pub const AUDIT_EXPORT_SCHEMA: &str = "a3s.cloud.audit-export.v1";
pub const AUDIT_EXPORT_PAYLOAD_TYPE: &str = "application/vnd.a3s.cloud.audit-export.v1+json";
pub const MAXIMUM_AUDIT_EXPORT_BYTES: usize = 1024 * 1024;
pub const MAXIMUM_AUDIT_EXPORT_WINDOW_DAYS: i64 = 31;
const MAXIMUM_AUDIT_EXPORT_RECORDS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportFilter {
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
    pub limit: usize,
}

impl AuditExportFilter {
    pub fn from_record_filter(filter: &AuditRecordFilter, limit: usize) -> Result<Self, String> {
        filter.validate()?;
        if limit == 0 || limit > MAXIMUM_AUDIT_EXPORT_RECORDS {
            return Err(format!(
                "audit export limit must be between 1 and {MAXIMUM_AUDIT_EXPORT_RECORDS}"
            ));
        }
        let from = filter
            .from
            .ok_or_else(|| "audit export from timestamp is required".to_owned())?;
        let to = filter
            .to
            .ok_or_else(|| "audit export to timestamp is required".to_owned())?;
        if from != canonical_timestamp(from) || to != canonical_timestamp(to) {
            return Err("audit export timestamps must use canonical microsecond precision".into());
        }
        if to.signed_duration_since(from) > Duration::days(MAXIMUM_AUDIT_EXPORT_WINDOW_DAYS) {
            return Err(format!(
                "audit export window must not exceed {MAXIMUM_AUDIT_EXPORT_WINDOW_DAYS} days"
            ));
        }
        Ok(Self {
            actor_principal_id: filter.actor_principal_id,
            action: filter.action.clone(),
            aggregate_id: filter.aggregate_id,
            request_id: filter.request_id,
            project_id: filter.project_id,
            environment_id: filter.environment_id,
            attribution_profile_id: filter.attribution_profile_id,
            attribution_status: filter.attribution_status,
            from,
            to,
            limit,
        })
    }

    pub(super) fn record_filter(&self) -> AuditRecordFilter {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportRecord {
    pub id: Uuid,
    pub organization_id: OrganizationId,
    pub actor_principal_id: Option<PrincipalId>,
    pub action: String,
    pub aggregate_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub project_id: Option<ProjectId>,
    pub environment_id: Option<EnvironmentId>,
    pub attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub attribution_status: AuditAttributionStatus,
}

impl From<AuditRecord> for AuditExportRecord {
    fn from(record: AuditRecord) -> Self {
        Self {
            id: record.id,
            organization_id: record.organization_id,
            actor_principal_id: record.actor_principal_id,
            action: record.action,
            aggregate_id: record.aggregate_id,
            occurred_at: record.occurred_at,
            request_id: record.request_id,
            project_id: record.project_id,
            environment_id: record.environment_id,
            attribution_profile_id: record.attribution_profile_id,
            attribution_status: record.attribution_status,
        }
    }
}

impl AuditExportRecord {
    fn audit_record(&self) -> AuditRecord {
        AuditRecord {
            id: self.id,
            organization_id: self.organization_id,
            actor_principal_id: self.actor_principal_id,
            action: self.action.clone(),
            aggregate_id: self.aggregate_id,
            occurred_at: self.occurred_at,
            request_id: self.request_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            attribution_profile_id: self.attribution_profile_id,
            attribution_status: self.attribution_status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportDocument {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub filter: AuditExportFilter,
    pub cursor: Option<String>,
    pub generated_at: DateTime<Utc>,
    pub records: Vec<AuditExportRecord>,
    pub next_cursor: Option<String>,
}

impl AuditExportDocument {
    pub fn from_page(
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        cursor: Option<String>,
        limit: usize,
        generated_at: DateTime<Utc>,
        page: AuditRecordPage,
    ) -> Result<Self, String> {
        let document = Self {
            schema: AUDIT_EXPORT_SCHEMA.into(),
            organization_id,
            filter: AuditExportFilter::from_record_filter(filter, limit)?,
            cursor,
            generated_at: canonical_timestamp(generated_at),
            records: page
                .records
                .into_iter()
                .map(AuditExportRecord::from)
                .collect(),
            next_cursor: page.next_cursor,
        };
        document.validate()?;
        Ok(document)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_EXPORT_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.generated_at != canonical_timestamp(self.generated_at)
            || self.filter.limit == 0
            || self.filter.limit > MAXIMUM_AUDIT_EXPORT_RECORDS
            || self.records.len() > self.filter.limit
        {
            return Err("audit export document identity or bounds are invalid".into());
        }
        let record_filter = self.filter.record_filter();
        AuditExportFilter::from_record_filter(&record_filter, self.filter.limit)?;
        let input_cursor = self
            .cursor
            .as_deref()
            .map(AuditRecordCursor::parse)
            .transpose()?;
        for record in &self.records {
            let record = record.audit_record();
            record.validate()?;
            if record.organization_id != self.organization_id
                || !record_filter.matches(&record)
                || input_cursor.is_some_and(|cursor| {
                    record.occurred_at > cursor.occurred_at
                        || (record.occurred_at == cursor.occurred_at
                            && record.id >= cursor.audit_id)
                })
            {
                return Err("audit export record does not match its signed selection".into());
            }
        }
        if self.records.windows(2).any(|pair| {
            pair[0].occurred_at < pair[1].occurred_at
                || (pair[0].occurred_at == pair[1].occurred_at && pair[0].id <= pair[1].id)
        }) {
            return Err("audit export records are not in strict descending keyset order".into());
        }
        match (self.next_cursor.as_deref(), self.records.last()) {
            (Some(cursor), Some(last))
                if AuditRecordCursor::parse(cursor)?
                    == AuditRecordCursor {
                        occurred_at: last.occurred_at,
                        audit_id: last.id,
                    } => {}
            (None, _) => {}
            _ => return Err("audit export continuation cursor is invalid".into()),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportSigningKey {
    pub algorithm: String,
    pub key_id: String,
    pub public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_version: Option<u32>,
}

impl AuditExportSigningKey {
    pub fn validate(&self) -> Result<(), String> {
        if self.algorithm != "ed25519"
            || self.key_version == Some(0)
            || self.key_id.len() != 64
            || !self
                .key_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("audit export signing key identity is invalid".into());
        }
        let public_key = self.public_key_bytes()?;
        if format!("{:x}", sha2::Sha256::digest(public_key)) != self.key_id {
            return Err("audit export signing key ID does not match its public key".into());
        }
        Ok(())
    }

    pub(super) fn public_key_bytes(&self) -> Result<[u8; 32], String> {
        let bytes = STANDARD
            .decode(&self.public_key)
            .map_err(|_| "audit export public key is not canonical base64".to_owned())?;
        if bytes.len() != 32 || STANDARD.encode(&bytes) != self.public_key {
            return Err("audit export public key is invalid".into());
        }
        bytes
            .try_into()
            .map_err(|_| "audit export public key is invalid".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedAuditExportSignature {
    pub key: AuditExportSigningKey,
    pub signature: Vec<u8>,
}

impl VerifiedAuditExportSignature {
    pub fn new(
        key: AuditExportSigningKey,
        signature: Vec<u8>,
    ) -> Result<Self, AuditExportSigningError> {
        key.validate().map_err(AuditExportSigningError::Rejected)?;
        if signature.len() != 64 {
            return Err(AuditExportSigningError::Rejected(
                "audit export Ed25519 signature must contain exactly 64 bytes".into(),
            ));
        }
        Ok(Self { key, signature })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditExportSigningError {
    #[error("audit export signing input is invalid: {0}")]
    Invalid(String),
    #[error("audit export signer is temporarily unavailable: {0}")]
    Unavailable(String),
    #[error("audit export signer rejected or failed verification: {0}")]
    Rejected(String),
}

#[async_trait]
pub trait IAuditExportSigner: Send + Sync {
    async fn sign(
        &self,
        pae: &[u8],
    ) -> Result<VerifiedAuditExportSignature, AuditExportSigningError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportDsseSignature {
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExportDsseEnvelope {
    pub payload_type: String,
    pub payload: String,
    pub signatures: Vec<AuditExportDsseSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditExport {
    pub envelope: AuditExportDsseEnvelope,
    pub signing_key: AuditExportSigningKey,
}

impl AuditExport {
    pub fn signable_bytes(document: &AuditExportDocument) -> Result<(Vec<u8>, Vec<u8>), String> {
        document.validate()?;
        let payload = canonical_json_bounded(
            document,
            MAXIMUM_AUDIT_EXPORT_BYTES,
            "audit export document",
        )?;
        let pae = dsse_pae_bounded(
            AUDIT_EXPORT_PAYLOAD_TYPE,
            &payload,
            MAXIMUM_AUDIT_EXPORT_BYTES,
        )?;
        Ok((payload, pae))
    }

    pub fn from_signature(
        payload: Vec<u8>,
        signature: VerifiedAuditExportSignature,
    ) -> Result<Self, String> {
        let export = Self {
            envelope: AuditExportDsseEnvelope {
                payload_type: AUDIT_EXPORT_PAYLOAD_TYPE.into(),
                payload: STANDARD.encode(payload),
                signatures: vec![AuditExportDsseSignature {
                    key_id: signature.key.key_id.clone(),
                    signature: STANDARD.encode(signature.signature),
                }],
            },
            signing_key: signature.key,
        };
        export.verify()?;
        Ok(export)
    }

    pub fn verify(&self) -> Result<Vec<u8>, String> {
        self.signing_key.validate()?;
        if self.envelope.payload_type != AUDIT_EXPORT_PAYLOAD_TYPE
            || self.envelope.signatures.len() != 1
        {
            return Err("audit export DSSE envelope shape is invalid".into());
        }
        let payload = STANDARD
            .decode(&self.envelope.payload)
            .map_err(|_| "audit export DSSE payload is not canonical base64".to_owned())?;
        if payload.len() > MAXIMUM_AUDIT_EXPORT_BYTES
            || STANDARD.encode(&payload) != self.envelope.payload
        {
            return Err("audit export DSSE payload is invalid".into());
        }
        let signature = &self.envelope.signatures[0];
        if signature.key_id != self.signing_key.key_id {
            return Err("audit export DSSE signature changed its key ID".into());
        }
        let signature_bytes = STANDARD
            .decode(&signature.signature)
            .map_err(|_| "audit export DSSE signature is not canonical base64".to_owned())?;
        if signature_bytes.len() != 64 || STANDARD.encode(&signature_bytes) != signature.signature {
            return Err("audit export DSSE signature is invalid".into());
        }
        let pae = dsse_pae_bounded(
            &self.envelope.payload_type,
            &payload,
            MAXIMUM_AUDIT_EXPORT_BYTES,
        )?;
        UnparsedPublicKey::new(&ED25519, self.signing_key.public_key_bytes()?)
            .verify(&pae, &signature_bytes)
            .map_err(|_| "audit export DSSE Ed25519 signature failed verification".to_owned())?;
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::canonical_timestamp;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn fixture_document() -> AuditExportDocument {
        let organization_id = OrganizationId::new();
        let occurred_at = "2026-08-13T01:02:03Z"
            .parse::<DateTime<Utc>>()
            .expect("timestamp");
        let filter = AuditRecordFilter {
            from: Some(
                "2026-08-01T00:00:00Z"
                    .parse::<DateTime<Utc>>()
                    .expect("from"),
            ),
            to: Some("2026-08-31T00:00:00Z".parse::<DateTime<Utc>>().expect("to")),
            ..AuditRecordFilter::default()
        };
        AuditExportDocument::from_page(
            organization_id,
            &filter,
            None,
            50,
            "2026-08-13T01:03:00Z"
                .parse::<DateTime<Utc>>()
                .expect("generated at"),
            AuditRecordPage {
                records: vec![AuditRecord {
                    id: Uuid::now_v7(),
                    organization_id,
                    actor_principal_id: Some(PrincipalId::new()),
                    action: "identity.membership.created".into(),
                    aggregate_id: Uuid::now_v7(),
                    occurred_at: canonical_timestamp(occurred_at),
                    request_id: Uuid::now_v7(),
                    project_id: Some(ProjectId::new()),
                    environment_id: Some(EnvironmentId::new()),
                    attribution_profile_id: Some(ProjectAttributionProfileId::new()),
                    attribution_status: AuditAttributionStatus::ProfileBound,
                }],
                next_cursor: None,
            },
        )
        .expect("audit export document")
    }

    fn sign(document: &AuditExportDocument) -> AuditExport {
        let (payload, pae) = AuditExport::signable_bytes(document).expect("signable bytes");
        let key = Ed25519KeyPair::from_seed_unchecked(&[0x51; 32]).expect("Ed25519 key");
        let public_key = key.public_key().as_ref();
        let signature = VerifiedAuditExportSignature::new(
            AuditExportSigningKey {
                algorithm: "ed25519".into(),
                key_id: format!("{:x}", sha2::Sha256::digest(public_key)),
                public_key: STANDARD.encode(public_key),
                key_version: None,
            },
            key.sign(&pae).as_ref().to_vec(),
        )
        .expect("verified signature");
        AuditExport::from_signature(payload, signature).expect("signed export")
    }

    #[test]
    fn canonical_redacted_document_round_trips_through_offline_dsse_verification() {
        let document = fixture_document();
        let first = AuditExport::signable_bytes(&document)
            .expect("first bytes")
            .0;
        let second = AuditExport::signable_bytes(&document)
            .expect("second bytes")
            .0;
        assert_eq!(first, second);

        let export = sign(&document);
        assert_eq!(export.verify().expect("offline verification"), first);
        let value: serde_json::Value = serde_json::from_slice(&first).expect("document JSON");
        assert_eq!(value["schema"], AUDIT_EXPORT_SCHEMA);
        assert_eq!(
            value["records"][0].as_object().map(serde_json::Map::len),
            Some(11)
        );
        for private in [
            "details",
            "labels",
            "businessOwnerReference",
            "costAttributionCode",
        ] {
            assert!(!value.to_string().contains(private));
        }
    }

    #[test]
    fn tampering_or_an_invalid_export_window_is_rejected() {
        let export = sign(&fixture_document());
        let mut payload_tampered = export.clone();
        payload_tampered.envelope.payload = STANDARD.encode(b"{}");
        assert!(payload_tampered.verify().is_err());
        let mut signature_tampered = export;
        signature_tampered.envelope.signatures[0].signature = STANDARD.encode([0_u8; 64]);
        assert!(signature_tampered.verify().is_err());

        let missing_from = AuditRecordFilter {
            to: Some(Utc::now()),
            ..AuditRecordFilter::default()
        };
        assert!(AuditExportFilter::from_record_filter(&missing_from, 50).is_err());
        let from = canonical_timestamp(Utc::now());
        let oversized = AuditRecordFilter {
            from: Some(from),
            to: Some(from + Duration::days(MAXIMUM_AUDIT_EXPORT_WINDOW_DAYS + 1)),
            ..AuditRecordFilter::default()
        };
        assert!(AuditExportFilter::from_record_filter(&oversized, 50)
            .expect_err("oversized window")
            .contains("31 days"));
    }
}
