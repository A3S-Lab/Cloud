//! Applications-owned blocking invocation observation.
//!
//! `APP0.2-C39` freezes the shared-cursor blocking wait projection: one
//! Blocking-mode invocation is observed against its session-owned Input,
//! Answer, and FinalOutput messages until the invocation is terminal. This is
//! not a second run history, timer worker, SSE stream, or Gateway route.
//! Persistence, CQRS polling, streaming parity, REST/OpenAPI/client/CLI/MCP,
//! and public availability stay later numbered gates.

use super::{
    ApplicationInvocation, ApplicationInvocationStatus, ApplicationMessage, ApplicationMessageKind,
    ApplicationResponseMode, ApplicationSession,
};
use crate::modules::shared_kernel::domain::{
    ApplicationEndUserId, ApplicationId, ApplicationInvocationId, ApplicationMessageId,
    ApplicationReleaseId, ApplicationSessionId, OrganizationId, ProjectId, Sha256Digest,
    canonical_timestamp,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Blocking-wait readiness derived from invocation status and channel messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationBlockingWaitStatus {
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

impl ApplicationBlockingWaitStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Waiting)
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "waiting" => Ok(Self::Waiting),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!(
                "unsupported Application blocking wait status {value:?}"
            )),
        }
    }
}

/// One deterministic blocking-wait observation over an exact Blocking invocation.
///
/// Callers may poll existing Applications session/invocation state until
/// `wait_status` is terminal. Exact message identity lists never rewrite the
/// ordered `ApplicationMessage` sequence. Streaming and asynchronous modes fail
/// closed here; they belong to later shared-cursor protocol slices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationBlockingObservation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_id: ApplicationReleaseId,
    pub application_release_digest: Sha256Digest,
    pub session_id: ApplicationSessionId,
    pub end_user_id: ApplicationEndUserId,
    pub invocation_id: ApplicationInvocationId,
    pub response_mode: ApplicationResponseMode,
    pub invocation_status: ApplicationInvocationStatus,
    pub wait_status: ApplicationBlockingWaitStatus,
    pub input_message_id: Option<ApplicationMessageId>,
    pub answer_message_ids: Vec<ApplicationMessageId>,
    pub final_output_message_id: Option<ApplicationMessageId>,
    pub observed_at: DateTime<Utc>,
}

