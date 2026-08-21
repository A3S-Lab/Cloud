use crate::infrastructure::{transaction_error, PostgresPersistenceError};
use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AdvanceConversationVariablesWrite,
    AppendApplicationMessageWrite, ApplicationEndUser, ApplicationInvocation,
    ApplicationInvocationWorkflowAuthority, ApplicationMessage, ApplicationSession,
    CloseApplicationSessionWrite, ConversationVariableRevision, IApplicationSessionRepository,
    OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationSessionId,
    ConversationVariableRevisionId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::PostgresExecutor;
use async_trait::async_trait;

use super::session_postgres_loads::{
    has_final_output, load_invocation_for_run, load_message, load_variable, load_variable_head,
    lock_invocation, lock_session,
};
use super::session_postgres_open as open;
use super::session_postgres_reads as reads;
use super::session_postgres_support::{ensure_unclaimed_effect, write_error};
use super::session_postgres_writes::{
    insert_effect_claim, insert_message, insert_variable_revision, message_effect_kind,
    update_invocation, update_session,
};

#[derive(Clone)]
pub struct PostgresApplicationSessionRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationSessionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationSessionRepository for PostgresApplicationSessionRepository {
    async fn open_session(
        &self,
        write: OpenApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
        open::open_session(&self.executor, write).await
    }

    async fn request_invocation(
        &self,
        write: RequestApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        open::request_invocation(&self.executor, write).await
    }

