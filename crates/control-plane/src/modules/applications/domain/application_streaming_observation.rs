//! Applications-owned streaming invocation observation.
//!
//! `APP0.2-C42` freezes the shared-cursor streaming page projection: one
//! Streaming-mode invocation is observed against its session-owned Input,
//! Answer, and FinalOutput messages with cursor-based paging. This is not a
//! second run history, timer worker, SSE stream, or Gateway route. Persistence
//! is not required: observation projects existing session state. CQRS polling
//! is `APP0.2-C43`. Management delivery is `APP0.2-C44`. Gate `APP0.2-C42`.

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

/// Streaming observation readiness derived from invocation status and channel messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationStreamingObservationStatus {
    Open,
    Succeeded,
    Failed,
    Cancelled,
}

impl ApplicationStreamingObservationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Open)
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "open" => Ok(Self::Open),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!(
                "unsupported Application streaming observation status {value:?}"
            )),
        }
    }
}

/// One page frame in a streaming observation cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationStreamingObservationFrame {
    pub message_id: ApplicationMessageId,
    pub sequence: u64,
    pub kind: ApplicationMessageKind,
}

/// One deterministic streaming observation page over an exact Streaming invocation.
///
/// Callers may page existing Applications session/invocation state using
/// `after_sequence` until `stream_status` is terminal. Exact message identity
/// lists never rewrite the ordered `ApplicationMessage` sequence. Blocking and
/// asynchronous modes fail closed here; they belong to other protocol slices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationStreamingObservation {
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
    pub stream_status: ApplicationStreamingObservationStatus,
    pub after_sequence: u64,
    pub frames: Vec<ApplicationStreamingObservationFrame>,
    pub next_sequence: u64,
    pub has_more: bool,
    pub observed_at: DateTime<Utc>,
}

