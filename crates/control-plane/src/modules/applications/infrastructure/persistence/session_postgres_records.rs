use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationEndUser, ApplicationInteractionMode, ApplicationInvocation,
    ApplicationInvocationStatus, ApplicationMessage, ApplicationMessageKind,
    ApplicationResponseMode, ApplicationSession, ApplicationSessionStatus,
    ApplicationWorkflowEffect, ConversationVariableRevision,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, ConversationVariableRevisionId, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, Sha256Digest, WorkflowRunId,
};
use a3s_orm::{sql_query, DecodeError, FromRow, FromValue, Row, SqlQuery};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const SELECT_END_USERS: &str = "select organization_id, project_id, application_id, id, audience, linked_principal_id, created_by, created_at from application_end_users";
const SELECT_SESSIONS: &str = "select organization_id, project_id, application_id, application_release_id, application_release_number, application_release_digest, end_user_id, id, interaction_mode, status, last_message_sequence, current_variable_revision_id, current_variable_revision_number, current_variable_digest, aggregate_version, created_at, updated_at, closed_at from application_sessions";
const SELECT_INVOCATIONS: &str = "select organization_id, project_id, application_id, application_release_id, application_release_digest, session_id, id, response_mode, input, input_digest, workflow_run_id, status, aggregate_version, requested_at, updated_at, completed_at from application_invocations";
const SELECT_MESSAGES: &str = "select organization_id, project_id, application_id, application_release_id, application_release_digest, session_id, invocation_id, id, sequence, kind, content, content_digest, workflow_run_id, workflow_step_id, workflow_attempt, workflow_effect_ordinal, created_at from application_messages";
const SELECT_VARIABLES: &str = "select organization_id, project_id, application_id, application_release_id, application_release_digest, session_id, id, revision_number, parent_revision_id, parent_digest, values_json, values_digest, workflow_run_id, workflow_step_id, workflow_attempt, workflow_effect_ordinal, created_at from application_conversation_variable_revisions";

pub(super) struct ApplicationEndUserRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    id: Uuid,
    audience: String,
    linked_principal_id: Option<Uuid>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for ApplicationEndUserRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            id: decode(row, 3)?,
            audience: decode(row, 4)?,
            linked_principal_id: decode(row, 5)?,
            created_by: decode(row, 6)?,
            created_at: decode(row, 7)?,
        })
    }
}

pub(super) struct ApplicationSessionRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_number: u64,
    application_release_digest: String,
    end_user_id: Uuid,
    id: Uuid,
    interaction_mode: String,
    status: String,
    last_message_sequence: u64,
    current_variable_revision_id: Uuid,
    current_variable_revision_number: u64,
    current_variable_digest: String,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

impl FromRow for ApplicationSessionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            application_release_id: decode(row, 3)?,
            application_release_number: decode(row, 4)?,
            application_release_digest: decode(row, 5)?,
            end_user_id: decode(row, 6)?,
            id: decode(row, 7)?,
            interaction_mode: decode(row, 8)?,
            status: decode(row, 9)?,
            last_message_sequence: decode(row, 10)?,
            current_variable_revision_id: decode(row, 11)?,
            current_variable_revision_number: decode(row, 12)?,
            current_variable_digest: decode(row, 13)?,
            aggregate_version: decode(row, 14)?,
            created_at: decode(row, 15)?,
            updated_at: decode(row, 16)?,
            closed_at: decode(row, 17)?,
        })
    }
}

pub(super) struct ApplicationInvocationRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    session_id: Uuid,
    id: Uuid,
    response_mode: String,
    input: Value,
    input_digest: String,
    workflow_run_id: Option<Uuid>,
    status: String,
    aggregate_version: u64,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl FromRow for ApplicationInvocationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            application_release_id: decode(row, 3)?,
            application_release_digest: decode(row, 4)?,
            session_id: decode(row, 5)?,
            id: decode(row, 6)?,
            response_mode: decode(row, 7)?,
            input: decode(row, 8)?,
            input_digest: decode(row, 9)?,
            workflow_run_id: decode(row, 10)?,
            status: decode(row, 11)?,
            aggregate_version: decode(row, 12)?,
            requested_at: decode(row, 13)?,
            updated_at: decode(row, 14)?,
            completed_at: decode(row, 15)?,
        })
    }
}

