use super::{AgentConversation, AgentExecution, AgentExecutionEvent, AgentExecutionEventKind};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, AgentConversationId, AgentExecutionCheckpointId,
    AgentExecutionId, DeploymentId, EnvironmentId, NodeId, OperationId, OrganizationId, ProjectId,
    Sha256Digest, WorkloadId, WorkloadReplicaId, WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const AGENT_EXECUTION_CHECKPOINT_SCHEMA: &str = "a3s.cloud.agent-execution-checkpoint.v1";
pub const AGENT_EXECUTION_CHECKPOINT_OBJECT_SCHEMA: &str =
    "a3s.cloud.agent-execution-checkpoint-object.v1";
pub const AGENT_EXECUTION_CHECKPOINT_NAMESPACE: &str = "agent-checkpoints";
pub const AGENT_EXECUTION_CHECKPOINT_MEDIA_TYPE: &str =
    "application/vnd.a3s.agent-execution-checkpoint+json;version=1";
/// Reserves 128 KiB of the provider prompt envelope for the fork input and
/// immutable lineage metadata.
pub const MAX_AGENT_EXECUTION_CHECKPOINT_BYTES: usize = 896 * 1024;
pub const MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS: usize = 1_000;

const CHECKPOINT_ID_DOMAIN: &str = "a3s.cloud.agent-execution-checkpoint-id.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentExecutionTelemetryCorrelation {
    pub operation_id: OperationId,
    pub provider_run_identity_digest: Sha256Digest,
    pub node_id: NodeId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub deployment_id: DeploymentId,
    pub replica_id: WorkloadReplicaId,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
}