impl ApplicationBlockingObservation {
    pub fn observe(
        session: &ApplicationSession,
        invocation: &ApplicationInvocation,
        messages: &[ApplicationMessage],
        observed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate()?;
        invocation.validate()?;
        validate_session_invocation(session, invocation)?;
        if invocation.response_mode != ApplicationResponseMode::Blocking {
            return Err(
                "Application blocking observation requires Blocking response mode".into(),
            );
        }
        let observed_at = canonical_timestamp(observed_at);
        if observed_at < invocation.requested_at {
            return Err("Application blocking observation cannot predate its invocation".into());
        }

        let frames = collect_invocation_frames(session, invocation, messages)?;
        let (input_message_id, answer_message_ids, final_output_message_id) =
            partition_frames(&frames)?;
        let wait_status = wait_status_for(invocation.status, final_output_message_id.is_some())?;

        let value = Self {
            organization_id: session.organization_id,
            project_id: session.project_id,
            application_id: session.application_id,
            application_release_id: session.application_release_id,
            application_release_digest: session.application_release_digest.clone(),
            session_id: session.id,
            end_user_id: session.end_user_id,
            invocation_id: invocation.id,
            response_mode: invocation.response_mode,
            invocation_status: invocation.status,
            wait_status,
            input_message_id,
            answer_message_ids,
            final_output_message_id,
            observed_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_id.as_uuid().is_nil()
            || self.session_id.as_uuid().is_nil()
            || self.end_user_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || Sha256Digest::parse(self.application_release_digest.as_str())?
                != self.application_release_digest
        {
            return Err("Application blocking observation identities cannot be nil".into());
        }
        if self.response_mode != ApplicationResponseMode::Blocking {
            return Err(
                "Application blocking observation requires Blocking response mode".into(),
            );
        }
        if self
            .answer_message_ids
            .iter()
            .any(|message_id| message_id.as_uuid().is_nil())
            || self
                .input_message_id
                .is_some_and(|message_id| message_id.as_uuid().is_nil())
            || self
                .final_output_message_id
                .is_some_and(|message_id| message_id.as_uuid().is_nil())
        {
            return Err("Application blocking observation message identities cannot be nil".into());
        }
        match self.wait_status {
            ApplicationBlockingWaitStatus::Waiting => {
                if self.invocation_status.is_terminal() {
                    return Err(
                        "Application blocking observation waiting status cannot be terminal"
                            .into(),
                    );
                }
                if self.final_output_message_id.is_some() {
                    return Err(
                        "Application blocking observation cannot expose FinalOutput while waiting"
                            .into(),
                    );
                }
            }
            ApplicationBlockingWaitStatus::Succeeded => {
                if self.invocation_status != ApplicationInvocationStatus::Succeeded
                    || self.final_output_message_id.is_none()
                    || self.input_message_id.is_none()
                {
                    return Err(
                        "Application blocking observation succeeded status requires Succeeded invocation with Input and FinalOutput"
                            .into(),
                    );
                }
            }
            ApplicationBlockingWaitStatus::Failed => {
                if self.invocation_status != ApplicationInvocationStatus::Failed
                    || self.final_output_message_id.is_some()
                {
                    return Err(
                        "Application blocking observation failed status requires Failed invocation without FinalOutput"
                            .into(),
                    );
                }
            }
            ApplicationBlockingWaitStatus::Cancelled => {
                if self.invocation_status != ApplicationInvocationStatus::Cancelled
                    || self.final_output_message_id.is_some()
                {
                    return Err(
                        "Application blocking observation cancelled status requires Cancelled invocation without FinalOutput"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
}

fn validate_session_invocation(
    session: &ApplicationSession,
    invocation: &ApplicationInvocation,
) -> Result<(), String> {
    if invocation.organization_id != session.organization_id
        || invocation.project_id != session.project_id
        || invocation.application_id != session.application_id
        || invocation.application_release_id != session.application_release_id
        || invocation.application_release_digest != session.application_release_digest
        || invocation.session_id != session.id
    {
        return Err("Application blocking observation invocation is outside the exact session".into());
    }
    Ok(())
}

fn collect_invocation_frames(
    session: &ApplicationSession,
    invocation: &ApplicationInvocation,
    messages: &[ApplicationMessage],
) -> Result<Vec<ApplicationMessage>, String> {
    let mut frames = Vec::with_capacity(messages.len());
    for message in messages {
        message.validate()?;
        if message.session_id != session.id
            || message.organization_id != session.organization_id
            || message.project_id != session.project_id
            || message.application_id != session.application_id
            || message.application_release_id != session.application_release_id
            || message.application_release_digest != session.application_release_digest
        {
            return Err(
                "Application blocking observation message is outside the exact session".into(),
            );
        }
        if message.invocation_id != invocation.id {
            return Err(
                "Application blocking observation message belongs to a foreign invocation".into(),
            );
        }
        if message.sequence == 0 || message.sequence > session.last_message_sequence {
            return Err(
                "Application blocking observation message sequence drifts from the session head"
                    .into(),
            );
        }
        frames.push(message.clone());
    }
    frames.sort_by_key(|message| message.sequence);
    for window in frames.windows(2) {
        if window[0].sequence == window[1].sequence {
            return Err(
                "Application blocking observation rejects duplicate message sequences".into(),
            );
        }
        if window[0].id == window[1].id {
            return Err(
                "Application blocking observation rejects duplicate message identities".into(),
            );
        }
    }
    Ok(frames)
}

fn partition_frames(
    frames: &[ApplicationMessage],
) -> Result<
    (
        Option<ApplicationMessageId>,
        Vec<ApplicationMessageId>,
        Option<ApplicationMessageId>,
    ),
    String,
> {
    let mut input_message_id = None;
    let mut answer_message_ids = Vec::new();
    let mut final_output_message_id = None;
    let mut saw_answer_or_final = false;

    for message in frames {
        match message.kind {
            ApplicationMessageKind::Input => {
                if saw_answer_or_final {
                    return Err(
                        "Application blocking observation Input cannot follow Answer or FinalOutput"
                            .into(),
                    );
                }
                if input_message_id.is_some() {
                    return Err(
                        "Application blocking observation admits at most one Input message".into(),
                    );
                }
                input_message_id = Some(message.id);
            }
            ApplicationMessageKind::Answer => {
                if final_output_message_id.is_some() {
                    return Err(
                        "Application blocking observation Answer cannot follow FinalOutput".into(),
                    );
                }
                saw_answer_or_final = true;
                answer_message_ids.push(message.id);
            }
            ApplicationMessageKind::FinalOutput => {
                if final_output_message_id.is_some() {
                    return Err(
                        "Application blocking observation admits at most one FinalOutput message"
                            .into(),
                    );
                }
                saw_answer_or_final = true;
                final_output_message_id = Some(message.id);
            }
        }
    }

    Ok((input_message_id, answer_message_ids, final_output_message_id))
}

fn wait_status_for(
    invocation_status: ApplicationInvocationStatus,
    has_final_output: bool,
) -> Result<ApplicationBlockingWaitStatus, String> {
    match invocation_status {
        ApplicationInvocationStatus::Requested
        | ApplicationInvocationStatus::Running
        | ApplicationInvocationStatus::Cancelling => {
            if has_final_output {
                return Err(
                    "Application blocking observation cannot expose FinalOutput before terminal invocation"
                        .into(),
                );
            }
            Ok(ApplicationBlockingWaitStatus::Waiting)
        }
        ApplicationInvocationStatus::Succeeded => {
            if !has_final_output {
                return Err(
                    "Application blocking observation Succeeded invocation requires FinalOutput"
                        .into(),
                );
            }
            Ok(ApplicationBlockingWaitStatus::Succeeded)
        }
        ApplicationInvocationStatus::Failed => {
            if has_final_output {
                return Err(
                    "Application blocking observation Failed invocation cannot retain FinalOutput"
                        .into(),
                );
            }
            Ok(ApplicationBlockingWaitStatus::Failed)
        }
        ApplicationInvocationStatus::Cancelled => {
            if has_final_output {
                return Err(
                    "Application blocking observation Cancelled invocation cannot retain FinalOutput"
                        .into(),
                );
            }
            Ok(ApplicationBlockingWaitStatus::Cancelled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::applications::domain::{
        ApplicationAudience, ApplicationDeliveryPolicy, ApplicationEndUser, ApplicationExperience,
        ApplicationInteractionMode, ApplicationRelease, ApplicationReleaseContract,
        ApplicationReleaseContractSpec, ApplicationWorkflowBinding, ApplicationWorkflowEffect,
        ConversationVariableRevision,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
    };
    use chrono::{Duration, TimeZone};
    use serde_json::json;

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", format!("{:02x}", marker as u8).repeat(32)))
            .expect("digest")
    }

    fn release_with_modes(modes: Vec<ApplicationResponseMode>) -> ApplicationRelease {
        let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
            experience: ApplicationExperience::Chatflow,
            audience: ApplicationAudience::ProjectMembers,
            delivery: ApplicationDeliveryPolicy {
                interaction_mode: ApplicationInteractionMode::Conversation,
                response_modes: modes,
            },
            workflow: ApplicationWorkflowBinding {
                workflow_definition_id: WorkflowDefinitionId::new(),
                workflow_revision_id: WorkflowRevisionId::new(),
                workflow_contract_digest: digest('a'),
                workflow_payload_set_digest: digest('b'),
                workflow_semantic_contract_set_digest: digest('c'),
                input_schema_digest: digest('d'),
                output_schema_digest: digest('e'),
            },
            presentation_digest: digest('f'),
        })
        .expect("contract");
        ApplicationRelease::initial(
            OrganizationId::new(),
            ProjectId::new(),
            ApplicationId::new(),
            ApplicationReleaseId::new(),
            contract,
            PrincipalId::new(),
            Utc.with_ymd_and_hms(2026, 9, 14, 19, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("release")
    }

    fn active_session(release: &ApplicationRelease) -> ApplicationSession {
        let end_user = ApplicationEndUser::create(
            ApplicationEndUserId::new(),
            release,
            Some(PrincipalId::new()),
            release.created_by,
            release.created_at,
        )
        .expect("end user");
        let session_id = ApplicationSessionId::new();
        let variables = ConversationVariableRevision::initial(
            session_id,
            release,
            json!({"locale": "en-US"}),
            release.created_at,
        )
        .expect("variables");
        ApplicationSession::create(
            session_id,
            release,
            &end_user,
            &variables,
            release.created_at,
        )
        .expect("session")
    }

    fn requested_invocation(
        session: &ApplicationSession,
        release: &ApplicationRelease,
        mode: ApplicationResponseMode,
    ) -> ApplicationInvocation {
        ApplicationInvocation::request(
            ApplicationInvocationId::new(),
            session,
            release,
            mode,
            json!({"query": "hello"}),
            release.created_at,
        )
        .expect("invocation")
    }

    fn bound_running(
        session: &ApplicationSession,
        release: &ApplicationRelease,
    ) -> (ApplicationSession, ApplicationInvocation, ApplicationMessage) {
        let invocation = requested_invocation(session, release, ApplicationResponseMode::Blocking);
        let input = ApplicationMessage::input(session, &invocation, release.created_at)
            .expect("input");
        let session = session.append_message(1, &input).expect("append input");
        let run_id = WorkflowRunId::new();
        let invocation = invocation
            .bind_workflow_run(1, run_id, release.created_at + Duration::seconds(1))
            .expect("bind");
        (session, invocation, input)
    }

    #[test]
    fn waiting_observation_exposes_input_and_partial_answers() {
        let release = release_with_modes(vec![ApplicationResponseMode::Blocking]);
        let session = active_session(&release);
        let (session, invocation, input) = bound_running(&session, &release);
        let answer = ApplicationMessage::workflow_frame(
            &session,
            &invocation,
            ApplicationMessageKind::Answer,
            ApplicationWorkflowEffect::new(
                invocation.workflow_run_id.expect("run"),
                "answer",
                1,
                0,
            )
            .expect("effect"),
            json!({"text": "partial"}),
            release.created_at + Duration::seconds(2),
        )
        .expect("answer");
        let session = session.append_message(2, &answer).expect("append answer");

        let observation = ApplicationBlockingObservation::observe(
            &session,
            &invocation,
            &[input.clone(), answer.clone()],
            release.created_at + Duration::seconds(3),
        )
        .expect("waiting");
        assert_eq!(observation.wait_status, ApplicationBlockingWaitStatus::Waiting);
        assert!(!observation.wait_status.is_terminal());
        assert_eq!(observation.input_message_id, Some(input.id));
        assert_eq!(observation.answer_message_ids, vec![answer.id]);
        assert_eq!(observation.final_output_message_id, None);
        observation.validate().expect("validate");
    }

    #[test]
    fn succeeded_observation_requires_final_output_and_orders_answers() {
        let release = release_with_modes(vec![
            ApplicationResponseMode::Blocking,
            ApplicationResponseMode::Streaming,
        ]);
        let session = active_session(&release);
        let (session, invocation, input) = bound_running(&session, &release);
        let run_id = invocation.workflow_run_id.expect("run");
        let first = ApplicationMessage::workflow_frame(
            &session,
            &invocation,
            ApplicationMessageKind::Answer,
            ApplicationWorkflowEffect::new(run_id, "answer", 1, 0).expect("effect"),
            json!({"text": "one"}),
            release.created_at + Duration::seconds(2),
        )
        .expect("first");
        let session = session.append_message(2, &first).expect("append first");
        let second = ApplicationMessage::workflow_frame(
            &session,
            &invocation,
            ApplicationMessageKind::Answer,
            ApplicationWorkflowEffect::new(run_id, "answer", 1, 1).expect("effect"),
            json!({"text": "two"}),
            release.created_at + Duration::seconds(3),
        )
        .expect("second");
        let session = session.append_message(3, &second).expect("append second");
        let final_output = ApplicationMessage::workflow_frame(
            &session,
            &invocation,
            ApplicationMessageKind::FinalOutput,
            ApplicationWorkflowEffect::new(run_id, "final", 1, 0).expect("effect"),
            json!({"result": "done"}),
            release.created_at + Duration::seconds(4),
        )
        .expect("final");
        let session = session
            .append_message(4, &final_output)
            .expect("append final");
        let invocation = invocation
            .observe_terminal(
                2,
                ApplicationInvocationStatus::Succeeded,
                release.created_at + Duration::seconds(5),
            )
            .expect("terminal");

        let observation = ApplicationBlockingObservation::observe(
            &session,
            &invocation,
            &[input.clone(), second.clone(), first.clone(), final_output.clone()],
            release.created_at + Duration::seconds(6),
        )
        .expect("succeeded");
        assert_eq!(
            observation.wait_status,
            ApplicationBlockingWaitStatus::Succeeded
        );
        assert_eq!(observation.input_message_id, Some(input.id));
        assert_eq!(
            observation.answer_message_ids,
            vec![first.id, second.id]
        );
        assert_eq!(observation.final_output_message_id, Some(final_output.id));
    }

    #[test]
    fn failed_and_cancelled_observations_reject_final_output() {
        let release = release_with_modes(vec![ApplicationResponseMode::Blocking]);
        let session = active_session(&release);
        let (session, invocation, input) = bound_running(&session, &release);
        let failed = invocation
            .clone()
            .observe_terminal(
                2,
                ApplicationInvocationStatus::Failed,
                release.created_at + Duration::seconds(2),
            )
            .expect("failed");
        let failed_observation = ApplicationBlockingObservation::observe(
            &session,
            &failed,
            &[input.clone()],
            release.created_at + Duration::seconds(3),
        )
        .expect("failed observation");
        assert_eq!(
            failed_observation.wait_status,
            ApplicationBlockingWaitStatus::Failed
        );

        let cancelling = invocation
            .request_cancellation(2, release.created_at + Duration::seconds(2))
            .expect("cancelling");
        let cancelled = cancelling
            .observe_terminal(
                3,
                ApplicationInvocationStatus::Cancelled,
                release.created_at + Duration::seconds(3),
            )
            .expect("cancelled");
        let cancelled_observation = ApplicationBlockingObservation::observe(
            &session,
            &cancelled,
            &[input],
            release.created_at + Duration::seconds(4),
        )
        .expect("cancelled observation");
        assert_eq!(
            cancelled_observation.wait_status,
            ApplicationBlockingWaitStatus::Cancelled
        );
    }

    #[test]
    fn streaming_mode_and_foreign_messages_fail_closed() {
        let release = release_with_modes(vec![
            ApplicationResponseMode::Blocking,
            ApplicationResponseMode::Streaming,
        ]);
        let session = active_session(&release);
        let streaming = requested_invocation(&session, &release, ApplicationResponseMode::Streaming);
        assert!(
            ApplicationBlockingObservation::observe(
                &session,
                &streaming,
                &[],
                release.created_at + Duration::seconds(1),
            )
            .is_err()
        );

        let (session, invocation, input) = bound_running(&session, &release);
        let foreign_invocation =
            requested_invocation(&session, &release, ApplicationResponseMode::Blocking);
        let foreign_input =
            ApplicationMessage::input(&session, &foreign_invocation, release.created_at)
                .expect("foreign input");
        assert!(
            ApplicationBlockingObservation::observe(
                &session,
                &invocation,
                &[input, foreign_input],
                release.created_at + Duration::seconds(2),
            )
            .is_err()
        );
    }

    #[test]
    fn succeeded_without_final_output_and_answer_after_final_fail_closed() {
        let release = release_with_modes(vec![ApplicationResponseMode::Blocking]);
        let session = active_session(&release);
        let (session, invocation, input) = bound_running(&session, &release);
        let succeeded = invocation
            .clone()
            .observe_terminal(
                2,
                ApplicationInvocationStatus::Succeeded,
                release.created_at + Duration::seconds(2),
            )
            .expect("succeeded");
        assert!(
            ApplicationBlockingObservation::observe(
                &session,
                &succeeded,
                &[input.clone()],
                release.created_at + Duration::seconds(3),
            )
            .is_err()
        );

        let run_id = invocation.workflow_run_id.expect("run");
        let final_output = ApplicationMessage::workflow_frame(
            &session,
            &invocation,
            ApplicationMessageKind::FinalOutput,
            ApplicationWorkflowEffect::new(run_id, "final", 1, 0).expect("effect"),
            json!({"result": "done"}),
            release.created_at + Duration::seconds(2),
        )
        .expect("final");
        let session = session
            .append_message(2, &final_output)
            .expect("append final");
        let late_answer = ApplicationMessage::workflow_frame(
            &session,
            &invocation,
            ApplicationMessageKind::Answer,
            ApplicationWorkflowEffect::new(run_id, "answer", 1, 0).expect("effect"),
            json!({"text": "late"}),
            release.created_at + Duration::seconds(3),
        )
        .expect("late answer");
        let session = session
            .append_message(3, &late_answer)
            .expect("append late");
        let succeeded = invocation
            .observe_terminal(
                2,
                ApplicationInvocationStatus::Succeeded,
                release.created_at + Duration::seconds(4),
            )
            .expect("terminal");
        assert!(
            ApplicationBlockingObservation::observe(
                &session,
                &succeeded,
                &[input, final_output, late_answer],
                release.created_at + Duration::seconds(5),
            )
            .is_err()
        );
    }
}
