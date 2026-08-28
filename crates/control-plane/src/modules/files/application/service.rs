use super::resource_access::{organization_quota, project, user_file_not_found};
use super::{IUserFileObjectStore, UserFileObjectError, UserFileObjectReader};
use crate::modules::files::domain::{
    IUserFileRepository, ReserveUserFileWrite, TransitionUserFileWrite, UserFile,
    UserFileAdmissionContract, UserFileLifecycleChanged, UserFileQuota, UserFileScanDecision,
    UserFileScanReceipt, UserFileState,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
    UserFileId,
};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

pub const DEFAULT_USER_FILE_LIST_LIMIT: usize = 50;
pub const MAXIMUM_USER_FILE_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFileMutationResult {
    pub file: UserFile,
    pub replayed: bool,
}

pub struct UserFileApplicationService {
    files: Arc<dyn IUserFileRepository>,
    objects: Arc<dyn IUserFileObjectStore>,
}

impl UserFileApplicationService {
    pub fn new(
        files: Arc<dyn IUserFileRepository>,
        objects: Arc<dyn IUserFileObjectStore>,
    ) -> Self {
        Self { files, objects }
    }

    pub async fn reserve(
        &self,
        request: ReserveUserFile,
    ) -> ApplicationResult<UserFileMutationResult> {
        project(request.project_id, &request.resource_access)?;
        let contract = UserFileAdmissionContract::parse_acl(&request.admission_acl)
            .map_err(ApplicationError::Invalid)?;
        if contract.spec().content.organization_id != request.organization_id
            || contract.spec().content.project_id != request.project_id
        {
            return Err(ApplicationError::Invalid(
                "UserFile admission ACL is outside the requested tenant scope".into(),
            ));
        }
        let idempotency = idempotency(
            format!(
                "organizations/{}/projects/{}/user-files",
                request.organization_id, request.project_id
            ),
            request.idempotency_key,
            &CanonicalReserveUserFile {
                organization_id: request.organization_id,
                project_id: request.project_id,
                actor_principal_id: request.actor_principal_id,
                contract_digest: contract.digest().as_str(),
            },
        )?;
        if let Some(replay) = self.files.replay_write(&idempotency).await? {
            if replay.value.organization_id != request.organization_id
                || replay.value.project_id != request.project_id
                || replay.value.created_by != request.actor_principal_id
                || replay.value.contract.digest() != contract.digest()
                || replay.value.state != UserFileState::AwaitingUpload
            {
                return Err(ApplicationError::Internal(
                    "UserFile reservation replay is inconsistent".into(),
                ));
            }
            return Ok(UserFileMutationResult {
                file: replay.value,
                replayed: true,
            });
        }
        let file = UserFile::reserve(contract, request.actor_principal_id, Utc::now())
            .map_err(ApplicationError::Invalid)?;
        let event = UserFileLifecycleChanged::changed(&file, request.request_id, None)
            .map_err(ApplicationError::Internal)?;
        let result = self
            .files
            .reserve(ReserveUserFileWrite {
                file,
                event,
                actor_principal_id: request.actor_principal_id,
                request_id: request.request_id,
                idempotency,
            })
            .await?;
        Ok(UserFileMutationResult {
            file: result.value,
            replayed: result.replayed,
        })
    }

    pub async fn record_upload(
        &self,
        request: RecordUserFileUpload,
    ) -> ApplicationResult<UserFileMutationResult> {
        let RecordUserFileUpload { transition, reader } = request;
        project(transition.project_id, &transition.resource_access)?;
        let idempotency = transition_idempotency(
            &transition,
            "upload",
            &CanonicalTransition {
                organization_id: transition.organization_id,
                project_id: transition.project_id,
                user_file_id: transition.user_file_id,
                expected_version: transition.expected_version,
                actor_principal_id: transition.actor_principal_id,
                evidence: None,
            },
        )?;
        if let Some(result) = self
            .transition_replay(&transition, &idempotency, UserFileState::AwaitingScan)
            .await?
        {
            return Ok(result);
        }
        let current = self.load_transition_file(&transition).await?;
        let write = self
            .objects
            .put(&current.contract.spec().content, reader)
            .await
            .map_err(object_error)?;
        let next = current
            .record_upload(transition.expected_version, &write, Utc::now())
            .map_err(ApplicationError::Conflict)?;
        self.persist_transition(transition, idempotency, next).await
    }