impl ApplicationStreamingObservation {
    pub fn observe(
        session: &ApplicationSession,
        invocation: &ApplicationInvocation,
        messages: &[ApplicationMessage],
        after_sequence: u64,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        session.validate()?;
        invocation.validate()?;
        validate_session_invocation(session, invocation)?;
        if invocation.response_mode != ApplicationResponseMode::Streaming {
            return Err(
                "Application streaming observation requires Streaming response mode".into(),
            );
        }
        let observed_at = canonical_timestamp(observed_at);
        if observed_at < invocation.requested_at {
            return Err("Application streaming observation cannot predate its invocation".into());
        }
        if after_sequence > session.last_message_sequence {
            return Err(
                "Application streaming observation cursor cannot exceed the session head".into(),
            );
        }

        let frames = collect_invocation_frames(session, invocation, messages)?;
        let (input_message_id, _answer_message_ids, final_output_message_id) =
            partition_frames(&frames)?;
        let stream_status = stream_status_for(
            invocation.status,
            input_message_id.is_some(),
            final_output_message_id.is_some(),
        )?;

        let page_messages: Vec<&ApplicationMessage> = frames
            .iter()
            .filter(|message| message.sequence > after_sequence)
            .collect();
        let observation_frames: Vec<ApplicationStreamingObservationFrame> = page_messages
            .iter()
            .map(|message| ApplicationStreamingObservationFrame {
                message_id: message.id,
                sequence: message.sequence,
                kind: message.kind,
            })
            .collect();
        let next_sequence = page_messages
            .last()
            .map(|message| message.sequence)
            .unwrap_or(after_sequence);
        let has_more = frames
            .iter()
            .any(|message| message.sequence > next_sequence);

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
            stream_status,
            after_sequence,
            frames: observation_frames,
            next_sequence,
            has_more,
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
            return Err("Application streaming observation identities cannot be nil".into());
        }
        if self.response_mode != ApplicationResponseMode::Streaming {
            return Err(
                "Application streaming observation requires Streaming response mode".into(),
            );
        }
        if self
            .frames
            .iter()
            .any(|frame| frame.message_id.as_uuid().is_nil() || frame.sequence == 0)
        {
            return Err("Application streaming observation frame identities cannot be nil".into());
        }
        for window in self.frames.windows(2) {
            if window[0].sequence >= window[1].sequence {
                return Err(
                    "Application streaming observation frames must be strictly ordered by sequence"
                        .into(),
                );
            }
        }
        if self.frames.first().is_some_and(|frame| frame.sequence <= self.after_sequence) {
            return Err(
                "Application streaming observation page must start after the cursor".into(),
            );
        }
        let expected_next = self
            .frames
            .last()
            .map(|frame| frame.sequence)
            .unwrap_or(self.after_sequence);
        if self.next_sequence != expected_next {
            return Err(
                "Application streaming observation next_sequence must match the page tail".into(),
            );
        }
        match self.stream_status {
            ApplicationStreamingObservationStatus::Open => {
                if self.invocation_status.is_terminal() {
                    return Err(
                        "Application streaming observation open status cannot be terminal".into(),
                    );
                }
            }
            ApplicationStreamingObservationStatus::Succeeded => {
                if self.invocation_status != ApplicationInvocationStatus::Succeeded {
                    return Err(
                        "Application streaming observation succeeded status requires Succeeded invocation"
                            .into(),
                    );
                }
            }
            ApplicationStreamingObservationStatus::Failed => {
                if self.invocation_status != ApplicationInvocationStatus::Failed {
                    return Err(
                        "Application streaming observation failed status requires Failed invocation"
                            .into(),
                    );
                }
            }
            ApplicationStreamingObservationStatus::Cancelled => {
                if self.invocation_status != ApplicationInvocationStatus::Cancelled {
                    return Err(
                        "Application streaming observation cancelled status requires Cancelled invocation"
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
        return Err(
            "Application streaming observation invocation is outside the exact session".into(),
        );
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
                "Application streaming observation message is outside the exact session".into(),
            );
        }
        if message.invocation_id != invocation.id {
            return Err(
                "Application streaming observation message belongs to a foreign invocation".into(),
            );
        }
        if message.sequence == 0 || message.sequence > session.last_message_sequence {
            return Err(
                "Application streaming observation message sequence drifts from the session head"
                    .into(),
            );
        }
        frames.push(message.clone());
    }
    frames.sort_by_key(|message| message.sequence);
    for window in frames.windows(2) {
        if window[0].sequence == window[1].sequence {
            return Err(
                "Application streaming observation rejects duplicate message sequences".into(),
            );
        }
        if window[0].id == window[1].id {
            return Err(
                "Application streaming observation rejects duplicate message identities".into(),
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
                        "Application streaming observation Input cannot follow Answer or FinalOutput"
                            .into(),
                    );
                }
                if input_message_id.is_some() {
                    return Err(
                        "Application streaming observation admits at most one Input message".into(),
                    );
                }
                input_message_id = Some(message.id);
            }
            ApplicationMessageKind::Answer => {
                if final_output_message_id.is_some() {
                    return Err(
                        "Application streaming observation Answer cannot follow FinalOutput".into(),
                    );
                }
                saw_answer_or_final = true;
                answer_message_ids.push(message.id);
            }
            ApplicationMessageKind::FinalOutput => {
                if final_output_message_id.is_some() {
                    return Err(
                        "Application streaming observation admits at most one FinalOutput message"
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

fn stream_status_for(
    invocation_status: ApplicationInvocationStatus,
    has_input: bool,
    has_final_output: bool,
) -> Result<ApplicationStreamingObservationStatus, String> {
    match invocation_status {
        ApplicationInvocationStatus::Requested
        | ApplicationInvocationStatus::Running
        | ApplicationInvocationStatus::Cancelling => {
            if has_final_output {
                return Err(
                    "Application streaming observation cannot expose FinalOutput before terminal invocation"
                        .into(),
                );
            }
            Ok(ApplicationStreamingObservationStatus::Open)
        }
        ApplicationInvocationStatus::Succeeded => {
            if !has_input || !has_final_output {
                return Err(
                    "Application streaming observation Succeeded invocation requires Input and FinalOutput"
                        .into(),
                );
            }
            Ok(ApplicationStreamingObservationStatus::Succeeded)
        }
        ApplicationInvocationStatus::Failed => {
            if has_final_output {
                return Err(
                    "Application streaming observation Failed invocation cannot retain FinalOutput"
                        .into(),
                );
            }
            Ok(ApplicationStreamingObservationStatus::Failed)
        }
        ApplicationInvocationStatus::Cancelled => {
            if has_final_output {
                return Err(
                    "Application streaming observation Cancelled invocation cannot retain FinalOutput"
                        .into(),
                );
            }
            Ok(ApplicationStreamingObservationStatus::Cancelled)
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
        let invocation = requested_invocation(session, release, ApplicationResponseMode::Streaming);
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
    fn open_observation_returns_input_and_partial_answers_after_cursor() {
        let release = release_with_modes(vec![ApplicationResponseMode::Streaming]);
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

        let observation = ApplicationStreamingObservation::observe(
            &session,
            &invocation,
            &[input.clone(), answer.clone()],
            0,
            release.created_at + Duration::seconds(3),
        )
        .expect("open");
        assert_eq!(
            observation.stream_status,
            ApplicationStreamingObservationStatus::Open
        );
        assert!(!observation.stream_status.is_terminal());
        assert_eq!(observation.after_sequence, 0);
        assert_eq!(observation.frames.len(), 2);
        assert_eq!(observation.frames[0].message_id, input.id);
        assert_eq!(observation.frames[0].sequence, 1);
        assert_eq!(observation.frames[1].message_id, answer.id);
        assert_eq!(observation.next_sequence, 2);
        assert!(!observation.has_more);
        observation.validate().expect("validate");
    }

    #[test]
    fn cursor_skips_already_seen_frames() {
        let release = release_with_modes(vec![ApplicationResponseMode::Streaming]);
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
        let messages = [input.clone(), first.clone(), second.clone()];

        let first_page = ApplicationStreamingObservation::observe(
            &session,
            &invocation,
            &messages,
            0,
            release.created_at + Duration::seconds(4),
        )
        .expect("first page");
        assert_eq!(first_page.frames.len(), 3);
        assert_eq!(first_page.next_sequence, 3);
        assert!(!first_page.has_more);

        let second_page = ApplicationStreamingObservation::observe(
            &session,
            &invocation,
            &messages,
            1,
            release.created_at + Duration::seconds(5),
        )
        .expect("second page");
        assert_eq!(second_page.frames.len(), 2);
        assert_eq!(second_page.frames[0].message_id, first.id);
        assert_eq!(second_page.frames[1].message_id, second.id);
        assert_eq!(second_page.after_sequence, 1);
        assert_eq!(second_page.next_sequence, 3);
        assert!(!second_page.has_more);
    }

    #[test]
    fn succeeded_observation_orders_answers_and_exposes_final_output() {
        let release = release_with_modes(vec![
            ApplicationResponseMode::Streaming,
            ApplicationResponseMode::Blocking,
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
        let messages = [input.clone(), second.clone(), first.clone(), final_output.clone()];

        let observation = ApplicationStreamingObservation::observe(
            &session,
            &invocation,
            &messages,
            0,
            release.created_at + Duration::seconds(6),
        )
        .expect("succeeded");
        assert_eq!(
            observation.stream_status,
            ApplicationStreamingObservationStatus::Succeeded
        );
        assert_eq!(
            observation
                .frames
                .iter()
                .filter(|frame| frame.kind == ApplicationMessageKind::Answer)
                .map(|frame| frame.message_id)
                .collect::<Vec<_>>(),
            vec![first.id, second.id]
        );
        assert_eq!(
            observation.frames.last().expect("final frame").message_id,
            final_output.id
        );

        let tail = ApplicationStreamingObservation::observe(
            &session,
            &invocation,
            &messages,
            4,
            release.created_at + Duration::seconds(7),
        )
        .expect("tail page");
        assert_eq!(
            tail.stream_status,
            ApplicationStreamingObservationStatus::Succeeded
        );
        assert!(tail.frames.is_empty());
        assert_eq!(tail.next_sequence, 4);
        assert!(!tail.has_more);
    }

    #[test]
    fn failed_and_cancelled_observations_reject_final_output() {
        let release = release_with_modes(vec![ApplicationResponseMode::Streaming]);
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
        let failed_observation = ApplicationStreamingObservation::observe(
            &session,
            &failed,
            &[input.clone()],
            0,
            release.created_at + Duration::seconds(3),
        )
        .expect("failed observation");
        assert_eq!(
            failed_observation.stream_status,
            ApplicationStreamingObservationStatus::Failed
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
        let cancelled_observation = ApplicationStreamingObservation::observe(
            &session,
            &cancelled,
            &[input],
            0,
            release.created_at + Duration::seconds(4),
        )
        .expect("cancelled observation");
        assert_eq!(
            cancelled_observation.stream_status,
            ApplicationStreamingObservationStatus::Cancelled
        );
    }

    #[test]
    fn blocking_mode_and_foreign_messages_fail_closed() {
        let release = release_with_modes(vec![
            ApplicationResponseMode::Blocking,
            ApplicationResponseMode::Streaming,
        ]);
        let session = active_session(&release);
        let blocking = requested_invocation(&session, &release, ApplicationResponseMode::Blocking);
        assert!(
            ApplicationStreamingObservation::observe(
                &session,
                &blocking,
                &[],
                0,
                release.created_at + Duration::seconds(1),
            )
            .is_err()
        );

        let (session, invocation, input) = bound_running(&session, &release);
        let foreign_invocation =
            requested_invocation(&session, &release, ApplicationResponseMode::Streaming);
        let foreign_input =
            ApplicationMessage::input(&session, &foreign_invocation, release.created_at)
                .expect("foreign input");
        assert!(
            ApplicationStreamingObservation::observe(
                &session,
                &invocation,
                &[input, foreign_input],
                0,
                release.created_at + Duration::seconds(2),
            )
            .is_err()
        );
    }

    #[test]
    fn succeeded_without_final_output_and_answer_after_final_fail_closed() {
        let release = release_with_modes(vec![ApplicationResponseMode::Streaming]);
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
            ApplicationStreamingObservation::observe(
                &session,
                &succeeded,
                &[input.clone()],
                0,
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
            ApplicationStreamingObservation::observe(
                &session,
                &succeeded,
                &[input, final_output, late_answer],
                0,
                release.created_at + Duration::seconds(5),
            )
            .is_err()
        );
    }

    #[test]
    fn after_sequence_beyond_session_head_fails_closed() {
        let release = release_with_modes(vec![ApplicationResponseMode::Streaming]);
        let session = active_session(&release);
        let (session, invocation, input) = bound_running(&session, &release);
        assert!(
            ApplicationStreamingObservation::observe(
                &session,
                &invocation,
                &[input],
                2,
                release.created_at + Duration::seconds(2),
            )
            .is_err()
        );
    }
}
