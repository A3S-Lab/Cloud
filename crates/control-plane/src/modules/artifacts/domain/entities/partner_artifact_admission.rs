//! ArtifactAdmission receipt. Digest identity only — no partner blob bytes.

use super::partner_artifact_kind::PartnerArtifactKind;
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, Sha256Digest};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Upper bound: 8 TiB. Larger objects stay on the shared object service, not this receipt.
pub const MAX_PARTNER_ARTIFACT_BYTE_SIZE: u64 = 8 * 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartnerArtifactAdmissionId(Uuid);

impl PartnerArtifactAdmissionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for PartnerArtifactAdmissionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartnerArtifactAdmission {
    pub id: PartnerArtifactAdmissionId,
    pub organization_id: OrganizationId,
    pub content_digest: Sha256Digest,
    pub kind: PartnerArtifactKind,
    pub byte_size: u64,
    pub partner_ref: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
}

impl PartnerArtifactAdmission {
    pub fn create(
        id: PartnerArtifactAdmissionId,
        organization_id: OrganizationId,
        content_digest: Sha256Digest,
        kind: PartnerArtifactKind,
        byte_size: u64,
        partner_ref: String,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if byte_size == 0 || byte_size > MAX_PARTNER_ARTIFACT_BYTE_SIZE {
            return Err(format!(
                "partner artifact byte size must be between 1 and {MAX_PARTNER_ARTIFACT_BYTE_SIZE}"
            ));
        }
        let partner_ref = partner_ref.trim().to_owned();
        if partner_ref.is_empty()
            || partner_ref.len() > 256
            || partner_ref.chars().any(|ch| ch.is_control())
        {
            return Err(
                "partner artifact ref must be 1..=256 characters without control chars".into(),
            );
        }
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            organization_id,
            content_digest,
            kind,
            byte_size,
            partner_ref,
            aggregate_version: 1,
            created_at,
        })
    }

    pub fn same_facts(&self, other: &Self) -> bool {
        self.organization_id == other.organization_id
            && self.content_digest == other.content_digest
            && self.kind == other.kind
            && self.byte_size == other.byte_size
            && self.partner_ref == other.partner_ref
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_size_and_control_refs() {
        let digest = Sha256Digest::parse(format!("sha256:{}", "ab".repeat(32))).expect("digest");
        assert!(PartnerArtifactAdmission::create(
            PartnerArtifactAdmissionId::new(),
            OrganizationId::new(),
            digest.clone(),
            PartnerArtifactKind::Model,
            0,
            "weights".into(),
            Utc::now(),
        )
        .is_err());
        assert!(PartnerArtifactAdmission::create(
            PartnerArtifactAdmissionId::new(),
            OrganizationId::new(),
            digest,
            PartnerArtifactKind::Git,
            12,
            "bad\nref".into(),
            Utc::now(),
        )
        .is_err());
    }

    #[test]
    fn migration_stores_digest_not_bytes() {
        let sql =
            include_str!("../../../../../../../migrations/214_partner_artifact_admissions.sql");
        let lower = sql.to_ascii_lowercase();
        assert!(!lower.contains("bytea"));
        assert!(lower.contains("content_digest"));
        assert!(lower.contains("partner_artifact_admissions_org_digest_unique"));
    }
}
