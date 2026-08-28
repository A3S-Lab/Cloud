use super::{
    UserFile, UserFileLifecycleChanged, UserFileObjectWrite, UserFileQuota, UserFileScanDecision,
    UserFileScanReceipt, UserFileState,
};
use crate::modules::shared_kernel::domain::{
    validate_audit_action, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, UserFileId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReserveUserFileWrite {
    pub file: UserFile,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl ReserveUserFileWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.file.validate()?;
        self.idempotency.validate()?;
        if self.file.state != UserFileState::AwaitingUpload
            || self.file.aggregate_version != 1
            || self.actor_principal_id.as_uuid().is_nil()
            || self.actor_principal_id != self.file.created_by
            || self.request_id.is_nil()
        {
            return Err("initial UserFile persistence write is invalid".into());
        }
        validate_audit_action(self.audit_action())?;
        validate_event(&self.event, &self.file, self.request_id)
    }

    pub const fn audit_action(&self) -> &'static str {
        user_file_audit_action(self.file.state)
    }
}

#[derive(Debug, Clone)]
pub struct TransitionUserFileWrite {
    pub file: UserFile,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

impl TransitionUserFileWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.file.validate()?;
        self.idempotency.validate()?;
        if self.expected_version == 0
            || self.file.aggregate_version != self.expected_version.saturating_add(1)
            || self.file.state == UserFileState::AwaitingUpload
            || self.actor_principal_id.as_uuid().is_nil()
            || self.request_id.is_nil()
        {
            return Err("UserFile lifecycle persistence write is invalid".into());
        }
        validate_audit_action(self.audit_action())?;
        validate_event(&self.event, &self.file, self.request_id)
    }

    pub fn validate_against(&self, current: &UserFile) -> Result<(), String> {
        self.validate()?;
        current.validate()?;
        if current.aggregate_version != self.expected_version {
            return Err("UserFile changed while applying its lifecycle transition".into());
        }
        let expected = expected_successor(current, &self.file)?;
        if expected != self.file {
            return Err("UserFile lifecycle transition changed immutable state".into());
        }
        Ok(())
    }

    pub const fn audit_action(&self) -> &'static str {
        user_file_audit_action(self.file.state)
    }
}

const fn user_file_audit_action(state: UserFileState) -> &'static str {
    match state {
        UserFileState::AwaitingUpload => "file.user-file.reserved",
        UserFileState::AwaitingScan => "file.user-file.upload-recorded",
        UserFileState::Admitted => "file.user-file.admitted",
        UserFileState::Rejected => "file.user-file.rejected",
        UserFileState::Expired => "file.user-file.expired",
        UserFileState::Tombstoned => "file.user-file.tombstoned",
    }
}

#[async_trait]
pub trait IUserFileRepository: Send + Sync {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<UserFile>>, RepositoryError>;

    async fn reserve(
        &self,
        write: ReserveUserFileWrite,
    ) -> Result<IdempotentWrite<UserFile>, RepositoryError>;

    async fn transition(
        &self,
        write: TransitionUserFileWrite,
    ) -> Result<IdempotentWrite<UserFile>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        user_file_id: UserFileId,
    ) -> Result<Option<UserFile>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<UserFile>, RepositoryError>;

    async fn quota(
        &self,
        organization_id: OrganizationId,
    ) -> Result<UserFileQuota, RepositoryError>;
}

fn expected_successor(current: &UserFile, next: &UserFile) -> Result<UserFile, String> {
    match next.state {
        UserFileState::AwaitingUpload => {
            Err("UserFile cannot transition back to reservation".into())
        }
        UserFileState::AwaitingScan => current.record_upload(
            current.aggregate_version,
            &UserFileObjectWrite::stored(current.contract.spec().content.clone(), false),
            next.uploaded_at
                .ok_or_else(|| "uploaded UserFile is missing its timestamp".to_owned())?,
        ),
        UserFileState::Admitted | UserFileState::Rejected => {
            let decision = if next.state == UserFileState::Admitted {
                UserFileScanDecision::Admitted
            } else {
                UserFileScanDecision::Rejected {
                    reason_code: next
                        .rejection_reason_code
                        .clone()
                        .ok_or_else(|| "rejected UserFile is missing its reason".to_owned())?,
                }
            };
            let receipt = UserFileScanReceipt::new(
                current.contract.spec().content.clone(),
                next.scan_evidence_digest
                    .clone()
                    .ok_or_else(|| "scanned UserFile is missing its evidence".to_owned())?,
                decision,
            )?;
            current.record_scan(
                current.aggregate_version,
                &receipt,
                next.scanned_at
                    .ok_or_else(|| "scanned UserFile is missing its timestamp".to_owned())?,
            )
        }
        UserFileState::Expired => current.expire_upload(
            current.aggregate_version,
            next.expired_at
                .ok_or_else(|| "expired UserFile is missing its timestamp".to_owned())?,
        ),
        UserFileState::Tombstoned => current.tombstone(
            current.aggregate_version,
            next.tombstoned_at
                .ok_or_else(|| "tombstoned UserFile is missing its timestamp".to_owned())?,
        ),
    }
}

fn validate_event(
    event: &DomainEventEnvelope,
    file: &UserFile,
    request_id: Uuid,
) -> Result<(), String> {
    if event.event_id.is_nil() || event.correlation_id != request_id {
        return Err("UserFile lifecycle event identity is invalid".into());
    }
    let expected = UserFileLifecycleChanged::changed(file, request_id, event.causation_id)?;
    if event.event_key != expected.event_key
        || event.schema_version != expected.schema_version
        || event.organization_id() != expected.organization_id()
        || event.aggregate_id != expected.aggregate_id
        || event.aggregate_version != expected.aggregate_version
        || event.occurred_at != expected.occurred_at
        || event.payload != expected.payload
        || !matches!(event.payload, Value::Object(_))
    {
        return Err("UserFile lifecycle event and aggregate are inconsistent".into());
    }
    Ok(())
}