impl AgentExecutionTelemetryCorrelation {
    pub fn from_execution(execution: &AgentExecution) -> Result<Self, String> {
        execution.validate()?;
        let binding = execution
            .code
            .as_ref()
            .ok_or_else(|| "Agent checkpoint requires a bound provider Runtime".to_owned())?;
        binding.require_invocation_profile()?;
        let correlation = Self {
            operation_id: execution.operation_id,
            provider_run_identity_digest: Sha256Digest::parse(
                binding.provider_identity()?.digest()?,
            )?,
            node_id: binding.node_id(),
            workload_id: binding.workload_id(),
            workload_revision_id: binding.workload_revision_id(),
            deployment_id: binding.deployment_id(),
            replica_id: binding.replica_id(),
            runtime_unit_id: binding.runtime_unit_id().to_owned(),
            runtime_generation: binding.runtime_generation(),
        };
        correlation.validate()?;
        Ok(correlation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.operation_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.deployment_id.as_uuid().is_nil()
            || self.replica_id.as_uuid().is_nil()
            || self.runtime_generation == 0
            || self.runtime_unit_id.trim().is_empty()
            || self.runtime_unit_id.trim() != self.runtime_unit_id
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
            || Sha256Digest::parse(self.provider_run_identity_digest.as_str())
                .ok()
                .as_ref()
                != Some(&self.provider_run_identity_digest)
        {
            return Err("Agent execution telemetry correlation is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentExecutionCheckpointEvent {
    pub sequence: u64,
    pub kind: AgentExecutionEventKind,
    pub content: Value,
    pub content_digest: Sha256Digest,
    pub content_size_bytes: u64,
    pub occurred_at: DateTime<Utc>,
}

impl AgentExecutionCheckpointEvent {
    fn from_event(event: &AgentExecutionEvent) -> Result<Self, String> {
        event.validate()?;
        Ok(Self {
            sequence: event.sequence,
            kind: event.kind,
            content: event.content.value().clone(),
            content_digest: event.content.digest().clone(),
            content_size_bytes: event.content.size_bytes(),
            occurred_at: event.occurred_at,
        })
    }

    fn validate(&self) -> Result<(), String> {
        super::AgentEventContent::restore(
            self.content.clone(),
            self.content_digest.clone(),
            self.content_size_bytes,
        )?;
        if self.sequence == 0 || self.occurred_at != canonical_timestamp(self.occurred_at) {
            return Err("Agent checkpoint event is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentExecutionCheckpointSnapshot {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub operation_id: OperationId,
    pub agent_artifact_digest: Sha256Digest,
    pub provider_profile_digest: Sha256Digest,
    pub invocation_profile_digest: Sha256Digest,
    pub through_event_sequence: u64,
    pub event_count: u16,
    pub telemetry_correlation: AgentExecutionTelemetryCorrelation,
    /// Self-contained semantic trajectory. Fork checkpoints prepend the exact
    /// verified trajectory inherited from their parent checkpoint.
    pub events: Vec<AgentExecutionCheckpointEvent>,
    pub captured_at: DateTime<Utc>,
}

impl AgentExecutionCheckpointSnapshot {
    pub fn capture(
        execution: &AgentExecution,
        events: &[AgentExecutionEvent],
    ) -> Result<Self, String> {
        if execution.lineage.is_some() {
            return Err(
                "forked Agent execution snapshots require their inherited trajectory".into(),
            );
        }
        Self::capture_with_inherited_trajectory(execution, &[], events)
    }

    fn capture_with_inherited_trajectory(
        execution: &AgentExecution,
        inherited_events: &[AgentExecutionCheckpointEvent],
        events: &[AgentExecutionEvent],
    ) -> Result<Self, String> {
        execution.validate()?;
        let event_count = inherited_events
            .len()
            .checked_add(events.len())
            .ok_or_else(|| "Agent checkpoint event count overflowed".to_owned())?;
        if events.is_empty() || event_count > MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS {
            return Err("Agent checkpoint event count is outside its bounded range".into());
        }
        if events.first().map(|event| event.kind)
            != Some(AgentExecutionEventKind::ExecutionRequested)
        {
            return Err("Agent checkpoint local trajectory must begin with its request".into());
        }
        let inherited_content_bytes = inherited_events.iter().try_fold(0_u64, |total, event| {
            total
                .checked_add(event.content_size_bytes)
                .ok_or_else(|| "Agent checkpoint content size overflowed".to_owned())
        })?;
        let content_bytes = events
            .iter()
            .try_fold(inherited_content_bytes, |total, event| {
                total
                    .checked_add(event.content.size_bytes())
                    .ok_or_else(|| "Agent checkpoint content size overflowed".to_owned())
            })?;
        if content_bytes
            > u64::try_from(MAX_AGENT_EXECUTION_CHECKPOINT_BYTES)
                .map_err(|_| "Agent checkpoint byte bound overflowed".to_owned())?
        {
            return Err(format!(
                "Agent checkpoint event content exceeds the {MAX_AGENT_EXECUTION_CHECKPOINT_BYTES}-byte snapshot bound"
            ));
        }
        let binding = execution
            .code
            .as_ref()
            .ok_or_else(|| "Agent checkpoint requires a bound provider Runtime".to_owned())?;
        let invocation_profile = binding.require_invocation_profile()?;
        let mut projected = inherited_events.to_vec();
        projected.extend(
            events
                .iter()
                .map(|event| {
                    if event.organization_id != execution.organization_id
                        || event.conversation_id != execution.conversation_id
                        || event.execution_id != execution.id
                    {
                        return Err("Agent checkpoint event falls outside its execution".into());
                    }
                    AgentExecutionCheckpointEvent::from_event(event)
                })
                .collect::<Result<Vec<_>, String>>()?,
        );
        let boundary = projected
            .last()
            .ok_or_else(|| "Agent checkpoint has no boundary event".to_owned())?;
        let through_event_sequence = boundary.sequence;
        let captured_at = boundary.occurred_at;
        let snapshot = Self {
            schema: AGENT_EXECUTION_CHECKPOINT_SCHEMA.into(),
            organization_id: execution.organization_id,
            conversation_id: execution.conversation_id,
            execution_id: execution.id,
            operation_id: execution.operation_id,
            agent_artifact_digest: execution.agent.artifact_digest().clone(),
            provider_profile_digest: Sha256Digest::parse(
                execution.provider.profile_digest().to_owned(),
            )?,
            invocation_profile_digest: Sha256Digest::parse(invocation_profile.digest()?)?,
            through_event_sequence,
            event_count: u16::try_from(projected.len())
                .map_err(|_| "Agent checkpoint event count overflowed".to_owned())?,
            telemetry_correlation: AgentExecutionTelemetryCorrelation::from_execution(execution)?,
            events: projected,
            captured_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.telemetry_correlation.validate()?;
        if self.schema != AGENT_EXECUTION_CHECKPOINT_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.conversation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.operation_id.as_uuid().is_nil()
            || self.telemetry_correlation.operation_id != self.operation_id
            || self.through_event_sequence == 0
            || self.events.is_empty()
            || self.events.len() > MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS
            || usize::from(self.event_count) != self.events.len()
            || self.captured_at != canonical_timestamp(self.captured_at)
            || self.events.first().map(|event| event.kind)
                != Some(AgentExecutionEventKind::ExecutionRequested)
            || self.events.last().is_none_or(|event| {
                event.sequence != self.through_event_sequence
                    || event.occurred_at != self.captured_at
            })
            || !self.events.windows(2).all(|pair| {
                pair[0].sequence < pair[1].sequence && pair[0].occurred_at <= pair[1].occurred_at
            })
        {
            return Err("Agent execution checkpoint snapshot is invalid".into());
        }
        for digest in [
            &self.agent_artifact_digest,
            &self.provider_profile_digest,
            &self.invocation_profile_digest,
        ] {
            if Sha256Digest::parse(digest.as_str()).ok().as_ref() != Some(digest) {
                return Err("Agent checkpoint digest binding is invalid".into());
            }
        }
        for event in &self.events {
            event.validate()?;
        }
        self.canonical_bytes().map(|_| ())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_json_bounded(
            self,
            MAX_AGENT_EXECUTION_CHECKPOINT_BYTES,
            "Agent execution checkpoint",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentExecutionCheckpointObjectReference {
    pub schema: String,
    pub namespace: String,
    pub object_ref: String,
    pub digest: Sha256Digest,
    pub size_bytes: u64,
    pub media_type: String,
}

impl AgentExecutionCheckpointObjectReference {
    pub fn from_inventory(object_ref: impl Into<String>, size_bytes: u64) -> Result<Self, String> {
        let object_ref = object_ref.into();
        let identity = checkpoint_object_identity(&object_ref)?;
        let reference = Self {
            schema: AGENT_EXECUTION_CHECKPOINT_OBJECT_SCHEMA.into(),
            namespace: AGENT_EXECUTION_CHECKPOINT_NAMESPACE.into(),
            object_ref,
            digest: identity.digest,
            size_bytes,
            media_type: AGENT_EXECUTION_CHECKPOINT_MEDIA_TYPE.into(),
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn identity(&self) -> Result<AgentExecutionCheckpointObjectIdentity, String> {
        self.validate()?;
        checkpoint_object_identity(&self.object_ref)
    }

    pub fn from_snapshot(
        checkpoint_id: AgentExecutionCheckpointId,
        snapshot: &AgentExecutionCheckpointSnapshot,
    ) -> Result<(Self, Vec<u8>), String> {
        let bytes = snapshot.canonical_bytes()?;
        let digest = Sha256Digest::from_bytes(&bytes);
        let hexadecimal = digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| "Agent checkpoint digest is invalid".to_owned())?;
        let reference = Self {
            schema: AGENT_EXECUTION_CHECKPOINT_OBJECT_SCHEMA.into(),
            namespace: AGENT_EXECUTION_CHECKPOINT_NAMESPACE.into(),
            object_ref: checkpoint_object_ref(
                snapshot.organization_id,
                snapshot.execution_id,
                checkpoint_id,
                hexadecimal,
            ),
            digest,
            size_bytes: u64::try_from(bytes.len())
                .map_err(|_| "Agent checkpoint size overflowed".to_owned())?,
            media_type: AGENT_EXECUTION_CHECKPOINT_MEDIA_TYPE.into(),
        };
        reference.validate()?;
        Ok((reference, bytes))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AGENT_EXECUTION_CHECKPOINT_OBJECT_SCHEMA
            || self.namespace != AGENT_EXECUTION_CHECKPOINT_NAMESPACE
            || self.object_ref.is_empty()
            || self.object_ref.len() > 4096
            || self.object_ref.contains(['\\', '\0', '\r', '\n'])
            || self.size_bytes == 0
            || self.size_bytes > MAX_AGENT_EXECUTION_CHECKPOINT_BYTES as u64
            || self.media_type != AGENT_EXECUTION_CHECKPOINT_MEDIA_TYPE
            || Sha256Digest::parse(self.digest.as_str()).ok().as_ref() != Some(&self.digest)
        {
            return Err("Agent execution checkpoint object reference is invalid".into());
        }
        let hexadecimal = self
            .digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| "Agent checkpoint digest is invalid".to_owned())?;
        let mut segments = self.object_ref.split('/');
        if segments.next() != Some("organizations")
            || segments
                .next()
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_none()
            || segments.next() != Some("executions")
            || segments
                .next()
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_none()
            || segments.next() != Some("checkpoints")
            || segments
                .next()
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_none()
            || segments.next() != Some("sha256")
            || segments.next() != Some(hexadecimal)
            || segments.next() != Some("checkpoint.json")
            || segments.next().is_some()
        {
            return Err(
                "Agent checkpoint object path changed its identity or digest binding".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionCheckpointObjectIdentity {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub checkpoint_id: AgentExecutionCheckpointId,
    pub digest: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionCheckpointObjectInventoryEntry {
    pub object_ref: String,
    pub size_bytes: u64,
}

impl AgentExecutionCheckpointObjectInventoryEntry {
    pub fn reference(&self) -> Result<AgentExecutionCheckpointObjectReference, String> {
        AgentExecutionCheckpointObjectReference::from_inventory(
            self.object_ref.clone(),
            self.size_bytes,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionCheckpointObjectInventoryPage {
    pub entries: Vec<AgentExecutionCheckpointObjectInventoryEntry>,
    pub next_after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentExecutionCheckpoint {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub id: AgentExecutionCheckpointId,
    pub through_event_sequence: u64,
    pub event_count: u16,
    pub agent_artifact_digest: Sha256Digest,
    pub provider_profile_digest: Sha256Digest,
    pub invocation_profile_digest: Sha256Digest,
    pub object: AgentExecutionCheckpointObjectReference,
    pub telemetry_correlation: AgentExecutionTelemetryCorrelation,
    pub aggregate_version: u64,
    pub captured_at: DateTime<Utc>,
}

impl AgentExecutionCheckpoint {
    pub fn capture(
        conversation: &AgentConversation,
        execution: &AgentExecution,
        events: &[AgentExecutionEvent],
    ) -> Result<CapturedAgentExecutionCheckpoint, String> {
        if execution.lineage.is_some() {
            return Err(
                "forked Agent execution checkpoints require their inherited trajectory".into(),
            );
        }
        Self::capture_materialized(conversation, execution, events, &[])
    }

    pub fn capture_with_parent(
        conversation: &AgentConversation,
        execution: &AgentExecution,
        events: &[AgentExecutionEvent],
        parent_checkpoint: &AgentExecutionCheckpoint,
        parent_snapshot: &AgentExecutionCheckpointSnapshot,
    ) -> Result<CapturedAgentExecutionCheckpoint, String> {
        execution.validate()?;
        parent_checkpoint.validate_snapshot(parent_snapshot)?;
        let lineage = execution.lineage.as_ref().ok_or_else(|| {
            "root Agent execution cannot inherit a checkpoint trajectory".to_owned()
        })?;
        let invocation_digest = execution
            .code
            .as_ref()
            .ok_or_else(|| "Agent checkpoint requires a bound provider Runtime".to_owned())?
            .require_invocation_profile()?
            .digest()?;
        if parent_checkpoint.organization_id != execution.organization_id
            || parent_checkpoint.conversation_id != execution.conversation_id
            || parent_checkpoint.execution_id != lineage.parent_execution_id
            || parent_checkpoint.id != lineage.parent_checkpoint_id
            || parent_checkpoint.object.digest != lineage.parent_checkpoint_digest
            || &parent_checkpoint.agent_artifact_digest != execution.agent.artifact_digest()
            || parent_checkpoint.provider_profile_digest.as_str()
                != execution.provider.profile_digest()
            || parent_checkpoint.invocation_profile_digest.as_str() != invocation_digest
        {
            return Err("Agent checkpoint inherited trajectory changed its fork lineage".into());
        }
        Self::capture_materialized(conversation, execution, events, &parent_snapshot.events)
    }

    fn capture_materialized(
        conversation: &AgentConversation,
        execution: &AgentExecution,
        events: &[AgentExecutionEvent],
        inherited_events: &[AgentExecutionCheckpointEvent],
    ) -> Result<CapturedAgentExecutionCheckpoint, String> {
        conversation.validate()?;
        execution.validate()?;
        if conversation.organization_id != execution.organization_id
            || conversation.id != execution.conversation_id
        {
            return Err("Agent checkpoint conversation changed its execution authority".into());
        }
        let snapshot = AgentExecutionCheckpointSnapshot::capture_with_inherited_trajectory(
            execution,
            inherited_events,
            events,
        )?;
        let id = Self::derive_id(execution.id, snapshot.through_event_sequence);
        let (object, bytes) =
            AgentExecutionCheckpointObjectReference::from_snapshot(id, &snapshot)?;
        let checkpoint = Self {
            organization_id: execution.organization_id,
            project_id: conversation.project_id,
            environment_id: conversation.environment_id,
            conversation_id: execution.conversation_id,
            execution_id: execution.id,
            id,
            through_event_sequence: snapshot.through_event_sequence,
            event_count: snapshot.event_count,
            agent_artifact_digest: snapshot.agent_artifact_digest.clone(),
            provider_profile_digest: snapshot.provider_profile_digest.clone(),
            invocation_profile_digest: snapshot.invocation_profile_digest.clone(),
            object,
            telemetry_correlation: snapshot.telemetry_correlation.clone(),
            aggregate_version: 1,
            captured_at: snapshot.captured_at,
        };
        checkpoint.validate()?;
        checkpoint.validate_snapshot(&snapshot)?;
        Ok(CapturedAgentExecutionCheckpoint {
            checkpoint,
            snapshot,
            bytes,
        })
    }

    pub fn derive_id(
        execution_id: AgentExecutionId,
        through_event_sequence: u64,
    ) -> AgentExecutionCheckpointId {
        AgentExecutionCheckpointId::from_uuid(Uuid::new_v5(
            &execution_id.as_uuid(),
            format!("{CHECKPOINT_ID_DOMAIN}:{through_event_sequence}").as_bytes(),
        ))
    }

    pub fn validate(&self) -> Result<(), String> {
        self.object.validate()?;
        self.telemetry_correlation.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.conversation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.id != Self::derive_id(self.execution_id, self.through_event_sequence)
            || self.through_event_sequence == 0
            || self.event_count == 0
            || usize::from(self.event_count) > MAX_AGENT_EXECUTION_CHECKPOINT_EVENTS
            || self.aggregate_version != 1
            || self.captured_at != canonical_timestamp(self.captured_at)
            || self.telemetry_correlation.operation_id.as_uuid().is_nil()
        {
            return Err("Agent execution checkpoint projection is invalid".into());
        }
        for digest in [
            &self.agent_artifact_digest,
            &self.provider_profile_digest,
            &self.invocation_profile_digest,
        ] {
            if Sha256Digest::parse(digest.as_str()).ok().as_ref() != Some(digest) {
                return Err("Agent checkpoint projection digest is invalid".into());
            }
        }
        let hexadecimal = self
            .object
            .digest
            .as_str()
            .strip_prefix("sha256:")
            .ok_or_else(|| "Agent checkpoint object digest is invalid".to_owned())?;
        if self.object.object_ref
            != checkpoint_object_ref(
                self.organization_id,
                self.execution_id,
                self.id,
                hexadecimal,
            )
        {
            return Err(
                "Agent checkpoint object path changed its tenant or execution binding".into(),
            );
        }
        Ok(())
    }

    pub fn validate_snapshot(
        &self,
        snapshot: &AgentExecutionCheckpointSnapshot,
    ) -> Result<(), String> {
        self.validate()?;
        snapshot.validate()?;
        let bytes = snapshot.canonical_bytes()?;
        if snapshot.organization_id != self.organization_id
            || snapshot.conversation_id != self.conversation_id
            || snapshot.execution_id != self.execution_id
            || snapshot.through_event_sequence != self.through_event_sequence
            || snapshot.event_count != self.event_count
            || snapshot.agent_artifact_digest != self.agent_artifact_digest
            || snapshot.provider_profile_digest != self.provider_profile_digest
            || snapshot.invocation_profile_digest != self.invocation_profile_digest
            || snapshot.telemetry_correlation != self.telemetry_correlation
            || snapshot.captured_at != self.captured_at
            || Sha256Digest::from_bytes(&bytes) != self.object.digest
            || u64::try_from(bytes.len())
                .map_err(|_| "Agent checkpoint size overflowed".to_owned())?
                != self.object.size_bytes
        {
            return Err("Agent checkpoint object changed its committed projection".into());
        }
        Ok(())
    }
}

fn checkpoint_object_ref(
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
    checkpoint_id: AgentExecutionCheckpointId,
    digest_hexadecimal: &str,
) -> String {
    format!(
        "organizations/{organization_id}/executions/{execution_id}/checkpoints/{checkpoint_id}/sha256/{digest_hexadecimal}/checkpoint.json"
    )
}

fn checkpoint_object_identity(
    object_ref: &str,
) -> Result<AgentExecutionCheckpointObjectIdentity, String> {
    let mut segments = object_ref.split('/');
    if segments.next() != Some("organizations") {
        return Err("Agent checkpoint object path has no organization identity".into());
    }
    let organization_id = segments
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .map(OrganizationId::from_uuid)
        .ok_or_else(|| {
            "Agent checkpoint object path has an invalid organization identity".to_owned()
        })?;
    if segments.next() != Some("executions") {
        return Err("Agent checkpoint object path has no execution identity".into());
    }
    let execution_id = segments
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .map(AgentExecutionId::from_uuid)
        .ok_or_else(|| {
            "Agent checkpoint object path has an invalid execution identity".to_owned()
        })?;
    if segments.next() != Some("checkpoints") {
        return Err("Agent checkpoint object path has no checkpoint identity".into());
    }
    let checkpoint_id = segments
        .next()
        .and_then(|value| Uuid::parse_str(value).ok())
        .map(AgentExecutionCheckpointId::from_uuid)
        .ok_or_else(|| {
            "Agent checkpoint object path has an invalid checkpoint identity".to_owned()
        })?;
    if segments.next() != Some("sha256") {
        return Err("Agent checkpoint object path has no digest algorithm".into());
    }
    let digest = segments
        .next()
        .map(|value| format!("sha256:{value}"))
        .ok_or_else(|| "Agent checkpoint object path has no digest".to_owned())?;
    if segments.next() != Some("checkpoint.json") || segments.next().is_some() {
        return Err("Agent checkpoint object path has an invalid checkpoint suffix".into());
    }
    Ok(AgentExecutionCheckpointObjectIdentity {
        organization_id,
        execution_id,
        checkpoint_id,
        digest: Sha256Digest::parse(digest)?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAgentExecutionCheckpoint {
    pub checkpoint: AgentExecutionCheckpoint,
    pub snapshot: AgentExecutionCheckpointSnapshot,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionCheckpointObjectWrite {
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentExecutionCheckpointObjectError {
    #[error("Agent checkpoint object request is invalid: {0}")]
    Invalid(String),
    #[error("Agent checkpoint object conflicts with existing content: {0}")]
    Conflict(String),
    #[error("Agent checkpoint object was not found")]
    NotFound,
    #[error("Agent checkpoint object failed integrity validation: {0}")]
    Integrity(String),
    #[error("Agent checkpoint object storage is unavailable: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait IAgentExecutionCheckpointObjectStore: Send + Sync {
    async fn put(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
        body: Vec<u8>,
    ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError>;

    async fn get(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<Vec<u8>, AgentExecutionCheckpointObjectError>;

    async fn inventory_page(
        &self,
        _after: Option<&str>,
        _limit: usize,
    ) -> Result<AgentExecutionCheckpointObjectInventoryPage, AgentExecutionCheckpointObjectError>
    {
        Err(AgentExecutionCheckpointObjectError::Unavailable(
            "Agent checkpoint object inventory is unsupported by this adapter".into(),
        ))
    }

    async fn remove(
        &self,
        _reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<(), AgentExecutionCheckpointObjectError> {
        Err(AgentExecutionCheckpointObjectError::Unavailable(
            "Agent checkpoint object cleanup is unsupported by this adapter".into(),
        ))
    }
}
