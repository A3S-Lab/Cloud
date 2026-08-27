use super::schema::{
    AgentConversations, AgentExecutionChangeSets, AgentExecutionEvents, AgentExecutions,
};
use crate::modules::agents::domain::{
    AgentCodeRunBinding, AgentConversation, AgentConversationStatus, AgentEventContent,
    AgentExecution, AgentExecutionChangeSet, AgentExecutionEvent, AgentExecutionEventKind,
    AgentExecutionStatus, AgentProviderProfileBinding, AgentReleaseBinding,
};
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, AssetId, AssetReleaseId, BuildRunId, DeploymentId,
    EnvironmentId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError, Sha256Digest,
    WorkloadId, WorkloadReplicaId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    AgentProtocolChangeSetV1, AgentProtocolRunIdentityV1, AgentProviderRunStateV1,
    HarnessInvocationProfileV1,
};
use a3s_orm::expression::Selection;
use a3s_orm::{DecodeError, Expression, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

pub(super) struct ConversationSelection;
pub(super) struct ExecutionSelection;
pub(super) struct EventSelection;
pub(super) struct ChangeSetSelection;

impl Selection for ConversationSelection {
    type Output = ConversationRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AgentConversations::organization_id().expression(),
            AgentConversations::project_id().expression(),
            AgentConversations::environment_id().expression(),
            AgentConversations::id().expression(),
            AgentConversations::status().expression(),
            AgentConversations::last_event_sequence().expression(),
            AgentConversations::aggregate_version().expression(),
            AgentConversations::created_at().expression(),
            AgentConversations::updated_at().expression(),
            AgentConversations::closed_at().expression(),
        ]
    }
}

impl Selection for ExecutionSelection {
    type Output = ExecutionRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AgentExecutions::organization_id().expression(),
            AgentExecutions::conversation_id().expression(),
            AgentExecutions::id().expression(),
            AgentExecutions::operation_id().expression(),
            AgentExecutions::agent_asset_id().expression(),
            AgentExecutions::agent_asset_release_id().expression(),
            AgentExecutions::agent_build_run_id().expression(),
            AgentExecutions::agent_artifact_uri().expression(),
            AgentExecutions::agent_artifact_digest().expression(),
            AgentExecutions::agent_artifact_media_type().expression(),
            AgentExecutions::agent_artifact_size_bytes().expression(),
            AgentExecutions::status().expression(),
            AgentExecutions::failure().expression(),
            AgentExecutions::aggregate_version().expression(),
            AgentExecutions::requested_at().expression(),
            AgentExecutions::updated_at().expression(),
            AgentExecutions::started_at().expression(),
            AgentExecutions::cancellation_requested_at().expression(),
            AgentExecutions::finished_at().expression(),
            AgentExecutions::provider_kind().expression(),
            AgentExecutions::provider_revision().expression(),
            AgentExecutions::provider_protocol().expression(),
            AgentExecutions::provider_native_protocol().expression(),
            AgentExecutions::provider_profile_acl().expression(),
            AgentExecutions::provider_profile_digest().expression(),
            AgentExecutions::provider_capability_digest().expression(),
            AgentExecutions::provider_node_id().expression(),
            AgentExecutions::provider_workload_id().expression(),
            AgentExecutions::provider_workload_revision_id().expression(),
            AgentExecutions::provider_deployment_id().expression(),
            AgentExecutions::provider_replica_id().expression(),
            AgentExecutions::provider_runtime_unit_id().expression(),
            AgentExecutions::provider_runtime_generation().expression(),
            AgentExecutions::provider_runtime_spec_digest().expression(),
            AgentExecutions::provider_service_port_name().expression(),
            AgentExecutions::provider_release_identity().expression(),
            AgentExecutions::provider_session_id().expression(),
            AgentExecutions::provider_run_id().expression(),
            AgentExecutions::provider_event_cursor().expression(),
            AgentExecutions::provider_state().expression(),
            AgentExecutions::provider_bound_at().expression(),
            AgentExecutions::provider_observed_at().expression(),
            AgentExecutions::invocation_profile().expression(),
            AgentExecutions::invocation_profile_digest().expression(),
        ]
    }
}

impl Selection for EventSelection {
    type Output = EventRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AgentExecutionEvents::organization_id().expression(),
            AgentExecutionEvents::conversation_id().expression(),
            AgentExecutionEvents::sequence().expression(),
            AgentExecutionEvents::execution_id().expression(),
            AgentExecutionEvents::kind().expression(),
            AgentExecutionEvents::content().expression(),
            AgentExecutionEvents::content_digest().expression(),
            AgentExecutionEvents::content_size_bytes().expression(),
            AgentExecutionEvents::occurred_at().expression(),
        ]
    }
}

