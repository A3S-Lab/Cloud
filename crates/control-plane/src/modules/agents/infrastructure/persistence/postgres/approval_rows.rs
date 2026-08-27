use super::schema::AgentApprovalCheckpoints;
use crate::modules::agents::domain::{AgentApprovalCheckpoint, AgentApprovalCheckpointStatus};
use crate::modules::shared_kernel::domain::{
    AgentApprovalCheckpointId, AgentApprovalDecisionId, AgentConversationId, AgentExecutionId,
    AuthorizationDecisionRef, EnvironmentId, NodeCommandId, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, Sha256Digest,
};
use a3s_cloud_contracts::{
    AgentProviderApprovalOutcomeV1, AgentProviderToolPayloadIdentityV1, HarnessToolBindingV1,
};
use a3s_orm::expression::Selection;
use a3s_orm::{DecodeError, Expression, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) struct ApprovalCheckpointSelection;

impl Selection for ApprovalCheckpointSelection {
    type Output = ApprovalCheckpointRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AgentApprovalCheckpoints::organization_id().expression(),
            AgentApprovalCheckpoints::project_id().expression(),
            AgentApprovalCheckpoints::environment_id().expression(),
            AgentApprovalCheckpoints::conversation_id().expression(),
            AgentApprovalCheckpoints::execution_id().expression(),
            AgentApprovalCheckpoints::id().expression(),
            AgentApprovalCheckpoints::provider_run_identity_digest().expression(),
            AgentApprovalCheckpoints::invocation_profile_digest().expression(),
            AgentApprovalCheckpoints::source_event_sequence().expression(),
            AgentApprovalCheckpoints::call_id().expression(),
            AgentApprovalCheckpoints::tool_name().expression(),
            AgentApprovalCheckpoints::tool_revision().expression(),
            AgentApprovalCheckpoints::tool_contract_digest().expression(),
            AgentApprovalCheckpoints::request_digest().expression(),
            AgentApprovalCheckpoints::request_size_bytes().expression(),
            AgentApprovalCheckpoints::request_media_type().expression(),
            AgentApprovalCheckpoints::status().expression(),
            AgentApprovalCheckpoints::decision_id().expression(),
            AgentApprovalCheckpoints::outcome().expression(),
            AgentApprovalCheckpoints::decided_by().expression(),
            AgentApprovalCheckpoints::authorization_decision_id().expression(),
            AgentApprovalCheckpoints::authorization_decision_digest().expression(),
            AgentApprovalCheckpoints::reason().expression(),
            AgentApprovalCheckpoints::decision_digest().expression(),
            AgentApprovalCheckpoints::resume_command_id().expression(),
            AgentApprovalCheckpoints::resume_command_digest().expression(),
            AgentApprovalCheckpoints::aggregate_version().expression(),
            AgentApprovalCheckpoints::requested_at().expression(),
            AgentApprovalCheckpoints::expires_at().expression(),
            AgentApprovalCheckpoints::updated_at().expression(),
            AgentApprovalCheckpoints::decided_at().expression(),
            AgentApprovalCheckpoints::resumed_at().expression(),
            AgentApprovalCheckpoints::cancelled_at().expression(),
        ]
    }
}

pub(super) struct ApprovalCheckpointRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    conversation_id: Uuid,
    execution_id: Uuid,
    id: Uuid,
    provider_run_identity_digest: String,
    invocation_profile_digest: String,
    source_event_sequence: u64,
    call_id: String,
    tool_name: String,
    tool_revision: String,
    tool_contract_digest: String,
    request_digest: String,
    request_size_bytes: u64,
    request_media_type: String,
    status: String,
    decision_id: Option<Uuid>,
    outcome: Option<String>,
    decided_by: Option<Uuid>,
    authorization_decision_id: Option<String>,
    authorization_decision_digest: Option<String>,
    reason: Option<String>,
    decision_digest: Option<String>,
    resume_command_id: Option<Uuid>,
    resume_command_digest: Option<String>,
    aggregate_version: u64,
    requested_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    decided_at: Option<DateTime<Utc>>,
    resumed_at: Option<DateTime<Utc>>,
    cancelled_at: Option<DateTime<Utc>>,
}

