use super::{AgentExecutionEventDraft, AgentExecutionEventKind, AgentReleaseBinding};
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
    Succeeded,
    Failed,
    Cancelled,
}

impl AgentExecutionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
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
    pub status: AgentExecutionStatus,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
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
        let requested_at = canonical_timestamp(requested_at);
        let execution = Self {
            organization_id,
            conversation_id,
            id,
            operation_id,
            agent,
            status: AgentExecutionStatus::Pending,
            failure: None,
            aggregate_version: 1,
            requested_at,
            updated_at: requested_at,
            started_at: None,
            finished_at: None,
        };
        execution.validate()?;
        Ok(execution)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.requested_at = canonical_timestamp(self.requested_at);
        self.updated_at = canonical_timestamp(self.updated_at);
        self.started_at = self.started_at.map(canonical_timestamp);
        self.finished_at = self.finished_at.map(canonical_timestamp);
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
                } else if self.status == AgentExecutionStatus::Running {
                    self.transition(AgentExecutionStatus::Running, event.occurred_at)
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
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.agent.validate()?;
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
            || self.finished_at.is_some_and(|value| {
                value != canonical_timestamp(value) || value < self.requested_at
            })
            || (self.status == AgentExecutionStatus::Running && self.started_at.is_none())
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
            AgentExecutionStatus::Pending | AgentExecutionStatus::Running
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
}

fn validate_failure(reason: &str) -> Result<(), String> {
    if reason.is_empty() || reason.len() > 16 * 1024 || reason.contains(['\0', '\r', '\n']) {
        return Err("Agent execution failure reason is invalid".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        AssetId, AssetReleaseId, BuildRunId, Sha256Digest,
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
            super::AgentEventContent::inline_json(serde_json::json!({"text": "hello"}))
                .expect("content"),
            at,
        )
        .expect("output");
        execution.apply_event(&output).expect("apply output");
        assert_eq!(execution.status, AgentExecutionStatus::Running);

        let completed = AgentExecutionEventDraft::new(
            AgentExecutionEventKind::ExecutionCompleted,
            super::AgentEventContent::inline_json(serde_json::json!({})).expect("content"),
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
            super::AgentEventContent::inline_json(serde_json::json!({})).expect("content"),
            at,
        )
        .expect("completed");

        assert!(execution.apply_event(&completed).is_err());
        assert_eq!(execution, before);
    }
}