    async fn advance_invocation(
        &self,
        write: AdvanceApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let current = lock_invocation(
                        transaction,
                        write.invocation.organization_id,
                        write.invocation.project_id,
                        write.invocation.application_id,
                        write.invocation.id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if current == write.invocation
                        && current.aggregate_version == write.expected_version.saturating_add(1)
                    {
                        return Ok(IdempotentWrite {
                            value: current,
                            replayed: true,
                        });
                    }
                    write
                        .validate_against(&current)
                        .map_err(RepositoryError::Conflict)?;
                    update_invocation(transaction, &write.invocation, write.expected_version)
                        .await
                        .map_err(|error| {
                            write_error(
                                error,
                                "WorkflowRun is already bound to another Application invocation",
                            )
                        })?;
                    Ok(IdempotentWrite {
                        value: write.invocation,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn append_message(
        &self,
        write: AppendApplicationMessageWrite,
    ) -> Result<IdempotentWrite<ApplicationMessage>, RepositoryError> {
        write.message.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let session = lock_session(
                        transaction,
                        write.message.organization_id,
                        write.message.project_id,
                        write.message.application_id,
                        write.message.session_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if let Some(current) = load_message(transaction, &write.message).await? {
                        if current != write.message {
                            return Err(RepositoryError::Conflict(
                                "Application message effect replay changed content".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: current,
                            replayed: true,
                        });
                    }
                    let effect = write.message.workflow_effect.as_ref().ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "Application Workflow message effect is missing".into(),
                        )
                    })?;
                    ensure_unclaimed_effect(
                        transaction,
                        write.message.organization_id,
                        write.message.application_id,
                        write.message.session_id,
                        effect,
                    )
                    .await?;
                    let invocation = lock_invocation(
                        transaction,
                        write.message.organization_id,
                        write.message.project_id,
                        write.message.application_id,
                        write.message.invocation_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if has_final_output(transaction, &write.message).await? {
                        return Err(RepositoryError::Conflict(
                            "Application invocation already has its final output".into(),
                        )
                        .into());
                    }
                    write
                        .validate_against(&session, &invocation)
                        .map_err(RepositoryError::Conflict)?;
                    let next_session = session
                        .append_message(write.expected_session_version, &write.message)
                        .map_err(RepositoryError::Conflict)?;
                    insert_message(transaction, &write.message)
                        .await
                        .map_err(|error| {
                            write_error(
                                error,
                                "Application message identity or sequence is already in use",
                            )
                        })?;
                    insert_effect_claim(
                        transaction,
                        write.message.organization_id.as_uuid(),
                        write.message.project_id.as_uuid(),
                        write.message.application_id.as_uuid(),
                        write.message.session_id.as_uuid(),
                        effect,
                        message_effect_kind(write.message.kind),
                        write.message.id.as_uuid(),
                    )
                    .await
                    .map_err(|error| {
                        write_error(
                            error,
                            "Workflow effect is already owned by another Application semantic write",
                        )
                    })?;
                    update_session(transaction, &next_session, write.expected_session_version)
                        .await?;
                    Ok(IdempotentWrite {
                        value: write.message,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn advance_variables(
        &self,
        write: AdvanceConversationVariablesWrite,
    ) -> Result<IdempotentWrite<ConversationVariableRevision>, RepositoryError> {
        write
            .revision
            .validate()
            .map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let session = lock_session(
                        transaction,
                        write.revision.organization_id,
                        write.revision.project_id,
                        write.revision.application_id,
                        write.revision.session_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if let Some(current) = load_variable(transaction, &write.revision).await? {
                        if current != write.revision {
                            return Err(RepositoryError::Conflict(
                                "Conversation variable effect replay changed values".into(),
                            )
                            .into());
                        }
                        return Ok(IdempotentWrite {
                            value: current,
                            replayed: true,
                        });
                    }
                    let effect = write.revision.source_effect.as_ref().ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "Conversation variable Workflow effect is missing".into(),
                        )
                    })?;
                    ensure_unclaimed_effect(
                        transaction,
                        write.revision.organization_id,
                        write.revision.application_id,
                        write.revision.session_id,
                        effect,
                    )
                    .await?;
                    let parent = load_variable_head(transaction, &session)
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Application session variable head is missing".into(),
                            )
                        })?;
                    load_invocation_for_run(transaction, &write.revision, effect.workflow_run_id)
                        .await?
                        .ok_or_else(|| {
                            RepositoryError::Conflict(
                                "Conversation variable effect is not bound to this Application session"
                                    .into(),
                            )
                        })?;
                    write
                        .validate_against(&session, &parent)
                        .map_err(RepositoryError::Conflict)?;
                    let next_session = session
                        .advance_variables(write.expected_session_version, &write.revision)
                        .map_err(RepositoryError::Conflict)?;
                    insert_variable_revision(transaction, &write.revision)
                        .await
                        .map_err(|error| {
                            write_error(
                                error,
                                "Conversation variable revision identity is already in use",
                            )
                        })?;
                    insert_effect_claim(
                        transaction,
                        write.revision.organization_id.as_uuid(),
                        write.revision.project_id.as_uuid(),
                        write.revision.application_id.as_uuid(),
                        write.revision.session_id.as_uuid(),
                        effect,
                        "conversation_variables",
                        write.revision.id.as_uuid(),
                    )
                    .await
                    .map_err(|error| {
                        write_error(
                            error,
                            "Workflow effect is already owned by another Application semantic write",
                        )
                    })?;
                    update_session(transaction, &next_session, write.expected_session_version)
                        .await?;
                    Ok(IdempotentWrite {
                        value: write.revision,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn close_session(
        &self,
        write: CloseApplicationSessionWrite,
    ) -> Result<ApplicationSession, RepositoryError> {
        write.session.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let current = lock_session(
                        transaction,
                        write.session.organization_id,
                        write.session.project_id,
                        write.session.application_id,
                        write.session.id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if current == write.session
                        && current.aggregate_version == write.expected_version.saturating_add(1)
                    {
                        return Ok(current);
                    }
                    write
                        .validate_against(&current)
                        .map_err(RepositoryError::Conflict)?;
                    update_session(transaction, &write.session, write.expected_version).await?;
                    Ok(write.session)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_end_user(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        end_user_id: ApplicationEndUserId,
    ) -> Result<Option<ApplicationEndUser>, RepositoryError> {
        reads::find_end_user(
            &self.executor,
            organization_id,
            project_id,
            application_id,
            end_user_id,
        )
        .await
    }

    async fn find_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Option<ApplicationSession>, RepositoryError> {
        reads::find_session(
            &self.executor,
            organization_id,
            project_id,
            application_id,
            session_id,
        )
        .await
    }

    async fn find_invocation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocation>, RepositoryError> {
        reads::find_invocation(
            &self.executor,
            organization_id,
            project_id,
            application_id,
            invocation_id,
        )
        .await
    }

    async fn find_invocation_workflow_authority(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocationWorkflowAuthority>, RepositoryError> {
        reads::find_invocation_workflow_authority(
            &self.executor,
            organization_id,
            project_id,
            application_id,
            invocation_id,
        )
        .await
    }

    async fn list_messages(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<ApplicationMessage>, RepositoryError> {
        reads::list_messages(
            &self.executor,
            organization_id,
            project_id,
            application_id,
            session_id,
            after_sequence,
            limit,
        )
        .await
    }

    async fn find_variable_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        revision_id: ConversationVariableRevisionId,
    ) -> Result<Option<ConversationVariableRevision>, RepositoryError> {
        reads::find_variable_revision(
            &self.executor,
            organization_id,
            project_id,
            application_id,
            session_id,
            revision_id,
        )
        .await
    }
}
