use crate::modules::applications::domain::{
    AdvanceApplicationInvocationWrite, AdvanceConversationVariablesWrite,
    AppendApplicationMessageWrite, ApplicationEndUser, ApplicationInvocation,
    ApplicationInvocationWorkflowAuthority, ApplicationMessage, ApplicationMessageKind,
    ApplicationSession, CloseApplicationSessionWrite, ConversationVariableRevision,
    IApplicationSessionRepository, OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationSessionId, ConversationVariableRevisionId, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError, WorkflowRunId,
};
use async_trait::async_trait;
use tokio::sync::RwLock;

use super::session_in_memory_state::*;

#[derive(Default)]
pub struct InMemoryApplicationSessionRepository {
    state: RwLock<State>,
}

impl InMemoryApplicationSessionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IApplicationSessionRepository for InMemoryApplicationSessionRepository {
    async fn open_session(
        &self,
        write: OpenApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let session_key = session_key(&write.session);
        if let Some(current) = state.sessions.get(&session_key) {
            let variables = state
                .variables
                .get(&variable_key(&write.initial_variables))
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Application session replay variables are missing".into(),
                    )
                })?;
            let end_user = state
                .end_users
                .get(&end_user_key(&write.end_user))
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Application session replay end user is missing".into(),
                    )
                })?;
            if !write
                .matches_replay(current, end_user, variables)
                .map_err(RepositoryError::Storage)?
            {
                return Err(RepositoryError::Conflict(
                    "Application session identity was reused with different initial state".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: current.clone(),
                replayed: true,
            });
        }

        let end_user_key = end_user_key(&write.end_user);
        if state
            .end_users
            .get(&end_user_key)
            .is_some_and(|current| current != &write.end_user)
        {
            return Err(RepositoryError::Conflict(
                "Application end-user identity is already in use".into(),
            ));
        }
        let variable_key = variable_key(&write.initial_variables);
        if state.variables.contains_key(&variable_key) {
            return Err(RepositoryError::Conflict(
                "Conversation variable revision identity is already in use".into(),
            ));
        }

        state
            .end_users
            .entry(end_user_key)
            .or_insert(write.end_user);
        state
            .variables
            .insert(variable_key, write.initial_variables);
        state.sessions.insert(session_key, write.session.clone());
        Ok(IdempotentWrite {
            value: write.session,
            replayed: false,
        })
    }

    async fn request_invocation(
        &self,
        write: RequestApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let invocation_key = invocation_key(&write.invocation);
        if let Some(current) = state.invocations.get(&invocation_key) {
            let existing_authority = state
                .invocation_workflow_authorities
                .get(&invocation_key)
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Application invocation replay Workflow authority is missing".into(),
                    )
                })?;
            let existing_message = state
                .messages
                .get(&message_key(&write.input_message))
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "Application invocation replay input message is missing".into(),
                    )
                })?;
            if !write
                .matches_replay(current, existing_authority, existing_message)
                .map_err(RepositoryError::Storage)?
            {
                return Err(RepositoryError::Conflict(
                    "Application invocation identity was reused with different input".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: current.clone(),
                replayed: true,
            });
        }

        let session_key = session_key_for_invocation(&write.invocation);
        let current_session = state
            .sessions
            .get(&session_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        write
            .validate_against(&current_session)
            .map_err(RepositoryError::Conflict)?;
        ensure_message_identity_available(&state, &write.input_message)?;
        let next_session = current_session
            .append_message(write.expected_session_version, &write.input_message)
            .map_err(RepositoryError::Conflict)?;

        store_message(&mut state, write.input_message);
        state
            .invocations
            .insert(invocation_key, write.invocation.clone());
        state
            .invocation_workflow_authorities
            .insert(invocation_key, write.workflow_authority);
        state.sessions.insert(session_key, next_session);
        Ok(IdempotentWrite {
            value: write.invocation,
            replayed: false,
        })
    }

    async fn advance_invocation(
        &self,
        write: AdvanceApplicationInvocationWrite,
    ) -> Result<IdempotentWrite<ApplicationInvocation>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = invocation_key(&write.invocation);
        let current = state
            .invocations
            .get(&key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
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
        if let Some(workflow_run_id) = write.invocation.workflow_run_id {
            let run_key = (write.invocation.organization_id, workflow_run_id);
            if state
                .workflow_runs
                .get(&run_key)
                .is_some_and(|owner| owner != &key)
            {
                return Err(RepositoryError::Conflict(
                    "WorkflowRun is already bound to another Application invocation".into(),
                ));
            }
            state.workflow_runs.insert(run_key, key);
        }
        state.invocations.insert(key, write.invocation.clone());
        Ok(IdempotentWrite {
            value: write.invocation,
            replayed: false,
        })
    }

    async fn append_message(
        &self,
        write: AppendApplicationMessageWrite,
    ) -> Result<IdempotentWrite<ApplicationMessage>, RepositoryError> {
        write.message.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let message_key = message_key(&write.message);
        if let Some(current) = state.messages.get(&message_key) {
            if current != &write.message {
                return Err(RepositoryError::Conflict(
                    "Application message effect replay changed content".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: current.clone(),
                replayed: true,
            });
        }

        let effect = write.message.workflow_effect.as_ref().ok_or_else(|| {
            RepositoryError::Storage("Application Workflow message effect is missing".into())
        })?;
        let effect_key = EffectKey::new(
            write.message.organization_id,
            write.message.project_id,
            write.message.application_id,
            write.message.session_id,
            effect,
        );
        if state.effects.contains(&effect_key) {
            return Err(RepositoryError::Conflict(
                "Workflow effect is already owned by another Application semantic write".into(),
            ));
        }
        let invocation_key = invocation_key_for_message(&write.message);
        let invocation = state
            .invocations
            .get(&invocation_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        ensure_run_owner(&state, effect.workflow_run_id, &invocation_key)?;
        if state.final_outputs.contains_key(&invocation_key) {
            return Err(RepositoryError::Conflict(
                "Application invocation already has its final output".into(),
            ));
        }
        let session_key = session_key_for_message(&write.message);
        let session = state
            .sessions
            .get(&session_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        write
            .validate_against(&session, &invocation)
            .map_err(RepositoryError::Conflict)?;
        ensure_message_identity_available(&state, &write.message)?;
        let next_session = session
            .append_message(write.expected_session_version, &write.message)
            .map_err(RepositoryError::Conflict)?;

        if write.message.kind == ApplicationMessageKind::FinalOutput {
            state.final_outputs.insert(invocation_key, write.message.id);
        }
        state.effects.insert(effect_key);
        store_message(&mut state, write.message.clone());
        state.sessions.insert(session_key, next_session);
        Ok(IdempotentWrite {
            value: write.message,
            replayed: false,
        })
    }

    async fn advance_variables(
        &self,
        write: AdvanceConversationVariablesWrite,
    ) -> Result<IdempotentWrite<ConversationVariableRevision>, RepositoryError> {
        write
            .revision
            .validate()
            .map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let variable_key = variable_key(&write.revision);
        if let Some(current) = state.variables.get(&variable_key) {
            if current != &write.revision {
                return Err(RepositoryError::Conflict(
                    "Conversation variable effect replay changed values".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: current.clone(),
                replayed: true,
            });
        }
        let effect = write.revision.source_effect.as_ref().ok_or_else(|| {
            RepositoryError::Storage("Conversation variable Workflow effect is missing".into())
        })?;
        let effect_key = EffectKey::new(
            write.revision.organization_id,
            write.revision.project_id,
            write.revision.application_id,
            write.revision.session_id,
            effect,
        );
        if state.effects.contains(&effect_key) {
            return Err(RepositoryError::Conflict(
                "Workflow effect is already owned by another Application semantic write".into(),
            ));
        }
        let session_key = session_key_for_revision(&write.revision);
        let session = state
            .sessions
            .get(&session_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        let parent_key = (
            session.organization_id,
            session.project_id,
            session.application_id,
            session.id,
            session.current_variable_revision_id,
        );
        let parent = state
            .variables
            .get(&parent_key)
            .ok_or_else(|| {
                RepositoryError::Storage("Application session variable head is missing".into())
            })?
            .clone();
        let run_owner = state
            .workflow_runs
            .get(&(write.revision.organization_id, effect.workflow_run_id))
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "Conversation variable effect is not bound to an Application invocation".into(),
                )
            })?;
        let owner = state.invocations.get(run_owner).ok_or_else(|| {
            RepositoryError::Storage("Application WorkflowRun owner is missing".into())
        })?;
        if owner.session_id != write.revision.session_id {
            return Err(RepositoryError::Conflict(
                "Conversation variable effect belongs to another Application session".into(),
            ));
        }
        write
            .validate_against(&session, &parent)
            .map_err(RepositoryError::Conflict)?;
        let next_session = session
            .advance_variables(write.expected_session_version, &write.revision)
            .map_err(RepositoryError::Conflict)?;

        state.effects.insert(effect_key);
        state.variables.insert(variable_key, write.revision.clone());
        state.sessions.insert(session_key, next_session);
        Ok(IdempotentWrite {
            value: write.revision,
            replayed: false,
        })
    }

    async fn close_session(
        &self,
        write: CloseApplicationSessionWrite,
    ) -> Result<IdempotentWrite<ApplicationSession>, RepositoryError> {
        write.session.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = session_key(&write.session);
        let current = state
            .sessions
            .get(&key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        if current == write.session
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
        state.sessions.insert(key, write.session.clone());
        Ok(IdempotentWrite {
            value: write.session,
            replayed: false,
        })
    }

    async fn find_end_user(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        end_user_id: ApplicationEndUserId,
    ) -> Result<Option<ApplicationEndUser>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .end_users
            .get(&(organization_id, project_id, application_id, end_user_id))
            .cloned())
    }

    async fn find_session(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
    ) -> Result<Option<ApplicationSession>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .sessions
            .get(&(organization_id, project_id, application_id, session_id))
            .cloned())
    }

    async fn find_invocation(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocation>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .invocations
            .get(&(organization_id, project_id, application_id, invocation_id))
            .cloned())
    }

    async fn find_invocation_for_workflow_run(
        &self,
        organization_id: OrganizationId,
        workflow_run_id: WorkflowRunId,
    ) -> Result<Option<ApplicationInvocation>, RepositoryError> {
        let state = self.state.read().await;
        let Some(key) = state.workflow_runs.get(&(organization_id, workflow_run_id)) else {
            return Ok(None);
        };
        state
            .invocations
            .get(key)
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "Application WorkflowRun points to a missing invocation".into(),
                )
            })
    }

    async fn find_message(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        message_id: ApplicationMessageId,
    ) -> Result<Option<ApplicationMessage>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .messages
            .get(&(organization_id, project_id, application_id, message_id))
            .cloned())
    }

    async fn find_invocation_workflow_authority(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        invocation_id: ApplicationInvocationId,
    ) -> Result<Option<ApplicationInvocationWorkflowAuthority>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .invocation_workflow_authorities
            .get(&(organization_id, project_id, application_id, invocation_id))
            .cloned())
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
        if limit == 0 {
            return Ok(Vec::new());
        }
        let state = self.state.read().await;
        let mut messages = state
            .message_sequences
            .iter()
            .filter(
                |((organization, project, application, session, sequence), _)| {
                    *organization == organization_id
                        && *project == project_id
                        && *application == application_id
                        && *session == session_id
                        && *sequence > after_sequence
                },
            )
            .map(|(_, id)| {
                state
                    .messages
                    .get(&(organization_id, project_id, application_id, *id))
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "Application message sequence points to missing content".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        messages.sort_by_key(|message| message.sequence);
        messages.truncate(limit);
        Ok(messages)
    }

    async fn find_variable_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        session_id: ApplicationSessionId,
        revision_id: ConversationVariableRevisionId,
    ) -> Result<Option<ConversationVariableRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .variables
            .get(&(
                organization_id,
                project_id,
                application_id,
                session_id,
                revision_id,
            ))
            .cloned())
    }
}