impl Selection for ChangeSetSelection {
    type Output = ChangeSetRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AgentExecutionChangeSets::organization_id().expression(),
            AgentExecutionChangeSets::execution_id().expression(),
            AgentExecutionChangeSets::batch_id().expression(),
            AgentExecutionChangeSets::node_id().expression(),
            AgentExecutionChangeSets::change_set().expression(),
            AgentExecutionChangeSets::recorded_at().expression(),
        ]
    }
}

pub(super) struct ConversationRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    status: String,
    last_event_sequence: u64,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

pub(super) struct ExecutionRow {
    organization_id: Uuid,
    conversation_id: Uuid,
    id: Uuid,
    operation_id: Uuid,
    agent_asset_id: Uuid,
    agent_asset_release_id: Uuid,
    agent_build_run_id: Uuid,
    agent_artifact_uri: String,
    agent_artifact_digest: String,
    agent_artifact_media_type: String,
    agent_artifact_size_bytes: u64,
    status: String,
    failure: Option<String>,
    aggregate_version: u64,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    cancellation_requested_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    provider_kind: Option<String>,
    provider_revision: Option<String>,
    provider_protocol: Option<String>,
    provider_native_protocol: Option<String>,
    provider_profile_acl: Option<String>,
    provider_profile_digest: Option<String>,
    provider_capability_digest: Option<String>,
    provider_node_id: Option<Uuid>,
    provider_workload_id: Option<Uuid>,
    provider_workload_revision_id: Option<Uuid>,
    provider_deployment_id: Option<Uuid>,
    provider_replica_id: Option<Uuid>,
    provider_runtime_unit_id: Option<String>,
    provider_runtime_generation: Option<u64>,
    provider_runtime_spec_digest: Option<String>,
    provider_service_port_name: Option<String>,
    provider_release_identity: Option<String>,
    provider_session_id: Option<String>,
    provider_run_id: Option<String>,
    provider_event_cursor: Option<u64>,
    provider_state: Option<String>,
    provider_bound_at: Option<DateTime<Utc>>,
    provider_observed_at: Option<DateTime<Utc>>,
    invocation_profile: Option<Value>,
    invocation_profile_digest: Option<String>,
}

pub(super) struct EventRow {
    organization_id: Uuid,
    conversation_id: Uuid,
    sequence: u64,
    execution_id: Uuid,
    kind: String,
    content: Value,
    content_digest: String,
    content_size_bytes: u64,
    occurred_at: DateTime<Utc>,
}

pub(super) struct ChangeSetRow {
    organization_id: Uuid,
    execution_id: Uuid,
    batch_id: Uuid,
    node_id: Uuid,
    change_set: Value,
    recorded_at: DateTime<Utc>,
}

macro_rules! from_row {
    ($row:ty, { $($field:ident: $index:literal),+ $(,)? }) => {
        impl FromRow for $row {
            fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
                Ok(Self { $($field: decode(row, $index)?,)+ })
            }
        }
    };
}

from_row!(ConversationRow, {
    organization_id: 0, project_id: 1, environment_id: 2, id: 3, status: 4,
    last_event_sequence: 5, aggregate_version: 6, created_at: 7, updated_at: 8,
    closed_at: 9,
});

from_row!(ExecutionRow, {
    organization_id: 0, conversation_id: 1, id: 2, operation_id: 3,
    agent_asset_id: 4, agent_asset_release_id: 5, agent_build_run_id: 6,
    agent_artifact_uri: 7, agent_artifact_digest: 8, agent_artifact_media_type: 9,
    agent_artifact_size_bytes: 10, status: 11, failure: 12, aggregate_version: 13,
    requested_at: 14, updated_at: 15, started_at: 16,
    cancellation_requested_at: 17, finished_at: 18, provider_kind: 19,
    provider_revision: 20, provider_protocol: 21, provider_native_protocol: 22,
    provider_profile_acl: 23, provider_profile_digest: 24,
    provider_capability_digest: 25, provider_node_id: 26, provider_workload_id: 27,
    provider_workload_revision_id: 28, provider_deployment_id: 29,
    provider_replica_id: 30, provider_runtime_unit_id: 31,
    provider_runtime_generation: 32, provider_runtime_spec_digest: 33,
    provider_service_port_name: 34, provider_release_identity: 35,
    provider_session_id: 36, provider_run_id: 37, provider_event_cursor: 38,
    provider_state: 39, provider_bound_at: 40, provider_observed_at: 41,
    invocation_profile: 42, invocation_profile_digest: 43,
});

from_row!(EventRow, {
    organization_id: 0, conversation_id: 1, sequence: 2, execution_id: 3,
    kind: 4, content: 5, content_digest: 6, content_size_bytes: 7, occurred_at: 8,
});