    pub async fn record_scan(
        &self,
        request: RecordUserFileScan,
    ) -> ApplicationResult<UserFileMutationResult> {
        project(
            request.transition.project_id,
            &request.transition.resource_access,
        )?;
        let evidence_digest =
            Sha256Digest::parse(&request.evidence_digest).map_err(ApplicationError::Invalid)?;
        let decision = request.decision.clone();
        let evidence = serde_json::to_value((&evidence_digest, &decision))
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = transition_idempotency(
            &request.transition,
            "scan",
            &CanonicalTransition {
                organization_id: request.transition.organization_id,
                project_id: request.transition.project_id,
                user_file_id: request.transition.user_file_id,
                expected_version: request.transition.expected_version,
                actor_principal_id: request.transition.actor_principal_id,
                evidence: Some(evidence),
            },
        )?;
        let expected_state = match &decision {
            UserFileScanDecision::Admitted => UserFileState::Admitted,
            UserFileScanDecision::Rejected { .. } => UserFileState::Rejected,
        };
        if let Some(result) = self
            .transition_replay(&request.transition, &idempotency, expected_state)
            .await?
        {
            return Ok(result);
        }
        let current = self.load_transition_file(&request.transition).await?;
        self.objects
            .verify(&current.contract.spec().content)
            .await
            .map_err(object_error)?;
        let receipt = UserFileScanReceipt::new(
            current.contract.spec().content.clone(),
            evidence_digest,
            decision,
        )
        .map_err(ApplicationError::Invalid)?;
        let next = current
            .record_scan(request.transition.expected_version, &receipt, Utc::now())
            .map_err(ApplicationError::Conflict)?;
        self.persist_transition(request.transition, idempotency, next)
            .await
    }

    pub async fn expire_upload(
        &self,
        request: UserFileTransition,
    ) -> ApplicationResult<UserFileMutationResult> {
        project(request.project_id, &request.resource_access)?;
        let idempotency = transition_idempotency(
            &request,
            "expire",
            &CanonicalTransition {
                organization_id: request.organization_id,
                project_id: request.project_id,
                user_file_id: request.user_file_id,
                expected_version: request.expected_version,
                actor_principal_id: request.actor_principal_id,
                evidence: None,
            },
        )?;
        if let Some(result) = self
            .transition_replay(&request, &idempotency, UserFileState::Expired)
            .await?
        {
            return Ok(result);
        }
        let current = self.load_transition_file(&request).await?;
        let next = current
            .expire_upload(request.expected_version, Utc::now())
            .map_err(ApplicationError::Conflict)?;
        self.persist_transition(request, idempotency, next).await
    }

    pub async fn tombstone(
        &self,
        request: UserFileTransition,
    ) -> ApplicationResult<UserFileMutationResult> {
        project(request.project_id, &request.resource_access)?;
        let idempotency = transition_idempotency(
            &request,
            "tombstone",
            &CanonicalTransition {
                organization_id: request.organization_id,
                project_id: request.project_id,
                user_file_id: request.user_file_id,
                expected_version: request.expected_version,
                actor_principal_id: request.actor_principal_id,
                evidence: None,
            },
        )?;
        if let Some(result) = self
            .transition_replay(&request, &idempotency, UserFileState::Tombstoned)
            .await?
        {
            return Ok(result);
        }
        let current = self.load_transition_file(&request).await?;
        let next = current
            .tombstone(request.expected_version, Utc::now())
            .map_err(ApplicationError::Conflict)?;
        self.persist_transition(request, idempotency, next).await
    }

