use super::{AgentEventContent, AgentExecutionEventDraft, AgentExecutionEventKind};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentApprovalCheckpointId, AgentApprovalDecisionId, AgentConversationId,
    AgentExecutionId, AuthorizationDecisionRef, EnvironmentId, NodeCommandId, OrganizationId,
    PrincipalId, ProjectId, Sha256Digest,
};
use a3s_cloud_contracts::{
    AgentProviderApprovalDecisionV1, AgentProviderApprovalOutcomeV1, AgentProviderCommandV1,
    AgentProviderRunIdentityV1, AgentProviderToolPayloadIdentityV1, HarnessToolBindingV1,
    AGENT_PROVIDER_TOOL_APPROVAL_TTL_MS_V1,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

const MAX_APPROVAL_REASON_BYTES: usize = 1024;
const MAX_PROVIDER_CALL_ID_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalCheckpointStatus {
    Pending,
    Approved,
    Denied,
    Expired,
    Resumed,
    Cancelled,
}

impl AgentApprovalCheckpointStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Resumed => "resumed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "expired" => Ok(Self::Expired),
            "resumed" => Ok(Self::Resumed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!(
                "unsupported Agent approval checkpoint status {value:?}"
            )),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Resumed | Self::Cancelled)
    }
}

#[derive(Debug, Clone)]
pub struct NewAgentApprovalCheckpoint {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub id: AgentApprovalCheckpointId,
    pub provider_run_identity_digest: Sha256Digest,
    pub invocation_profile_digest: Sha256Digest,
    pub source_event_sequence: u64,
    pub call_id: String,
    pub tool: HarnessToolBindingV1,
    pub request: AgentProviderToolPayloadIdentityV1,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentApprovalCheckpoint {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub id: AgentApprovalCheckpointId,
    pub provider_run_identity_digest: Sha256Digest,
    pub invocation_profile_digest: Sha256Digest,
    pub source_event_sequence: u64,
    pub call_id: String,
    pub tool: HarnessToolBindingV1,
    pub request: AgentProviderToolPayloadIdentityV1,
    pub status: AgentApprovalCheckpointStatus,
    pub decision_id: Option<AgentApprovalDecisionId>,
    pub outcome: Option<AgentProviderApprovalOutcomeV1>,
    pub decided_by: Option<PrincipalId>,
    pub authorization_decision: Option<AuthorizationDecisionRef>,
    pub reason: Option<String>,
    pub decision_digest: Option<Sha256Digest>,
    pub resume_command_id: Option<NodeCommandId>,
    pub resume_command_digest: Option<Sha256Digest>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub resumed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

impl AgentApprovalCheckpoint {
    pub fn deterministic_id(
        execution_id: AgentExecutionId,
        provider_run_identity_digest: &Sha256Digest,
        source_event_sequence: u64,
        call_id: &str,
        request_digest: &str,
    ) -> AgentApprovalCheckpointId {
        AgentApprovalCheckpointId::from_uuid(uuid::Uuid::new_v5(
            &execution_id.as_uuid(),
            format!(
                "a3s-agent-approval-v1:{}:{source_event_sequence}:{call_id}:{request_digest}",
                provider_run_identity_digest.as_str()
            )
            .as_bytes(),
        ))
    }

    pub fn deterministic_expiry_decision_id(&self) -> AgentApprovalDecisionId {
        AgentApprovalDecisionId::from_uuid(uuid::Uuid::new_v5(
            &self.id.as_uuid(),
            format!("a3s-agent-approval-expiry-v1:{}", self.expires_at).as_bytes(),
        ))
    }

    pub fn create(input: NewAgentApprovalCheckpoint) -> Result<Self, String> {
        let requested_at = canonical_timestamp(input.requested_at);
        let expires_at = requested_at
            .checked_add_signed(approval_ttl()?)
            .ok_or_else(|| "Agent approval checkpoint expiry overflowed".to_owned())?;
        let checkpoint = Self {
            organization_id: input.organization_id,
            project_id: input.project_id,
            environment_id: input.environment_id,
            conversation_id: input.conversation_id,
            execution_id: input.execution_id,
            id: input.id,
            provider_run_identity_digest: input.provider_run_identity_digest,
            invocation_profile_digest: input.invocation_profile_digest,
            source_event_sequence: input.source_event_sequence,
            call_id: input.call_id,
            tool: input.tool,
            request: input.request,
            status: AgentApprovalCheckpointStatus::Pending,
            decision_id: None,
            outcome: None,
            decided_by: None,
            authorization_decision: None,
            reason: None,
            decision_digest: None,
            resume_command_id: None,
            resume_command_digest: None,
            aggregate_version: 1,
            requested_at,
            expires_at,
            updated_at: requested_at,
            decided_at: None,
            resumed_at: None,
            cancelled_at: None,
        };
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decide(
        &mut self,
        expected_version: u64,
        decision_id: AgentApprovalDecisionId,
        outcome: AgentProviderApprovalOutcomeV1,
        decided_by: PrincipalId,
        authorization_decision: AuthorizationDecisionRef,
        reason: Option<String>,
        decided_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != AgentApprovalCheckpointStatus::Pending
            || !matches!(
                outcome,
                AgentProviderApprovalOutcomeV1::Approved | AgentProviderApprovalOutcomeV1::Denied
            )
            || decision_id.as_uuid().is_nil()
            || decided_by.as_uuid().is_nil()
        {
            return Err("Agent approval checkpoint cannot accept this interactive decision".into());
        }
        authorization_decision.validate()?;
        validate_agent_approval_reason(reason.as_deref())?;
        let decided_at = canonical_timestamp(decided_at);
        if decided_at >= self.expires_at {
            return Err("Agent approval checkpoint has expired".into());
        }
        self.apply_decision(
            expected_version,
            decision_id,
            outcome,
            Some(decided_by),
            Some(authorization_decision),
            reason,
            decided_at,
        )
    }

    pub fn expire(
        &mut self,
        expected_version: u64,
        decision_id: AgentApprovalDecisionId,
        expired_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != AgentApprovalCheckpointStatus::Pending || decision_id.as_uuid().is_nil() {
            return Err("Agent approval checkpoint cannot expire from its current state".into());
        }
        let expired_at = canonical_timestamp(expired_at);
        if expired_at < self.expires_at {
            return Err("Agent approval checkpoint cannot expire before its deadline".into());
        }
        self.apply_decision(
            expected_version,
            decision_id,
            AgentProviderApprovalOutcomeV1::Expired,
            None,
            None,
            None,
            expired_at,
        )
    }

    pub fn cancel(
        &mut self,
        expected_version: u64,
        cancelled_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status.is_terminal() {
            return Err("terminal Agent approval checkpoint cannot be cancelled".into());
        }
        let mut next = self.next_version(expected_version, cancelled_at)?;
        next.status = AgentApprovalCheckpointStatus::Cancelled;
        next.cancelled_at = Some(next.updated_at);
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn mark_resumed(
        &mut self,
        expected_version: u64,
        command_id: NodeCommandId,
        command: &AgentProviderCommandV1,
        resumed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if !matches!(
            self.status,
            AgentApprovalCheckpointStatus::Approved
                | AgentApprovalCheckpointStatus::Denied
                | AgentApprovalCheckpointStatus::Expired
        ) || command_id.as_uuid().is_nil()
        {
            return Err("Agent approval checkpoint is not ready to resume".into());
        }
        command.validate()?;
        let AgentProviderCommandV1::Resume { request } = command else {
            return Err("Agent approval checkpoint requires a provider resume command".into());
        };
        if request.decision != self.protocol_decision()? {
            return Err("Agent provider resume command changed its approval decision".into());
        }
        let command_digest = Sha256Digest::parse(command.digest()?)?;
        let mut next = self.next_version(expected_version, resumed_at)?;
        next.status = AgentApprovalCheckpointStatus::Resumed;
        next.resume_command_id = Some(command_id);
        next.resume_command_digest = Some(command_digest);
        next.resumed_at = Some(next.updated_at);
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn protocol_decision_for(
        &self,
        identity: &AgentProviderRunIdentityV1,
    ) -> Result<AgentProviderApprovalDecisionV1, String> {
        identity.validate()?;
        if identity.digest()? != self.provider_run_identity_digest.as_str() {
            return Err("Agent approval checkpoint targets another provider run".into());
        }
        self.protocol_decision()
    }

    pub fn protocol_decision(&self) -> Result<AgentProviderApprovalDecisionV1, String> {
        let decision = AgentProviderApprovalDecisionV1 {
            schema: AgentProviderApprovalDecisionV1::SCHEMA.into(),
            decision_id: self
                .decision_id
                .ok_or_else(|| "Agent approval checkpoint has no decision".to_owned())?
                .to_string(),
            checkpoint_id: self.id.to_string(),
            run_identity_digest: self.provider_run_identity_digest.as_str().into(),
            call_id: self.call_id.clone(),
            tool: self.tool.clone(),
            request_digest: self.request.digest.clone(),
            outcome: self
                .outcome
                .ok_or_else(|| "Agent approval checkpoint has no outcome".to_owned())?,
            decided_at_ms: timestamp_ms(
                self.decided_at
                    .ok_or_else(|| "Agent approval checkpoint has no decision time".to_owned())?,
            )?,
        };
        decision.validate()?;
        let stored = self
            .decision_digest
            .as_ref()
            .ok_or_else(|| "Agent approval checkpoint has no decision digest".to_owned())?;
        if decision.digest()? != stored.as_str() {
            return Err("Agent approval checkpoint decision digest changed".into());
        }
        Ok(decision)
    }

    pub fn resolution_event_draft(&self) -> Result<AgentExecutionEventDraft, String> {
        let decision_id = self
            .decision_id
            .ok_or_else(|| "Agent approval checkpoint has no decision".to_owned())?;
        let outcome = self
            .outcome
            .ok_or_else(|| "Agent approval checkpoint has no outcome".to_owned())?;
        let decided_at = self
            .decided_at
            .ok_or_else(|| "Agent approval checkpoint has no decision time".to_owned())?;
        let content = AgentEventContent::inline_json(serde_json::json!({
            "checkpointId": self.id,
            "decisionId": decision_id,
            "outcome": outcome,
            "decisionDigest": self
                .decision_digest
                .as_ref()
                .ok_or_else(|| "Agent approval checkpoint has no decision digest".to_owned())?,
            "decidedBy": self.decided_by,
            "authorizationDecision": self.authorization_decision,
            "reason": self.reason,
        }))?;
        AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ApprovalResolved,
            content,
            decided_at,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        self.tool.validate()?;
        self.request.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.conversation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || !self.tool.approval_required
            || !valid_call_id(&self.call_id)
            || self.aggregate_version == 0
            || self.requested_at != canonical_timestamp(self.requested_at)
            || self.expires_at != canonical_timestamp(self.expires_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.requested_at
            || self.expires_at
                != self
                    .requested_at
                    .checked_add_signed(approval_ttl()?)
                    .ok_or_else(|| "Agent approval checkpoint expiry overflowed".to_owned())?
        {
            return Err("stored Agent approval checkpoint identity or deadline is invalid".into());
        }
        validate_agent_approval_reason(self.reason.as_deref())?;
        if let Some(authorization) = &self.authorization_decision {
            authorization.validate()?;
        }
        for timestamp in [self.decided_at, self.resumed_at, self.cancelled_at]
            .into_iter()
            .flatten()
        {
            if timestamp != canonical_timestamp(timestamp)
                || timestamp < self.requested_at
                || timestamp > self.updated_at
            {
                return Err("stored Agent approval checkpoint timestamp is invalid".into());
            }
        }

        let has_decision = self.decision_id.is_some()
            && self.outcome.is_some()
            && self.decision_digest.is_some()
            && self.decided_at.is_some();
        let has_partial_decision = self.decision_id.is_some()
            || self.outcome.is_some()
            || self.decided_by.is_some()
            || self.authorization_decision.is_some()
            || self.reason.is_some()
            || self.decision_digest.is_some()
            || self.decided_at.is_some();
        if has_partial_decision && !has_decision {
            return Err("stored Agent approval checkpoint decision is incomplete".into());
        }
        if let Some(outcome) = self.outcome {
            match outcome {
                AgentProviderApprovalOutcomeV1::Approved
                | AgentProviderApprovalOutcomeV1::Denied => {
                    if self.decided_by.is_none() || self.authorization_decision.is_none() {
                        return Err(
                            "interactive Agent approval decision omitted its authority".into()
                        );
                    }
                    if self
                        .decided_at
                        .is_some_and(|value| value >= self.expires_at)
                    {
                        return Err("interactive Agent approval decision was expired".into());
                    }
                }
                AgentProviderApprovalOutcomeV1::Expired => {
                    if self.decided_by.is_some()
                        || self.authorization_decision.is_some()
                        || self.reason.is_some()
                        || self.decided_at.is_none_or(|value| value < self.expires_at)
                    {
                        return Err("expired Agent approval decision is invalid".into());
                    }
                }
            }
        }

        let has_resume = self.resume_command_id.is_some()
            && self.resume_command_digest.is_some()
            && self.resumed_at.is_some();
        let has_partial_resume = self.resume_command_id.is_some()
            || self.resume_command_digest.is_some()
            || self.resumed_at.is_some();
        if has_partial_resume && !has_resume {
            return Err("stored Agent approval checkpoint resume evidence is incomplete".into());
        }
        let lifecycle_matches = match self.status {
            AgentApprovalCheckpointStatus::Pending => {
                !has_partial_decision && !has_partial_resume && self.cancelled_at.is_none()
            }
            AgentApprovalCheckpointStatus::Approved => {
                self.outcome == Some(AgentProviderApprovalOutcomeV1::Approved)
                    && has_decision
                    && !has_partial_resume
                    && self.cancelled_at.is_none()
            }
            AgentApprovalCheckpointStatus::Denied => {
                self.outcome == Some(AgentProviderApprovalOutcomeV1::Denied)
                    && has_decision
                    && !has_partial_resume
                    && self.cancelled_at.is_none()
            }
            AgentApprovalCheckpointStatus::Expired => {
                self.outcome == Some(AgentProviderApprovalOutcomeV1::Expired)
                    && has_decision
                    && !has_partial_resume
                    && self.cancelled_at.is_none()
            }
            AgentApprovalCheckpointStatus::Resumed => {
                has_decision && has_resume && self.cancelled_at.is_none()
            }
            AgentApprovalCheckpointStatus::Cancelled => {
                !has_partial_resume && self.cancelled_at == Some(self.updated_at)
            }
        };
        if !lifecycle_matches {
            return Err("stored Agent approval checkpoint lifecycle is inconsistent".into());
        }
        if has_decision {
            self.protocol_decision()?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_decision(
        &mut self,
        expected_version: u64,
        decision_id: AgentApprovalDecisionId,
        outcome: AgentProviderApprovalOutcomeV1,
        decided_by: Option<PrincipalId>,
        authorization_decision: Option<AuthorizationDecisionRef>,
        reason: Option<String>,
        decided_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let mut next = self.next_version(expected_version, decided_at)?;
        next.status = match outcome {
            AgentProviderApprovalOutcomeV1::Approved => AgentApprovalCheckpointStatus::Approved,
            AgentProviderApprovalOutcomeV1::Denied => AgentApprovalCheckpointStatus::Denied,
            AgentProviderApprovalOutcomeV1::Expired => AgentApprovalCheckpointStatus::Expired,
        };
        next.decision_id = Some(decision_id);
        next.outcome = Some(outcome);
        next.decided_by = decided_by;
        next.authorization_decision = authorization_decision;
        next.reason = reason;
        next.decided_at = Some(next.updated_at);
        let decision = AgentProviderApprovalDecisionV1 {
            schema: AgentProviderApprovalDecisionV1::SCHEMA.into(),
            decision_id: decision_id.to_string(),
            checkpoint_id: next.id.to_string(),
            run_identity_digest: next.provider_run_identity_digest.as_str().into(),
            call_id: next.call_id.clone(),
            tool: next.tool.clone(),
            request_digest: next.request.digest.clone(),
            outcome,
            decided_at_ms: timestamp_ms(next.updated_at)?,
        };
        next.decision_digest = Some(Sha256Digest::parse(decision.digest()?)?);
        next.validate()?;
        *self = next;
        Ok(())
    }

    fn next_version(
        &self,
        expected_version: u64,
        occurred_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.validate()?;
        if expected_version == 0 || self.aggregate_version != expected_version {
            return Err("Agent approval checkpoint version changed".into());
        }
        let occurred_at = canonical_timestamp(occurred_at);
        if occurred_at < self.updated_at {
            return Err("Agent approval checkpoint transition time regressed".into());
        }
        let mut next = self.clone();
        next.aggregate_version = next
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Agent approval checkpoint version is exhausted".to_owned())?;
        next.updated_at = occurred_at;
        Ok(next)
    }
}

fn approval_ttl() -> Result<Duration, String> {
    i64::try_from(AGENT_PROVIDER_TOOL_APPROVAL_TTL_MS_V1)
        .map(Duration::milliseconds)
        .map_err(|_| "Agent approval checkpoint TTL exceeds the supported range".into())
}

fn timestamp_ms(value: DateTime<Utc>) -> Result<u64, String> {
    u64::try_from(value.timestamp_millis())
        .map_err(|_| "Agent approval checkpoint timestamp is outside protocol bounds".into())
}

fn valid_call_id(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= MAX_PROVIDER_CALL_ID_BYTES
        && !value.contains(['\0', '\r', '\n'])
}

pub fn validate_agent_approval_reason(reason: Option<&str>) -> Result<(), String> {
    if reason.is_some_and(|value| {
        value.is_empty()
            || value.trim() != value
            || value.len() > MAX_APPROVAL_REASON_BYTES
            || value.contains(['\0', '\r', '\n'])
    }) {
        Err("Agent approval decision reason is invalid".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{AgentProviderCommandV1, AgentProviderRunResumeV1};

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }

    fn identity() -> AgentProviderRunIdentityV1 {
        AgentProviderRunIdentityV1::new(
            digest('a').to_string(),
            digest('b').to_string(),
            digest('c').to_string(),
            "conversation-1".into(),
            "execution-1".into(),
        )
        .expect("identity")
    }

    fn checkpoint(at: DateTime<Utc>) -> AgentApprovalCheckpoint {
        let identity = identity();
        AgentApprovalCheckpoint::create(NewAgentApprovalCheckpoint {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            conversation_id: AgentConversationId::new(),
            execution_id: AgentExecutionId::new(),
            id: AgentApprovalCheckpointId::new(),
            provider_run_identity_digest: Sha256Digest::parse(identity.digest().expect("digest"))
                .expect("identity digest"),
            invocation_profile_digest: digest('d'),
            source_event_sequence: 0,
            call_id: "call-1".into(),
            tool: HarnessToolBindingV1 {
                name: "workspace.publish".into(),
                revision: "1.0.0".into(),
                contract_digest: digest('e').to_string(),
                approval_required: true,
            },
            request: AgentProviderToolPayloadIdentityV1 {
                digest: digest('f').to_string(),
                size_bytes: 42,
                media_type: "application/json".into(),
            },
            requested_at: at,
        })
        .expect("checkpoint")
    }

    fn authorization() -> AuthorizationDecisionRef {
        AuthorizationDecisionRef::new("grant-evaluation-1", digest('9')).expect("authorization")
    }

    #[test]
    fn approved_checkpoint_binds_one_exact_resume_command() {
        let at = canonical_timestamp(Utc::now());
        let identity = identity();
        let mut checkpoint = checkpoint(at);
        assert_eq!(
            checkpoint.expires_at,
            at + Duration::milliseconds(86_400_000)
        );
        checkpoint
            .decide(
                1,
                AgentApprovalDecisionId::new(),
                AgentProviderApprovalOutcomeV1::Approved,
                PrincipalId::new(),
                authorization(),
                Some("release approved".into()),
                at + Duration::seconds(1),
            )
            .expect("approve");
        let decision = checkpoint
            .protocol_decision_for(&identity)
            .expect("protocol decision");
        let command = AgentProviderCommandV1::Resume {
            request: AgentProviderRunResumeV1::new("resume-1".into(), identity, decision)
                .expect("resume request"),
        };
        checkpoint
            .mark_resumed(2, NodeCommandId::new(), &command, at + Duration::seconds(2))
            .expect("mark resumed");
        assert_eq!(checkpoint.status, AgentApprovalCheckpointStatus::Resumed);
        checkpoint.validate().expect("valid resumed checkpoint");
    }

    #[test]
    fn expiry_and_cancellation_win_without_hidden_resume() {
        let at = canonical_timestamp(Utc::now());
        let mut expired = checkpoint(at);
        assert!(expired
            .expire(1, AgentApprovalDecisionId::new(), at + Duration::hours(1))
            .is_err());
        let expires_at = expired.expires_at;
        expired
            .expire(1, AgentApprovalDecisionId::new(), expires_at)
            .expect("expire at deadline");
        assert_eq!(expired.status, AgentApprovalCheckpointStatus::Expired);

        let mut cancelled = checkpoint(at);
        cancelled
            .decide(
                1,
                AgentApprovalDecisionId::new(),
                AgentProviderApprovalOutcomeV1::Denied,
                PrincipalId::new(),
                authorization(),
                None,
                at + Duration::seconds(1),
            )
            .expect("deny");
        cancelled
            .cancel(2, at + Duration::seconds(2))
            .expect("cancellation wins");
        assert_eq!(cancelled.status, AgentApprovalCheckpointStatus::Cancelled);
        assert!(cancelled
            .mark_resumed(
                3,
                NodeCommandId::new(),
                &AgentProviderCommandV1::Resume {
                    request: AgentProviderRunResumeV1::new(
                        "resume-after-cancel".into(),
                        identity(),
                        cancelled.protocol_decision().expect("retained decision"),
                    )
                    .expect("resume request"),
                },
                at + Duration::seconds(3),
            )
            .is_err());
        cancelled.validate().expect("valid cancelled checkpoint");
    }
}