from_row!(ChangeSetRow, {
    organization_id: 0, execution_id: 1, batch_id: 2, node_id: 3,
    change_set: 4, recorded_at: 5,
});

impl ConversationRow {
    pub(super) fn aggregate(self) -> Result<AgentConversation, RepositoryError> {
        AgentConversation {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            id: AgentConversationId::from_uuid(self.id),
            status: AgentConversationStatus::parse(&self.status).map_err(|error| {
                corrupt(format!("Agent conversation status is invalid: {error}"))
            })?,
            last_event_sequence: self.last_event_sequence,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            closed_at: self.closed_at,
        }
        .restore()
        .map_err(|error| corrupt(format!("Agent conversation is invalid: {error}")))
    }
}

impl ExecutionRow {
    pub(super) fn aggregate(self) -> Result<AgentExecution, RepositoryError> {
        let provider = self.provider_binding()?;
        let code = self.code_binding(&provider)?;
        let digest = Sha256Digest::parse(self.agent_artifact_digest)
            .map_err(|error| corrupt(format!("Agent artifact digest is invalid: {error}")))?;
        let agent = AgentReleaseBinding::new(
            OrganizationId::from_uuid(self.organization_id),
            AssetId::from_uuid(self.agent_asset_id),
            AssetReleaseId::from_uuid(self.agent_asset_release_id),
            BuildRunId::from_uuid(self.agent_build_run_id),
            self.agent_artifact_uri,
            digest,
            self.agent_artifact_media_type,
            self.agent_artifact_size_bytes,
        )
        .map_err(|error| corrupt(format!("Agent release binding is invalid: {error}")))?;
        AgentExecution {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            conversation_id: AgentConversationId::from_uuid(self.conversation_id),
            id: AgentExecutionId::from_uuid(self.id),
            operation_id: OperationId::from_uuid(self.operation_id),
            agent,
            provider,
            code,
            status: AgentExecutionStatus::parse(&self.status)
                .map_err(|error| corrupt(format!("Agent execution status is invalid: {error}")))?,
            failure: self.failure,
            aggregate_version: self.aggregate_version,
            requested_at: self.requested_at,
            updated_at: self.updated_at,
            started_at: self.started_at,
            cancellation_requested_at: self.cancellation_requested_at,
            finished_at: self.finished_at,
        }
        .restore()
        .map_err(|error| corrupt(format!("Agent execution is invalid: {error}")))
    }

    fn provider_binding(&self) -> Result<AgentProviderProfileBinding, RepositoryError> {
        let required = (
            self.provider_kind.as_deref(),
            self.provider_revision.as_deref(),
            self.provider_protocol.as_deref(),
            self.provider_native_protocol.as_deref(),
            self.provider_profile_acl.as_deref(),
            self.provider_profile_digest.as_deref(),
            self.provider_capability_digest.as_deref(),
        );
        let (
            Some(kind),
            Some(revision),
            Some(protocol),
            Some(native_protocol),
            Some(profile_acl),
            Some(profile_digest),
            Some(capability_digest),
        ) = required
        else {
            return Err(corrupt("Agent execution has no immutable provider profile"));
        };
        AgentProviderProfileBinding::restore(
            kind.into(),
            revision.into(),
            protocol.into(),
            native_protocol.into(),
            profile_acl.into(),
            profile_digest.into(),
            capability_digest.into(),
        )
        .map_err(|error| corrupt(format!("Agent provider profile is invalid: {error}")))
    }