pub(super) struct ApplicationMessageRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    session_id: Uuid,
    invocation_id: Uuid,
    id: Uuid,
    sequence: u64,
    kind: String,
    content: Value,
    content_digest: String,
    workflow_run_id: Option<Uuid>,
    workflow_step_id: Option<String>,
    workflow_attempt: Option<u32>,
    workflow_effect_ordinal: Option<u32>,
    created_at: DateTime<Utc>,
}

impl FromRow for ApplicationMessageRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            application_release_id: decode(row, 3)?,
            application_release_digest: decode(row, 4)?,
            session_id: decode(row, 5)?,
            invocation_id: decode(row, 6)?,
            id: decode(row, 7)?,
            sequence: decode(row, 8)?,
            kind: decode(row, 9)?,
            content: decode(row, 10)?,
            content_digest: decode(row, 11)?,
            workflow_run_id: decode(row, 12)?,
            workflow_step_id: decode(row, 13)?,
            workflow_attempt: decode(row, 14)?,
            workflow_effect_ordinal: decode(row, 15)?,
            created_at: decode(row, 16)?,
        })
    }
}

pub(super) struct ConversationVariableRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    application_release_id: Uuid,
    application_release_digest: String,
    session_id: Uuid,
    id: Uuid,
    revision_number: u64,
    parent_revision_id: Option<Uuid>,
    parent_digest: Option<String>,
    values: Value,
    values_digest: String,
    workflow_run_id: Option<Uuid>,
    workflow_step_id: Option<String>,
    workflow_attempt: Option<u32>,
    workflow_effect_ordinal: Option<u32>,
    created_at: DateTime<Utc>,
}

impl FromRow for ConversationVariableRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            application_id: decode(row, 2)?,
            application_release_id: decode(row, 3)?,
            application_release_digest: decode(row, 4)?,
            session_id: decode(row, 5)?,
            id: decode(row, 6)?,
            revision_number: decode(row, 7)?,
            parent_revision_id: decode(row, 8)?,
            parent_digest: decode(row, 9)?,
            values: decode(row, 10)?,
            values_digest: decode(row, 11)?,
            workflow_run_id: decode(row, 12)?,
            workflow_step_id: decode(row, 13)?,
            workflow_attempt: decode(row, 14)?,
            workflow_effect_ordinal: decode(row, 15)?,
            created_at: decode(row, 16)?,
        })
    }
}

pub(super) fn end_user_select() -> SqlQuery<ApplicationEndUserRow> {
    sql_query(SELECT_END_USERS)
}

pub(super) fn session_select() -> SqlQuery<ApplicationSessionRow> {
    sql_query(SELECT_SESSIONS)
}

pub(super) fn invocation_select() -> SqlQuery<ApplicationInvocationRow> {
    sql_query(SELECT_INVOCATIONS)
}

pub(super) fn message_select() -> SqlQuery<ApplicationMessageRow> {
    sql_query(SELECT_MESSAGES)
}

pub(super) fn variable_select() -> SqlQuery<ConversationVariableRevisionRow> {
    sql_query(SELECT_VARIABLES)
}

pub(super) fn decode_end_user(
    row: ApplicationEndUserRow,
) -> Result<ApplicationEndUser, RepositoryError> {
    ApplicationEndUser {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        application_id: ApplicationId::from_uuid(row.application_id),
        id: ApplicationEndUserId::from_uuid(row.id),
        audience: ApplicationAudience::parse(&row.audience).map_err(stored("audience"))?,
        linked_principal_id: row.linked_principal_id.map(PrincipalId::from_uuid),
        created_by: PrincipalId::from_uuid(row.created_by),
        created_at: row.created_at,
    }
    .restore()
    .map_err(stored("end user"))
}

pub(super) fn decode_session(
    row: ApplicationSessionRow,
) -> Result<ApplicationSession, RepositoryError> {
    ApplicationSession {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        application_id: ApplicationId::from_uuid(row.application_id),
        application_release_id: ApplicationReleaseId::from_uuid(row.application_release_id),
        application_release_number: row.application_release_number,
        application_release_digest: digest(row.application_release_digest, "release digest")?,
        end_user_id: ApplicationEndUserId::from_uuid(row.end_user_id),
        id: ApplicationSessionId::from_uuid(row.id),
        interaction_mode: ApplicationInteractionMode::parse(&row.interaction_mode)
            .map_err(stored("interaction mode"))?,
        status: ApplicationSessionStatus::parse(&row.status).map_err(stored("status"))?,
        last_message_sequence: row.last_message_sequence,
        current_variable_revision_id: ConversationVariableRevisionId::from_uuid(
            row.current_variable_revision_id,
        ),
        current_variable_revision_number: row.current_variable_revision_number,
        current_variable_digest: digest(row.current_variable_digest, "variable digest")?,
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        closed_at: row.closed_at,
    }
    .restore()
    .map_err(stored("session"))
}

