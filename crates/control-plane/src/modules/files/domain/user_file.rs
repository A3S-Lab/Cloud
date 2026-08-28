use super::{UserFileAdmissionContract, UserFileContentReference, UserFileObjectWrite};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PrincipalId, ProjectId, Sha256Digest, UserFileId,
    UserFileUploadId,
};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

pub const USER_FILE_UPLOAD_MAX_TTL_SECONDS: i64 = 24 * 60 * 60;
pub const USER_FILE_RETENTION_MAX_DAYS: i64 = 3_650;
pub const USER_FILE_REJECTION_REASON_MAX_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserFileState {
    AwaitingUpload,
    AwaitingScan,
    Admitted,
    Rejected,
    Expired,
    Tombstoned,
}

impl UserFileState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingUpload => "awaiting_upload",
            Self::AwaitingScan => "awaiting_scan",
            Self::Admitted => "admitted",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Tombstoned => "tombstoned",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UserFileScanDecision {
    Admitted,
    Rejected { reason_code: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFileScanReceipt {
    reference: UserFileContentReference,
    evidence_digest: Sha256Digest,
    decision: UserFileScanDecision,
}

impl UserFileScanReceipt {
    pub fn new(
        reference: UserFileContentReference,
        evidence_digest: Sha256Digest,
        decision: UserFileScanDecision,
    ) -> Result<Self, String> {
        reference.validate()?;
        let evidence_digest = Sha256Digest::parse(evidence_digest.as_str())?;
        if let UserFileScanDecision::Rejected { reason_code } = &decision {
            validate_rejection_reason(reason_code)?;
        }
        Ok(Self {
            reference,
            evidence_digest,
            decision,
        })
    }

    pub const fn reference(&self) -> &UserFileContentReference {
        &self.reference
    }

    pub const fn evidence_digest(&self) -> &Sha256Digest {
        &self.evidence_digest
    }

    pub const fn decision(&self) -> &UserFileScanDecision {
        &self.decision
    }
}

/// Files-owned lifecycle for one bounded upload and its immutable bytes.
///
/// The aggregate owns metadata, upload expiry, scan admission, retention, and
/// optimistic versioning. It deliberately carries no provider, scanner,
/// multipart implementation, or application/Knowledge lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserFile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: UserFileId,
    pub upload_id: UserFileUploadId,
    pub contract: UserFileAdmissionContract,
    pub state: UserFileState,
    pub scan_evidence_digest: Option<Sha256Digest>,
    pub rejection_reason_code: Option<String>,
    pub tombstoned_from: Option<UserFileState>,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub uploaded_at: Option<DateTime<Utc>>,
    pub scanned_at: Option<DateTime<Utc>>,
    pub expired_at: Option<DateTime<Utc>>,
    pub tombstoned_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl UserFile {
    pub fn reserve(
        contract: UserFileAdmissionContract,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        contract.validate()?;
        let created_at = canonical_timestamp(created_at);
        let content = &contract.spec().content;
        let value = Self {
            organization_id: content.organization_id,
            project_id: content.project_id,
            id: content.user_file_id,
            upload_id: content.upload_id,
            contract,
            state: UserFileState::AwaitingUpload,
            scan_evidence_digest: None,
            rejection_reason_code: None,
            tombstoned_from: None,
            aggregate_version: 1,
            created_by,
            created_at,
            uploaded_at: None,
            scanned_at: None,
            expired_at: None,
            tombstoned_at: None,
            updated_at: created_at,
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: UserFileId,
        upload_id: UserFileUploadId,
        canonical_acl: &str,
        stored_contract_digest: &str,
        state: UserFileState,
        scan_evidence_digest: Option<Sha256Digest>,
        rejection_reason_code: Option<String>,
        tombstoned_from: Option<UserFileState>,
        aggregate_version: u64,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
        uploaded_at: Option<DateTime<Utc>>,
        scanned_at: Option<DateTime<Utc>>,
        expired_at: Option<DateTime<Utc>>,
        tombstoned_at: Option<DateTime<Utc>>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            id,
            upload_id,
            contract: UserFileAdmissionContract::restore(canonical_acl, stored_contract_digest)?,
            state,
            scan_evidence_digest,
            rejection_reason_code,
            tombstoned_from,
            aggregate_version,
            created_by,
            created_at: canonical_timestamp(created_at),
            uploaded_at: uploaded_at.map(canonical_timestamp),
            scanned_at: scanned_at.map(canonical_timestamp),
            expired_at: expired_at.map(canonical_timestamp),
            tombstoned_at: tombstoned_at.map(canonical_timestamp),
            updated_at: canonical_timestamp(updated_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn record_upload(
        &self,
        expected_version: u64,
        write: &UserFileObjectWrite,
        uploaded_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.state != UserFileState::AwaitingUpload {
            return Err("UserFile is not awaiting an upload".into());
        }
        if write.reference() != &self.contract.spec().content {
            return Err("UserFile upload receipt does not match its admitted reference".into());
        }
        write.reference().validate()?;
        let uploaded_at = canonical_timestamp(uploaded_at);
        if uploaded_at < self.updated_at || uploaded_at >= self.contract.spec().upload_expires_at {
            return Err("UserFile upload completed outside its admitted session".into());
        }
        let mut value = self.clone();
        value.state = UserFileState::AwaitingScan;
        value.aggregate_version = self.next_version(expected_version)?;
        value.uploaded_at = Some(uploaded_at);
        value.updated_at = uploaded_at;
        value.validate()?;
        Ok(value)
    }

    pub fn record_scan(
        &self,
        expected_version: u64,
        receipt: &UserFileScanReceipt,
        scanned_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.state != UserFileState::AwaitingScan {
            return Err("UserFile is not awaiting a scan decision".into());
        }
        if receipt.reference() != &self.contract.spec().content {
            return Err("UserFile scan receipt does not match its admitted reference".into());
        }
        receipt.reference().validate()?;
        let evidence_digest = Sha256Digest::parse(receipt.evidence_digest().as_str())?;
        let scanned_at = canonical_timestamp(scanned_at);
        if scanned_at < self.updated_at || scanned_at >= self.contract.spec().retention_until {
            return Err("UserFile scan completed outside its retention window".into());
        }
        let (state, rejection_reason_code) = match receipt.decision() {
            UserFileScanDecision::Admitted => (UserFileState::Admitted, None),
            UserFileScanDecision::Rejected { reason_code } => {
                validate_rejection_reason(reason_code)?;
                (UserFileState::Rejected, Some(reason_code.clone()))
            }
        };
        let mut value = self.clone();
        value.state = state;
        value.scan_evidence_digest = Some(evidence_digest);
        value.rejection_reason_code = rejection_reason_code;
        value.aggregate_version = self.next_version(expected_version)?;
        value.scanned_at = Some(scanned_at);
        value.updated_at = scanned_at;
        value.validate()?;
        Ok(value)
    }

    pub fn expire_upload(
        &self,
        expected_version: u64,
        expired_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.state != UserFileState::AwaitingUpload {
            return Err("only an awaiting UserFile upload can expire".into());
        }
        let expired_at = canonical_timestamp(expired_at);
        if expired_at < self.contract.spec().upload_expires_at || expired_at < self.updated_at {
            return Err("UserFile upload cannot expire before its admitted deadline".into());
        }
        let mut value = self.clone();
        value.state = UserFileState::Expired;
        value.aggregate_version = self.next_version(expected_version)?;
        value.expired_at = Some(expired_at);
        value.updated_at = expired_at;
        value.validate()?;
        Ok(value)
    }

    pub fn tombstone(
        &self,
        expected_version: u64,
        tombstoned_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.state == UserFileState::Tombstoned {
            return Err("UserFile is already tombstoned".into());
        }
        let tombstoned_at = canonical_timestamp(tombstoned_at);
        if tombstoned_at < self.updated_at {
            return Err("UserFile tombstone time precedes its current state".into());
        }
        let mut value = self.clone();
        value.tombstoned_from = Some(self.state);
        value.state = UserFileState::Tombstoned;
        value.aggregate_version = self.next_version(expected_version)?;
        value.tombstoned_at = Some(tombstoned_at);
        value.updated_at = tombstoned_at;
        value.validate()?;
        Ok(value)
    }

    pub fn admitted_reference(&self) -> Result<&UserFileContentReference, String> {
        self.validate()?;
        if self.state != UserFileState::Admitted {
            return Err("UserFile bytes have not passed scan admission".into());
        }
        Ok(&self.contract.spec().content)
    }

    /// Whether this lifecycle state still consumes the organization quota.
    ///
    /// Quota is reserved before any bytes are accepted and is released only
    /// by an expiry or tombstone transition. A rejected object remains
    /// allocated until its tombstone makes cleanup explicit.
    pub const fn quota_reserved(&self) -> bool {
        matches!(
            self.state,
            UserFileState::AwaitingUpload
                | UserFileState::AwaitingScan
                | UserFileState::Admitted
                | UserFileState::Rejected
        )
    }

    /// Whether the aggregate proves that immutable bytes reached storage.
    pub const fn has_stored_object(&self) -> bool {
        self.uploaded_at.is_some()
    }

    /// The earliest time at which the shared lifecycle event is actionable as
    /// an object-cleanup intent. Files does not own a second cleanup queue.
    pub fn cleanup_due_at(&self) -> Option<DateTime<Utc>> {
        if !self.has_stored_object() {
            return None;
        }
        match self.state {
            UserFileState::AwaitingScan | UserFileState::Admitted => {
                Some(self.contract.spec().retention_until)
            }
            UserFileState::Rejected | UserFileState::Tombstoned => Some(self.updated_at),
            UserFileState::AwaitingUpload | UserFileState::Expired => None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        let content = &self.contract.spec().content;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.upload_id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.organization_id != content.organization_id
            || self.project_id != content.project_id
            || self.id != content.user_file_id
            || self.upload_id != content.upload_id
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
        {
            return Err("stored UserFile scope, identity, or timestamp is invalid".into());
        }
        for timestamp in [
            self.uploaded_at,
            self.scanned_at,
            self.expired_at,
            self.tombstoned_at,
        ]
        .into_iter()
        .flatten()
        {
            if timestamp != canonical_timestamp(timestamp) {
                return Err("stored UserFile lifecycle timestamp is not canonical".into());
            }
        }
        self.validate_deadlines()?;
        self.validate_lifecycle()
    }

    fn next_version(&self, expected_version: u64) -> Result<u64, String> {
        if expected_version == 0 || expected_version != self.aggregate_version {
            return Err("UserFile was changed from a stale aggregate version".into());
        }
        expected_version
            .checked_add(1)
            .ok_or_else(|| "UserFile aggregate version is exhausted".into())
    }

    fn validate_deadlines(&self) -> Result<(), String> {
        let spec = self.contract.spec();
        let maximum_upload_expiry = self
            .created_at
            .checked_add_signed(TimeDelta::seconds(USER_FILE_UPLOAD_MAX_TTL_SECONDS))
            .ok_or_else(|| "UserFile upload deadline overflowed".to_owned())?;
        let maximum_retention = self
            .created_at
            .checked_add_signed(TimeDelta::days(USER_FILE_RETENTION_MAX_DAYS))
            .ok_or_else(|| "UserFile retention deadline overflowed".to_owned())?;
        if spec.upload_expires_at != canonical_timestamp(spec.upload_expires_at)
            || spec.retention_until != canonical_timestamp(spec.retention_until)
            || spec.upload_expires_at <= self.created_at
            || spec.upload_expires_at > maximum_upload_expiry
            || spec.retention_until <= spec.upload_expires_at
            || spec.retention_until > maximum_retention
        {
            return Err("UserFile upload or retention deadline is outside its bound".into());
        }
        Ok(())
    }

    fn validate_lifecycle(&self) -> Result<(), String> {
        if let Some(digest) = &self.scan_evidence_digest {
            if Sha256Digest::parse(digest.as_str())? != *digest {
                return Err("UserFile scan evidence digest is not canonical".into());
            }
        }
        if let Some(reason) = &self.rejection_reason_code {
            validate_rejection_reason(reason)?;
        }
        match self.state {
            UserFileState::AwaitingUpload => {
                self.expect_phase(1, false, false, false, false, false, false)
            }
            UserFileState::AwaitingScan => {
                self.expect_phase(2, true, false, false, false, false, false)
            }
            UserFileState::Admitted => self.expect_phase(3, true, true, true, false, false, false),
            UserFileState::Rejected => self.expect_phase(3, true, true, true, true, false, false),
            UserFileState::Expired => self.expect_phase(2, false, false, false, false, true, false),
            UserFileState::Tombstoned => self.validate_tombstone(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn expect_phase(
        &self,
        version: u64,
        uploaded: bool,
        scanned: bool,
        evidence: bool,
        rejection: bool,
        expired: bool,
        tombstoned: bool,
    ) -> Result<(), String> {
        let latest = self
            .tombstoned_at
            .or(self.expired_at)
            .or(self.scanned_at)
            .or(self.uploaded_at)
            .unwrap_or(self.created_at);
        if self.aggregate_version != version
            || self.uploaded_at.is_some() != uploaded
            || self.scanned_at.is_some() != scanned
            || self.scan_evidence_digest.is_some() != evidence
            || self.rejection_reason_code.is_some() != rejection
            || self.expired_at.is_some() != expired
            || self.tombstoned_at.is_some() != tombstoned
            || self.tombstoned_from.is_some() != tombstoned
            || latest != self.updated_at
            || self.uploaded_at.is_some_and(|at| {
                at < self.created_at || at >= self.contract.spec().upload_expires_at
            })
            || self
                .scanned_at
                .zip(self.uploaded_at)
                .is_some_and(|(scan, upload)| {
                    scan < upload || scan >= self.contract.spec().retention_until
                })
            || self
                .expired_at
                .is_some_and(|at| at < self.contract.spec().upload_expires_at)
        {
            return Err("stored UserFile lifecycle is internally inconsistent".into());
        }
        Ok(())
    }

    fn validate_tombstone(&self) -> Result<(), String> {
        let Some(previous) = self.tombstoned_from else {
            return Err("tombstoned UserFile is missing its prior state".into());
        };
        if previous == UserFileState::Tombstoned {
            return Err("UserFile cannot be tombstoned from a tombstone".into());
        }
        let Some(tombstoned_at) = self.tombstoned_at else {
            return Err("tombstoned UserFile is missing its timestamp".into());
        };
        let prior_updated_at = self
            .expired_at
            .or(self.scanned_at)
            .or(self.uploaded_at)
            .unwrap_or(self.created_at);
        if tombstoned_at < prior_updated_at || tombstoned_at != self.updated_at {
            return Err("UserFile tombstone time is inconsistent".into());
        }
        let (version, uploaded, scanned, evidence, rejection, expired) = match previous {
            UserFileState::AwaitingUpload => (2, false, false, false, false, false),
            UserFileState::AwaitingScan => (3, true, false, false, false, false),
            UserFileState::Admitted => (4, true, true, true, false, false),
            UserFileState::Rejected => (4, true, true, true, true, false),
            UserFileState::Expired => (3, false, false, false, false, true),
            UserFileState::Tombstoned => unreachable!(),
        };
        if self.aggregate_version != version
            || self.uploaded_at.is_some() != uploaded
            || self.scanned_at.is_some() != scanned
            || self.scan_evidence_digest.is_some() != evidence
            || self.rejection_reason_code.is_some() != rejection
            || self.expired_at.is_some() != expired
        {
            return Err("tombstoned UserFile does not preserve its prior lifecycle".into());
        }
        if let Some(uploaded_at) = self.uploaded_at {
            if uploaded_at < self.created_at
                || uploaded_at >= self.contract.spec().upload_expires_at
            {
                return Err("tombstoned UserFile preserved an invalid upload time".into());
            }
        }
        if let Some(scanned_at) = self.scanned_at {
            let Some(uploaded_at) = self.uploaded_at else {
                return Err("tombstoned UserFile scan has no upload".into());
            };
            if scanned_at < uploaded_at || scanned_at >= self.contract.spec().retention_until {
                return Err("tombstoned UserFile preserved an invalid scan time".into());
            }
        }
        if self
            .expired_at
            .is_some_and(|at| at < self.contract.spec().upload_expires_at)
        {
            return Err("tombstoned UserFile preserved an invalid expiry".into());
        }
        Ok(())
    }
}

fn validate_rejection_reason(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > USER_FILE_REJECTION_REASON_MAX_BYTES
        || !value.is_ascii()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        return Err("UserFile rejection reason must be a bounded lowercase code".into());
    }
    Ok(())
}