    fn code_binding(
        &self,
        provider: &AgentProviderProfileBinding,
    ) -> Result<Option<AgentCodeRunBinding>, RepositoryError> {
        let all_absent = self.provider_node_id.is_none()
            && self.provider_workload_id.is_none()
            && self.provider_workload_revision_id.is_none()
            && self.provider_deployment_id.is_none()
            && self.provider_replica_id.is_none()
            && self.provider_runtime_unit_id.is_none()
            && self.provider_runtime_generation.is_none()
            && self.provider_runtime_spec_digest.is_none()
            && self.provider_service_port_name.is_none()
            && self.provider_release_identity.is_none()
            && self.provider_session_id.is_none()
            && self.provider_run_id.is_none()
            && self.provider_event_cursor.is_none()
            && self.provider_state.is_none()
            && self.provider_bound_at.is_none()
            && self.provider_observed_at.is_none();
        let required = (
            self.provider_node_id,
            self.provider_workload_id,
            self.provider_workload_revision_id,
            self.provider_deployment_id,
            self.provider_replica_id,
            self.provider_runtime_unit_id.as_deref(),
            self.provider_runtime_generation,
            self.provider_runtime_spec_digest.as_deref(),
            self.provider_service_port_name.as_deref(),
            self.provider_release_identity.as_deref(),
            self.provider_session_id.as_deref(),
            self.provider_run_id.as_deref(),
            self.provider_state.as_deref(),
            self.provider_bound_at,
        );
        let (
            Some(node_id),
            Some(workload_id),
            Some(workload_revision_id),
            Some(deployment_id),
            Some(replica_id),
            Some(runtime_unit_id),
            Some(runtime_generation),
            Some(runtime_spec_digest),
            Some(service_port_name),
            Some(release_identity),
            Some(session_id),
            Some(run_id),
            Some(state),
            Some(bound_at),
        ) = required
        else {
            if all_absent {
                if self.invocation_profile.is_some() || self.invocation_profile_digest.is_some() {
                    return Err(corrupt(
                        "Agent invocation profile has no provider Runtime binding",
                    ));
                }
                return Ok(None);
            }
            return Err(corrupt("Agent provider run binding is incomplete"));
        };
        let digest = Sha256Digest::parse(runtime_spec_digest).map_err(|error| {
            corrupt(format!("provider Runtime spec digest is invalid: {error}"))
        })?;
        let state = parse_provider_state(state)?;
        let mut binding = AgentCodeRunBinding::restore_with_provider(
            provider.clone(),
            NodeId::from_uuid(node_id),
            WorkloadId::from_uuid(workload_id),
            WorkloadRevisionId::from_uuid(workload_revision_id),
            DeploymentId::from_uuid(deployment_id),
            WorkloadReplicaId::from_uuid(replica_id),
            runtime_unit_id,
            runtime_generation,
            digest,
            service_port_name,
            AgentProtocolRunIdentityV1 {
                schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: provider.native_protocol().into(),
                agent_release_identity: release_identity.into(),
                session_id: session_id.into(),
                run_id: run_id.into(),
            },
            self.provider_event_cursor,
            state,
            bound_at,
            self.provider_observed_at,
        )
        .map_err(|error| corrupt(format!("Agent Code run binding is invalid: {error}")))?;
        match (
            self.invocation_profile.clone(),
            self.invocation_profile_digest.as_deref(),
        ) {
            (None, None) => {}
            (Some(value), Some(stored_digest)) => {
                let profile: HarnessInvocationProfileV1 =
                    serde_json::from_value(value).map_err(|error| {
                        corrupt(format!("Harness invocation profile is invalid: {error}"))
                    })?;
                if profile.digest().map_err(|error| {
                    corrupt(format!("Harness invocation profile is invalid: {error}"))
                })? != stored_digest
                {
                    return Err(corrupt(
                        "Harness invocation profile changed its canonical digest",
                    ));
                }
                binding = binding
                    .restore_invocation_profile(profile)
                    .map_err(|error| {
                        corrupt(format!("Harness invocation profile is invalid: {error}"))
                    })?;
            }
            _ => return Err(corrupt("Harness invocation profile binding is incomplete")),
        }
        Ok(Some(binding))
    }
}

fn parse_provider_state(value: &str) -> Result<AgentProviderRunStateV1, RepositoryError> {
    AgentProviderRunStateV1::parse(value)
        .map_err(|error| corrupt(format!("Agent provider run state is invalid: {error}")))
}

impl EventRow {
    pub(super) fn event(self) -> Result<AgentExecutionEvent, RepositoryError> {
        let digest = Sha256Digest::parse(self.content_digest)
            .map_err(|error| corrupt(format!("Agent event content digest is invalid: {error}")))?;
        let content = AgentEventContent::restore(self.content, digest, self.content_size_bytes)
            .map_err(|error| corrupt(format!("Agent event content is invalid: {error}")))?;
        let event = AgentExecutionEvent {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            conversation_id: AgentConversationId::from_uuid(self.conversation_id),
            execution_id: AgentExecutionId::from_uuid(self.execution_id),
            sequence: self.sequence,
            kind: AgentExecutionEventKind::parse(&self.kind)
                .map_err(|error| corrupt(format!("Agent event kind is invalid: {error}")))?,
            content,
            occurred_at: self.occurred_at,
        };
        event
            .validate()
            .map_err(|error| corrupt(format!("Agent execution event is invalid: {error}")))?;
        Ok(event)
    }
}

impl ChangeSetRow {
    pub(super) fn change_set(self) -> Result<AgentExecutionChangeSet, RepositoryError> {
        let change_set: AgentProtocolChangeSetV1 = serde_json::from_value(self.change_set)
            .map_err(|error| corrupt(format!("A3S Code change set is invalid: {error}")))?;
        AgentExecutionChangeSet {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            execution_id: AgentExecutionId::from_uuid(self.execution_id),
            batch_id: self.batch_id,
            node_id: NodeId::from_uuid(self.node_id),
            change_set,
            recorded_at: self.recorded_at,
        }
        .restore()
        .map_err(|error| corrupt(format!("Agent execution change set is invalid: {error}")))
    }
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