impl FromRow for ApprovalCheckpointRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            conversation_id: decode(row, 3)?,
            execution_id: decode(row, 4)?,
            id: decode(row, 5)?,
            provider_run_identity_digest: decode(row, 6)?,
            invocation_profile_digest: decode(row, 7)?,
            source_event_sequence: decode(row, 8)?,
            call_id: decode(row, 9)?,
            tool_name: decode(row, 10)?,
            tool_revision: decode(row, 11)?,
            tool_contract_digest: decode(row, 12)?,
            request_digest: decode(row, 13)?,
            request_size_bytes: decode(row, 14)?,
            request_media_type: decode(row, 15)?,
            status: decode(row, 16)?,
            decision_id: decode(row, 17)?,
            outcome: decode(row, 18)?,
            decided_by: decode(row, 19)?,
            authorization_decision_id: decode(row, 20)?,
            authorization_decision_digest: decode(row, 21)?,
            reason: decode(row, 22)?,
            decision_digest: decode(row, 23)?,
            resume_command_id: decode(row, 24)?,
            resume_command_digest: decode(row, 25)?,
            aggregate_version: decode(row, 26)?,
            requested_at: decode(row, 27)?,
            expires_at: decode(row, 28)?,
            updated_at: decode(row, 29)?,
            decided_at: decode(row, 30)?,
            resumed_at: decode(row, 31)?,
            cancelled_at: decode(row, 32)?,
        })
    }
}

impl ApprovalCheckpointRow {
    pub(super) fn aggregate(self) -> Result<AgentApprovalCheckpoint, RepositoryError> {
        let authorization_decision = match (
            self.authorization_decision_id,
            self.authorization_decision_digest,
        ) {
            (None, None) => None,
            (Some(id), Some(digest)) => Some(
                AuthorizationDecisionRef::new(id, parse_digest(digest, "authorization")?)
                    .map_err(corrupt)?,
            ),
            _ => {
                return Err(corrupt(
                    "Agent approval authorization decision is incomplete",
                ))
            }
        };
        let checkpoint = AgentApprovalCheckpoint {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            conversation_id: AgentConversationId::from_uuid(self.conversation_id),
            execution_id: AgentExecutionId::from_uuid(self.execution_id),
            id: AgentApprovalCheckpointId::from_uuid(self.id),
            provider_run_identity_digest: parse_digest(
                self.provider_run_identity_digest,
                "provider run identity",
            )?,
            invocation_profile_digest: parse_digest(
                self.invocation_profile_digest,
                "invocation profile",
            )?,
            source_event_sequence: self.source_event_sequence,
            call_id: self.call_id,
            tool: HarnessToolBindingV1 {
                name: self.tool_name,
                revision: self.tool_revision,
                contract_digest: self.tool_contract_digest,
                approval_required: true,
            },
            request: AgentProviderToolPayloadIdentityV1 {
                digest: self.request_digest,
                size_bytes: self.request_size_bytes,
                media_type: self.request_media_type,
            },
            status: AgentApprovalCheckpointStatus::parse(&self.status).map_err(corrupt)?,
            decision_id: self.decision_id.map(AgentApprovalDecisionId::from_uuid),
            outcome: self
                .outcome
                .as_deref()
                .map(AgentProviderApprovalOutcomeV1::parse)
                .transpose()
                .map_err(corrupt)?,
            decided_by: self.decided_by.map(PrincipalId::from_uuid),
            authorization_decision,
            reason: self.reason,
            decision_digest: self
                .decision_digest
                .map(|value| parse_digest(value, "decision"))
                .transpose()?,
            resume_command_id: self.resume_command_id.map(NodeCommandId::from_uuid),
            resume_command_digest: self
                .resume_command_digest
                .map(|value| parse_digest(value, "resume command"))
                .transpose()?,
            aggregate_version: self.aggregate_version,
            requested_at: self.requested_at,
            expires_at: self.expires_at,
            updated_at: self.updated_at,
            decided_at: self.decided_at,
            resumed_at: self.resumed_at,
            cancelled_at: self.cancelled_at,
        };
        checkpoint
            .validate()
            .map_err(|error| corrupt(format!("Agent approval checkpoint is invalid: {error}")))?;
        Ok(checkpoint)
    }
}

fn parse_digest(value: String, label: &str) -> Result<Sha256Digest, RepositoryError> {
    Sha256Digest::parse(value)
        .map_err(|error| corrupt(format!("Agent approval {label} digest is invalid: {error}")))
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn corrupt(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(format!("stored data is corrupt: {}", message.into()))
}