pub(super) fn decode_invocation(
    row: ApplicationInvocationRow,
) -> Result<ApplicationInvocation, RepositoryError> {
    ApplicationInvocation {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        application_id: ApplicationId::from_uuid(row.application_id),
        application_release_id: ApplicationReleaseId::from_uuid(row.application_release_id),
        application_release_digest: digest(row.application_release_digest, "release digest")?,
        session_id: ApplicationSessionId::from_uuid(row.session_id),
        id: ApplicationInvocationId::from_uuid(row.id),
        response_mode: ApplicationResponseMode::parse(&row.response_mode)
            .map_err(stored("response mode"))?,
        input: row.input,
        input_digest: digest(row.input_digest, "input digest")?,
        workflow_run_id: row.workflow_run_id.map(WorkflowRunId::from_uuid),
        status: ApplicationInvocationStatus::parse(&row.status).map_err(stored("status"))?,
        aggregate_version: row.aggregate_version,
        requested_at: row.requested_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
    }
    .restore()
    .map_err(stored("invocation"))
}

pub(super) fn decode_message(
    row: ApplicationMessageRow,
) -> Result<ApplicationMessage, RepositoryError> {
    ApplicationMessage {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        application_id: ApplicationId::from_uuid(row.application_id),
        application_release_id: ApplicationReleaseId::from_uuid(row.application_release_id),
        application_release_digest: digest(row.application_release_digest, "release digest")?,
        session_id: ApplicationSessionId::from_uuid(row.session_id),
        invocation_id: ApplicationInvocationId::from_uuid(row.invocation_id),
        id: ApplicationMessageId::from_uuid(row.id),
        sequence: row.sequence,
        kind: ApplicationMessageKind::parse(&row.kind).map_err(stored("kind"))?,
        content: row.content,
        content_digest: digest(row.content_digest, "content digest")?,
        workflow_effect: decode_effect(
            row.workflow_run_id,
            row.workflow_step_id,
            row.workflow_attempt,
            row.workflow_effect_ordinal,
        )?,
        created_at: row.created_at,
    }
    .restore()
    .map_err(stored("message"))
}

pub(super) fn decode_variable(
    row: ConversationVariableRevisionRow,
) -> Result<ConversationVariableRevision, RepositoryError> {
    ConversationVariableRevision {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        application_id: ApplicationId::from_uuid(row.application_id),
        application_release_id: ApplicationReleaseId::from_uuid(row.application_release_id),
        application_release_digest: digest(row.application_release_digest, "release digest")?,
        session_id: ApplicationSessionId::from_uuid(row.session_id),
        id: ConversationVariableRevisionId::from_uuid(row.id),
        revision_number: row.revision_number,
        parent_revision_id: row
            .parent_revision_id
            .map(ConversationVariableRevisionId::from_uuid),
        parent_digest: row
            .parent_digest
            .map(|value| digest(value, "parent digest"))
            .transpose()?,
        values: row.values,
        values_digest: digest(row.values_digest, "values digest")?,
        source_effect: decode_effect(
            row.workflow_run_id,
            row.workflow_step_id,
            row.workflow_attempt,
            row.workflow_effect_ordinal,
        )?,
        created_at: row.created_at,
    }
    .restore()
    .map_err(stored("conversation variable revision"))
}

fn decode_effect(
    workflow_run_id: Option<Uuid>,
    step_id: Option<String>,
    attempt: Option<u32>,
    ordinal: Option<u32>,
) -> Result<Option<ApplicationWorkflowEffect>, RepositoryError> {
    match (workflow_run_id, step_id, attempt, ordinal) {
        (None, None, None, None) => Ok(None),
        (Some(run_id), Some(step_id), Some(attempt), Some(ordinal)) => {
            ApplicationWorkflowEffect::new(
                WorkflowRunId::from_uuid(run_id),
                step_id,
                attempt,
                ordinal,
            )
            .map(Some)
            .map_err(stored("Workflow effect"))
        }
        _ => Err(RepositoryError::Storage(
            "stored Application Workflow effect is incomplete".into(),
        )),
    }
}

fn digest(value: String, label: &'static str) -> Result<Sha256Digest, RepositoryError> {
    Sha256Digest::parse(value).map_err(stored(label))
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored Application {label} is invalid: {error}"))
}