    pub async fn get(&self, request: GetUserFile) -> ApplicationResult<UserFile> {
        project(request.project_id, &request.resource_access)?;
        match self
            .files
            .find(
                request.organization_id,
                request.project_id,
                request.user_file_id,
            )
            .await
        {
            Ok(Some(file)) => Ok(file),
            Ok(None) | Err(RepositoryError::NotFound) => Err(user_file_not_found()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn list(&self, request: ListUserFiles) -> ApplicationResult<Vec<UserFile>> {
        project(request.project_id, &request.resource_access)?;
        let limit = request.limit.unwrap_or(DEFAULT_USER_FILE_LIST_LIMIT);
        if limit == 0 || limit > MAXIMUM_USER_FILE_LIST_LIMIT {
            return Err(ApplicationError::Invalid(format!(
                "UserFile list limit must be between 1 and {MAXIMUM_USER_FILE_LIST_LIMIT}"
            )));
        }
        self.files
            .list(request.organization_id, request.project_id, limit)
            .await
            .map_err(Into::into)
    }

    pub async fn quota(&self, request: GetUserFileQuota) -> ApplicationResult<UserFileQuota> {
        organization_quota(&request.resource_access)?;
        self.files
            .quota(request.organization_id)
            .await
            .map_err(Into::into)
    }

    async fn transition_replay(
        &self,
        request: &UserFileTransition,
        idempotency: &IdempotencyRequest,
        expected_state: UserFileState,
    ) -> ApplicationResult<Option<UserFileMutationResult>> {
        let Some(replay) = self.files.replay_write(idempotency).await? else {
            return Ok(None);
        };
        if replay.value.organization_id != request.organization_id
            || replay.value.project_id != request.project_id
            || replay.value.id != request.user_file_id
            || replay.value.aggregate_version != request.expected_version.saturating_add(1)
            || replay.value.state != expected_state
        {
            return Err(ApplicationError::Internal(
                "UserFile lifecycle replay is inconsistent".into(),
            ));
        }
        Ok(Some(UserFileMutationResult {
            file: replay.value,
            replayed: true,
        }))
    }

    async fn load_transition_file(
        &self,
        request: &UserFileTransition,
    ) -> ApplicationResult<UserFile> {
        let current = match self
            .files
            .find(
                request.organization_id,
                request.project_id,
                request.user_file_id,
            )
            .await
        {
            Ok(Some(file)) => file,
            Ok(None) | Err(RepositoryError::NotFound) => return Err(user_file_not_found()),
            Err(error) => return Err(error.into()),
        };
        if current.aggregate_version != request.expected_version {
            return Err(ApplicationError::Conflict(
                "UserFile was changed from a stale aggregate version".into(),
            ));
        }
        Ok(current)
    }

    async fn persist_transition(
        &self,
        request: UserFileTransition,
        idempotency: IdempotencyRequest,
        file: UserFile,
    ) -> ApplicationResult<UserFileMutationResult> {
        let event = UserFileLifecycleChanged::changed(&file, request.request_id, None)
            .map_err(ApplicationError::Internal)?;
        let result = self
            .files
            .transition(TransitionUserFileWrite {
                file,
                expected_version: request.expected_version,
                event,
                actor_principal_id: request.actor_principal_id,
                request_id: request.request_id,
                idempotency,
            })
            .await?;
        Ok(UserFileMutationResult {
            file: result.value,
            replayed: result.replayed,
        })
    }
}

pub struct ReserveUserFile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub admission_acl: String,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

pub struct UserFileTransition {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub user_file_id: UserFileId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

pub struct RecordUserFileUpload {
    pub transition: UserFileTransition,
    pub reader: UserFileObjectReader,
}

pub struct RecordUserFileScan {
    pub transition: UserFileTransition,
    pub evidence_digest: String,
    pub decision: UserFileScanDecision,
}

pub struct GetUserFile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub user_file_id: UserFileId,
    pub resource_access: ResourceAccessEvaluator,
}

pub struct ListUserFiles {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub limit: Option<usize>,
    pub resource_access: ResourceAccessEvaluator,
}

pub struct GetUserFileQuota {
    pub organization_id: OrganizationId,
    pub resource_access: ResourceAccessEvaluator,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalReserveUserFile<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor_principal_id: PrincipalId,
    contract_digest: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalTransition {
    organization_id: OrganizationId,
    project_id: ProjectId,
    user_file_id: UserFileId,
    expected_version: u64,
    actor_principal_id: PrincipalId,
    evidence: Option<serde_json::Value>,
}

fn idempotency(
    scope: String,
    key: String,
    canonical: &impl Serialize,
) -> ApplicationResult<IdempotencyRequest> {
    let bytes = serde_json::to_vec(canonical)
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
    IdempotencyRequest::new(scope, key, &bytes).map_err(ApplicationError::Invalid)
}

fn transition_idempotency(
    request: &UserFileTransition,
    action: &str,
    canonical: &impl Serialize,
) -> ApplicationResult<IdempotencyRequest> {
    idempotency(
        format!(
            "organizations/{}/projects/{}/user-files/{}/{action}",
            request.organization_id, request.project_id, request.user_file_id
        ),
        request.idempotency_key.clone(),
        canonical,
    )
}

fn object_error(error: UserFileObjectError) -> ApplicationError {
    match error {
        UserFileObjectError::Invalid(message) => ApplicationError::Invalid(message),
        UserFileObjectError::Conflict(message) => ApplicationError::Conflict(message),
        UserFileObjectError::NotFound => {
            ApplicationError::Unavailable("UserFile immutable object was not found".into())
        }
        UserFileObjectError::Integrity(message) => ApplicationError::Unavailable(message),
        UserFileObjectError::Unavailable(message) => ApplicationError::Unavailable(message),
    }
}
