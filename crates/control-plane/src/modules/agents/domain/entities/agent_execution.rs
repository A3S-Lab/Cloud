use super::{
    AgentCodeRunBinding, AgentExecutionEventDraft, AgentExecutionEventKind,
    AgentProviderProfileBinding, AgentReleaseBinding,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentConversationId, AgentExecutionId, OperationId, OrganizationId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExecutionStatus {
    Pending,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

impl AgentExecutionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "cancelling" => Ok(Self::Cancelling),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unsupported Agent execution status {value:?}")),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecution {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub id: AgentExecutionId,
    pub operation_id: OperationId,
    pub agent: AgentReleaseBinding,
    pub provider: AgentProviderProfileBinding,
    pub code: Option<AgentCodeRunBinding>,
    pub status: AgentExecutionStatus,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl AgentExecution {
    pub fn create(
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        id: AgentExecutionId,
        operation_id: OperationId,
        agent: AgentReleaseBinding,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::create_with_provider(
            organization_id,
            conversation_id,
            id,
            operation_id,
            agent,
            AgentProviderProfileBinding::native_code()?,
            requested_at,
        )
    }

    pub fn create_with_provider(
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        id: AgentExecutionId,
        operation_id: OperationId,
        agent: AgentReleaseBinding,
        provider: AgentProviderProfileBinding,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let requested_at = canonical_timestamp(requested_at);
        let execution = Self {
            organization_id,
            conversation_id,
            id,
            operation_id,
            agent,
            provider,
            code: None,
            status: AgentExecutionStatus::Pending,
            failure: None,
            aggregate_version: 1,
            requested_at,
            updated_at: requested_at,
            started_at: None,
            cancellation_requested_at: None,
            finished_at: None,
        };
        execution.validate()?;
        Ok(execution)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.requested_at = canonical_timestamp(self.requested_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.started_at = self.started_at.map(canonical_timestamp);
        self.cancellation_requested_at = self.cancellation_requested_at.map(canonical_timestamp);
        self.finished_at = self.finished_at.map(canonical_timestamp);
        if let Some(code) = self.code.as_mut() {
            code.restore_legacy_provider()?;
        }
        self.validate()?;
        Ok(self)
    }

    pub fn start(&mut self, started_at: DateTime<Utc>) -> Result<(), String> {
        if self.status == AgentExecutionStatus::Running {
            return self.observe_time(started_at);
        }
        if self.status != AgentExecutionStatus::Pending {
            return Err("Agent execution cannot start from its current state".into());
        }
        self.transition(AgentExecutionStatus::Running, started_at)?;
        self.started_at = Some(self.updated_at);
        Ok(())
    }

    pub fn bind_code_run(&mut self, binding: AgentCodeRunBinding) -> Result<bool, String> {
        binding.validate()?;
        if !binding.is_initial()
            || binding.provider()? != &self.provider
            || binding.identity().agent_release_identity.as_str()
                != self.agent.artifact_digest().as_str()
        {
            return Err("Agent execution Code run binding does not match its release".into());
        }
        if let Some(existing) = &self.code {
            return if existing.has_same_run_binding(&binding) {
                Ok(false)
            } else {
                Err("Agent execution cannot change its bound A3S Code run".into())
            };
        }
        if self.status != AgentExecutionStatus::Pending || binding.bound_at() < self.updated_at {
            return Err("Agent execution cannot change its bound A3S Code run".into());
        }
        self.record_observation(binding.bound_at())?;
        self.code = Some(binding);
        self.validate()?;
        Ok(true)
    }

    pub fn recover_code_run(
        &mut self,
        expected: &AgentCodeRunBinding,
        recovered_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        expected.validate()?;
        if self.status.is_terminal() {
            return Err("terminal Agent execution cannot recover its A3S Code run".into());
        }
        let current = self
            .code
            .as_ref()
            .ok_or_else(|| "Agent execution has no bound A3S Code run".to_string())?;
        if current.is_recovery_successor_of(expected, self.id) {
            return Ok(false);
        }
        if !current.has_same_run_binding(expected) {
            return Err("Agent execution Code recovery checkpoint changed".into());
        }

        let recovered_at = canonical_timestamp(recovered_at);
        if recovered_at < self.updated_at {
            return Err("Agent execution Code recovery time regressed".into());
        }
        let successor = current.recovery_successor(self.id, recovered_at)?;
        let mut next = self.clone();
        next.record_observation(recovered_at)?;
        next.code = Some(successor);
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn request_cancellation(&mut self, requested_at: DateTime<Utc>) -> Result<(), String> {
        if self.status.is_terminal() {
            return Err("terminal Agent execution cannot be cancelled".into());
        }
        if self.status == AgentExecutionStatus::Cancelling {
            return Err("Agent execution cancellation is already requested".into());
        }
        if !matches!(
            self.status,
            AgentExecutionStatus::Pending | AgentExecutionStatus::Running
        ) {
            return Err("Agent execution cannot be cancelled from its current state".into());
        }
        self.transition(AgentExecutionStatus::Cancelling, requested_at)?;
        self.cancellation_requested_at = Some(self.updated_at);
        self.validate()
    }

    pub fn accept_code_event_page(
        &mut self,
        page: &a3s_cloud_contracts::AgentProtocolEventPageV1,
        accepted_at: DateTime<Utc>,
        semantic_events: &[AgentExecutionEventDraft],
    ) -> Result<(), String> {
        let mut next = self.clone();
        let state = {
            let binding = next
                .code
                .as_mut()
                .ok_or_else(|| "Agent execution has no bound A3S Code run".to_string())?;
            binding.accept_event_page(page)?;
            binding.observed_state()
        };
        let accepted_at = canonical_timestamp(accepted_at).max(next.updated_at);
        if next.status.is_terminal() {
            return Err("terminal Agent execution cannot accept A3S Code events".into());
        }
        if state == a3s_cloud_contracts::AgentProtocolRunStateV1::Created {
            next.record_observation(accepted_at)?;
        } else if next.status == AgentExecutionStatus::Pending {
            next.start(accepted_at)?;
        } else {
            next.record_observation(accepted_at)?;
        }
        for event in semantic_events {
            if event.occurred_at != accepted_at
                || event.kind == AgentExecutionEventKind::ExecutionRequested
            {
                return Err("A3S Code semantic projection is invalid".into());
            }
            next.apply_event_inner(event)?;
        }
        let expected_terminal = state.is_terminal() && !page.has_more;
        let terminal_matches = if expected_terminal {
            match state {
                a3s_cloud_contracts::AgentProtocolRunStateV1::Completed => {
                    next.status == AgentExecutionStatus::Succeeded
                }
                a3s_cloud_contracts::AgentProtocolRunStateV1::Failed => {
                    next.status == AgentExecutionStatus::Failed
                }
                a3s_cloud_contracts::AgentProtocolRunStateV1::Cancelled => {
                    next.status == AgentExecutionStatus::Cancelled
                }
                _ => false,
            }
        } else {
            !next.status.is_terminal()
        };
        if !terminal_matches {
            return Err("A3S Code page and semantic terminal projection disagree".into());
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn accept_provider_event_page(
        &mut self,
        page: &a3s_cloud_contracts::AgentProviderEventPageV1,
        accepted_at: DateTime<Utc>,
        semantic_events: &[AgentExecutionEventDraft],
    ) -> Result<(), String> {
        let mut next = self.clone();
        let state = {
            let binding = next
                .code
                .as_mut()
                .ok_or_else(|| "Agent execution has no bound provider run".to_string())?;
            binding.accept_provider_event_page(page)?;
            page.state
        };
        let accepted_at = canonical_timestamp(accepted_at).max(next.updated_at);
        if next.status.is_terminal() {
            return Err("terminal Agent execution cannot accept provider events".into());
        }
        if state == a3s_cloud_contracts::AgentProviderRunStateV1::Created {
            next.record_observation(accepted_at)?;
        } else if next.status == AgentExecutionStatus::Pending {
            next.start(accepted_at)?;
        } else {
            next.record_observation(accepted_at)?;
        }
        for event in semantic_events {
            if event.occurred_at != accepted_at
                || event.kind == AgentExecutionEventKind::ExecutionRequested
            {
                return Err("Agent provider semantic projection is invalid".into());
            }
            next.apply_event_inner(event)?;
        }
        let expected_terminal = state.is_terminal() && !page.has_more;
        let terminal_matches = if expected_terminal {
            match state {
                a3s_cloud_contracts::AgentProviderRunStateV1::Completed => {
                    next.status == AgentExecutionStatus::Succeeded
                }
                a3s_cloud_contracts::AgentProviderRunStateV1::Failed => {
                    next.status == AgentExecutionStatus::Failed
                }
                a3s_cloud_contracts::AgentProviderRunStateV1::Cancelled => {
                    next.status == AgentExecutionStatus::Cancelled
                }
                _ => false,
            }
        } else {
            !next.status.is_terminal()
        };
        if !terminal_matches {
            return Err("Agent provider page and semantic terminal projection disagree".into());
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn succeed(&mut self, finished_at: DateTime<Utc>) -> Result<(), String> {
        self.finish(AgentExecutionStatus::Succeeded, None, finished_at)
    }

    pub fn fail(
        &mut self,
        reason: impl Into<String>,
        finished_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let reason = reason.into();
        validate_failure(&reason)?;
        self.finish(AgentExecutionStatus::Failed, Some(reason), finished_at)
    }

    pub fn cancel(&mut self, finished_at: DateTime<Utc>) -> Result<(), String> {
        self.finish(AgentExecutionStatus::Cancelled, None, finished_at)
    }

    pub fn apply_event(&mut self, event: &AgentExecutionEventDraft) -> Result<(), String> {
        let mut next = self.clone();
        next.apply_event_inner(event)?;
        *self = next;
        Ok(())
    }

    fn apply_event_inner(&mut self, event: &AgentExecutionEventDraft) -> Result<(), String> {
        match event.kind {
            AgentExecutionEventKind::ExecutionRequested => {
                Err("execution_requested is reserved for execution creation".into())
            }
            AgentExecutionEventKind::ModelOutput => {
                if self.status == AgentExecutionStatus::Pending {
                    self.start(event.occurred_at)
                } else if matches!(
                    self.status,
                    AgentExecutionStatus::Running | AgentExecutionStatus::Cancelling
                ) {
                    self.record_observation(event.occurred_at)
                } else {
                    Err("terminal Agent execution cannot emit model output".into())
                }
            }
            AgentExecutionEventKind::ExecutionCompleted => {
                if self.status == AgentExecutionStatus::Pending {
                    self.start(event.occurred_at)?;
                }
                self.succeed(event.occurred_at)
            }
            AgentExecutionEventKind::ExecutionFailed => {
                let reason = event
                    .content
                    .value()
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        "execution_failed content must contain a string reason".to_owned()
                    })?;
                self.fail(reason, event.occurred_at)
            }
            AgentExecutionEventKind::ExecutionCancelled => self.cancel(event.occurred_at),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.agent.validate()?;
        self.provider.validate()?;
        if let Some(code) = &self.code {
            code.validate()?;
            if code.provider()? != &self.provider
                || code.identity().agent_release_identity.as_str()
                    != self.agent.artifact_digest().as_str()
                || code.bound_at() < self.requested_at
            {
                return Err("Agent Code run binding falls outside its execution".into());
            }
        }
        if self.organization_id.as_uuid().is_nil()
            || self.conversation_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.organization_id != self.agent.organization_id()
            || self.aggregate_version == 0
            || self.requested_at != canonical_timestamp(self.requested_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.requested_at
            || self.started_at.is_some_and(|value| {
                value != canonical_timestamp(value) || value < self.requested_at
            })
            || self.cancellation_requested_at.is_some_and(|value| {
                value != canonical_timestamp(value)
                    || value < self.requested_at
                    || value > self.updated_at
            })
            || self.finished_at.is_some_and(|value| {
                value != canonical_timestamp(value) || value < self.requested_at
            })
            || (self.status == AgentExecutionStatus::Running && self.started_at.is_none())
            || (self.status == AgentExecutionStatus::Cancelling
                && self.cancellation_requested_at.is_none())
            || self.cancellation_requested_at.is_some()
                && matches!(
                    self.status,
                    AgentExecutionStatus::Pending | AgentExecutionStatus::Running
                )
            || self.status.is_terminal() != self.finished_at.is_some()
            || (self.status == AgentExecutionStatus::Failed) != self.failure.is_some()
        {
            return Err("Agent execution aggregate is invalid".into());
        }
        if let Some(failure) = &self.failure {
            validate_failure(failure)?;
        }
        Ok(())
    }

    fn finish(
        &mut self,
        status: AgentExecutionStatus,
        failure: Option<String>,
        finished_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if !status.is_terminal() || self.status.is_terminal() {
            return Err("terminal Agent execution cannot change its outcome".into());
        }
        if !matches!(
            self.status,
            AgentExecutionStatus::Pending
                | AgentExecutionStatus::Running
                | AgentExecutionStatus::Cancelling
        ) {
            return Err("Agent execution cannot finish from its current state".into());
        }
        self.transition(status, finished_at)?;
        self.failure = failure;
        self.finished_at = Some(self.updated_at);
        Ok(())
    }

    fn transition(
        &mut self,
        status: AgentExecutionStatus,
        occurred_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let occurred_at = canonical_timestamp(occurred_at);
        if occurred_at < self.updated_at {
            return Err("Agent execution transition time regressed".into());
        }
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Agent execution aggregate version overflowed".to_owned())?;
        self.updated_at = occurred_at;
        self.status = status;
        self.aggregate_version = aggregate_version;
        Ok(())
    }

    fn observe_time(&mut self, occurred_at: DateTime<Utc>) -> Result<(), String> {
        let occurred_at = canonical_timestamp(occurred_at);
        if occurred_at < self.updated_at {
            return Err("Agent execution transition time regressed".into());
        }
        self.updated_at = occurred_at;
        Ok(())
    }

    fn record_observation(&mut self, occurred_at: DateTime<Utc>) -> Result<(), String> {
        let occurred_at = canonical_timestamp(occurred_at);
        if occurred_at < self.updated_at {
            return Err("Agent execution observation time regressed".into());
        }
        self.updated_at = occurred_at;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Agent execution aggregate version overflowed".to_owned())?;
        Ok(())
    }
}

fn validate_failure(reason: &str) -> Result<(), String> {
    if reason.is_empty()
        || reason.len() > super::MAX_AGENT_EXECUTION_FAILURE_BYTES
        || reason.contains(['\0', '\r', '\n'])
    {
        return Err("Agent execution failure reason is invalid".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::AgentEventContent;
    use super::*;
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, BuildRunId, DeploymentId, NodeId, Sha256Digest, WorkloadId,
        WorkloadReplicaId, WorkloadRevisionId,
    };

    fn binding(organization_id: OrganizationId) -> AgentReleaseBinding {
        let digest = Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest");
        AgentReleaseBinding::new(
            organization_id,
            AssetId::new(),
            AssetReleaseId::new(),
            BuildRunId::new(),
            format!("oci://registry.example/agents/research@{digest}"),
            digest,
            "application/vnd.oci.image.manifest.v1+json",
            42,
        )
        .expect("binding")
    }

    fn code_binding(execution: &AgentExecution, bound_at: DateTime<Utc>) -> AgentCodeRunBinding {
        AgentCodeRunBinding::new(
            NodeId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            DeploymentId::new(),
            WorkloadReplicaId::new(),
            "agent-runtime:revision:1",
            1,
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Runtime digest"),
            "agent",
            a3s_cloud_contracts::AgentProtocolRunIdentityV1 {
                schema: a3s_cloud_contracts::AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: a3s_cloud_contracts::AGENT_PROTOCOL_V1.into(),
                agent_release_identity: execution.agent.artifact_digest().as_str().into(),
                session_id: format!("agent-conversation-{}", execution.conversation_id),
                run_id: format!("agent-execution-{}", execution.id),
            },
            bound_at,
        )
        .expect("Code run binding")
    }

    #[test]
    fn lifecycle_is_monotonic_and_terminal() {
        let organization_id = OrganizationId::new();
        let at = Utc::now();
        let mut execution = AgentExecution::create(
            organization_id,
            AgentConversationId::new(),
            AgentExecutionId::new(),
            OperationId::new(),
            binding(organization_id),
            at,
        )
        .expect("execution");

        execution.start(at).expect("start");
        execution.succeed(at).expect("succeed");
        assert_eq!(execution.status, AgentExecutionStatus::Succeeded);
        assert!(execution.fail("late failure", at).is_err());
    }

    #[test]
    fn semantic_events_drive_the_logical_execution_state() {
        let organization_id = OrganizationId::new();
        let at = Utc::now();
        let mut execution = AgentExecution::create(
            organization_id,
            AgentConversationId::new(),
            AgentExecutionId::new(),
            OperationId::new(),
            binding(organization_id),
            at,
        )
        .expect("execution");
        let output = AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ModelOutput,
            AgentEventContent::inline_json(serde_json::json!({"text": "hello"})).expect("content"),
            at,
        )
        .expect("output");
        execution.apply_event(&output).expect("apply output");
        assert_eq!(execution.status, AgentExecutionStatus::Running);

        let completed = AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ExecutionCompleted,
            AgentEventContent::inline_json(serde_json::json!({})).expect("content"),
            at,
        )
        .expect("completed");
        execution.apply_event(&completed).expect("apply completion");
        assert_eq!(execution.status, AgentExecutionStatus::Succeeded);
        assert!(execution.apply_event(&output).is_err());
    }

    #[test]
    fn rejected_event_transition_does_not_partially_mutate_the_aggregate() {
        let organization_id = OrganizationId::new();
        let at = Utc::now();
        let mut execution = AgentExecution::create(
            organization_id,
            AgentConversationId::new(),
            AgentExecutionId::new(),
            OperationId::new(),
            binding(organization_id),
            at,
        )
        .expect("execution");
        execution.aggregate_version = u64::MAX - 1;
        let before = execution.clone();
        let completed = AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ExecutionCompleted,
            AgentEventContent::inline_json(serde_json::json!({})).expect("content"),
            at,
        )
        .expect("completed");

        assert!(execution.apply_event(&completed).is_err());
        assert_eq!(execution, before);
    }

    #[test]
    fn cancellation_intent_is_monotonic_and_preserved_on_terminal_outcomes() {
        let organization_id = OrganizationId::new();
        let at = canonical_timestamp(Utc::now());
        let mut execution = AgentExecution::create(
            organization_id,
            AgentConversationId::new(),
            AgentExecutionId::new(),
            OperationId::new(),
            binding(organization_id),
            at,
        )
        .expect("execution");
        let cancelled_at = at + chrono::Duration::seconds(1);

        execution
            .request_cancellation(cancelled_at)
            .expect("request cancellation");
        assert_eq!(execution.status, AgentExecutionStatus::Cancelling);
        assert_eq!(execution.cancellation_requested_at, Some(cancelled_at));
        assert_eq!(execution.aggregate_version, 2);
        assert!(execution.request_cancellation(cancelled_at).is_err());

        let output = AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ModelOutput,
            AgentEventContent::inline_json(serde_json::json!({"text": "late"})).expect("content"),
            cancelled_at,
        )
        .expect("output");
        execution.apply_event(&output).expect("late output");
        assert_eq!(execution.status, AgentExecutionStatus::Cancelling);

        execution
            .cancel(cancelled_at + chrono::Duration::seconds(1))
            .expect("cancel");
        assert_eq!(execution.status, AgentExecutionStatus::Cancelled);
        assert_eq!(execution.cancellation_requested_at, Some(cancelled_at));
        execution.validate().expect("valid cancelled execution");
    }

    #[test]
    fn completion_may_win_a_race_with_cancellation() {
        let organization_id = OrganizationId::new();
        let at = canonical_timestamp(Utc::now());
        let mut execution = AgentExecution::create(
            organization_id,
            AgentConversationId::new(),
            AgentExecutionId::new(),
            OperationId::new(),
            binding(organization_id),
            at,
        )
        .expect("execution");
        execution.start(at).expect("start");
        execution
            .request_cancellation(at)
            .expect("request cancellation");
        execution.succeed(at).expect("completion wins");

        assert_eq!(execution.status, AgentExecutionStatus::Succeeded);
        assert_eq!(execution.cancellation_requested_at, Some(at));
        execution.validate().expect("valid completed execution");
    }

    #[test]
    fn code_recovery_replays_after_the_successor_has_started() {
        let organization_id = OrganizationId::new();
        let at = canonical_timestamp(Utc::now());
        let mut execution = AgentExecution::create(
            organization_id,
            AgentConversationId::new(),
            AgentExecutionId::new(),
            OperationId::new(),
            binding(organization_id),
            at,
        )
        .expect("execution");
        let checkpoint = code_binding(&execution, at);
        execution
            .bind_code_run(checkpoint.clone())
            .expect("bind Code run");

        let recovered_at = at + chrono::Duration::seconds(1);
        assert!(execution
            .recover_code_run(&checkpoint, recovered_at)
            .expect("recover Code run"));
        let first_recovery = execution.clone();
        assert!(!execution
            .recover_code_run(&checkpoint, recovered_at)
            .expect("replay recovery"));
        assert_eq!(execution, first_recovery);

        let identity = execution
            .code
            .as_ref()
            .expect("recovered binding")
            .identity()
            .clone();
        let page = a3s_cloud_contracts::AgentProtocolEventPageV1 {
            schema: a3s_cloud_contracts::AgentProtocolEventPageV1::SCHEMA.into(),
            identity,
            after_event_sequence: None,
            first_available_sequence: None,
            latest_sequence_exclusive: 0,
            next_after_event_sequence: None,
            state: a3s_cloud_contracts::AgentProtocolRunStateV1::Planning,
            observed_at_ms: u64::try_from((at + chrono::Duration::days(1)).timestamp_millis())
                .expect("provider timestamp"),
            retention_gap: false,
            has_more: false,
            events: Vec::new(),
        };
        execution
            .accept_code_event_page(&page, recovered_at + chrono::Duration::seconds(1), &[])
            .expect("start recovered run");
        let progressed = execution.clone();
        assert!(!execution
            .recover_code_run(&checkpoint, recovered_at + chrono::Duration::seconds(2),)
            .expect("replay recovery after progress"));
        assert_eq!(execution, progressed);
    }
}
