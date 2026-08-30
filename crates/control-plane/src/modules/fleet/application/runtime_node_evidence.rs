use crate::modules::fleet::domain::entities::{Node, NodePool};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodePoolRepository, INodeRepository, RuntimeObservationRecord,
};
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::fleet::published::{
    RuntimeNodeEvidence, ValidatedRuntimeNodeEvidenceProjection,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, NodeId, NodePoolId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeNodeEvidenceQuery {
    organization_id: OrganizationId,
    node_pool_id: NodePoolId,
    node_id: NodeId,
    runtime_unit_id: String,
    runtime_generation: u64,
    evaluated_at: DateTime<Utc>,
}

impl RuntimeNodeEvidenceQuery {
    pub fn new(
        organization_id: OrganizationId,
        node_pool_id: NodePoolId,
        node_id: NodeId,
        runtime_unit_id: impl Into<String>,
        runtime_generation: u64,
        evaluated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            node_pool_id,
            node_id,
            runtime_unit_id: runtime_unit_id.into(),
            runtime_generation,
            evaluated_at: canonical_timestamp(evaluated_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.node_pool_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.runtime_generation == 0
            || self.runtime_unit_id.is_empty()
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
            || self.runtime_unit_id.trim() != self.runtime_unit_id
            || self.evaluated_at != canonical_timestamp(self.evaluated_at)
        {
            return Err("Runtime Node evidence query identity is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IRuntimeNodeEvidenceQueryPort: Send + Sync {
    async fn find_runtime_node_evidence(
        &self,
        query: RuntimeNodeEvidenceQuery,
    ) -> Result<Option<RuntimeNodeEvidence>, RepositoryError>;
}

/// Fleet owner-side query service. Only this service interprets current pool
/// membership, Node lifecycle, Agent session, and observation persistence.
pub struct RuntimeNodeEvidenceQueryService {
    node_pools: Arc<dyn INodePoolRepository>,
    nodes: Arc<dyn INodeRepository>,
    node_control: Arc<dyn INodeControlRepository>,
}

impl RuntimeNodeEvidenceQueryService {
    pub fn new<R>(repository: Arc<R>) -> Self
    where
        R: INodePoolRepository + INodeRepository + INodeControlRepository + 'static,
    {
        Self {
            node_pools: repository.clone(),
            nodes: repository.clone(),
            node_control: repository,
        }
    }

    async fn require_stable_owner_snapshot(
        &self,
        query: &RuntimeNodeEvidenceQuery,
        pool: &NodePool,
        node: &Node,
        record: &RuntimeObservationRecord,
    ) -> Result<(), RepositoryError> {
        let current_pool = self
            .node_pools
            .find(query.organization_id, query.node_pool_id)
            .await
            .map_err(|error| concurrent_projection_error("NodePool", error))?;
        let current_node = self
            .nodes
            .find(query.organization_id, query.node_id)
            .await
            .map_err(|error| concurrent_projection_error("Node", error))?;
        let current_record = self
            .node_control
            .latest_runtime_observation(
                query.node_id,
                &query.runtime_unit_id,
                query.runtime_generation,
            )
            .await
            .map_err(|error| concurrent_projection_error("Runtime observation", error))?
            .ok_or_else(owner_snapshot_changed)?;
        if current_pool != *pool || current_node != *node || current_record != *record {
            return Err(owner_snapshot_changed());
        }
        Ok(())
    }
}

#[async_trait]
impl IRuntimeNodeEvidenceQueryPort for RuntimeNodeEvidenceQueryService {
    async fn find_runtime_node_evidence(
        &self,
        query: RuntimeNodeEvidenceQuery,
    ) -> Result<Option<RuntimeNodeEvidence>, RepositoryError> {
        query.validate().map_err(RepositoryError::Conflict)?;
        let pool = match self
            .node_pools
            .find(query.organization_id, query.node_pool_id)
            .await
        {
            Ok(pool) => pool,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        pool.validate().map_err(owner_projection_error)?;
        let node = match self.nodes.find(query.organization_id, query.node_id).await {
            Ok(node) => node,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        if pool.organization_id != query.organization_id
            || pool.id != query.node_pool_id
            || node.organization_id != query.organization_id
            || node.id != query.node_id
        {
            return Err(owner_projection_error(
                "Fleet repositories substituted a requested owner identity".into(),
            ));
        }
        if pool.member_node_ids.binary_search(&node.id).is_err()
            || pool.member_removal(node.id).is_some()
            || pool.node_is_in_active_maintenance(node.id, query.evaluated_at)
            || node.state != NodeState::Ready
        {
            return Ok(None);
        }
        let record = match self
            .node_control
            .latest_runtime_observation(node.id, &query.runtime_unit_id, query.runtime_generation)
            .await?
        {
            Some(record) => record,
            None => return Ok(None),
        };
        if record.node_id != node.id || record.agent_instance_id != node.agent_instance_id {
            return Ok(None);
        }
        let runtime_capabilities: a3s_runtime::contract::RuntimeCapabilities =
            serde_json::from_value(node.capabilities.document().clone()).map_err(|error| {
                owner_projection_error(format!(
                    "Node capabilities are not a Runtime contract: {error}"
                ))
            })?;
        if runtime_capabilities.provider_id.as_str() != node.capabilities.provider_id()
            || runtime_capabilities.provider_build != node.capabilities.provider_build()
        {
            return Err(owner_projection_error(
                "Node capability envelope drifted from its Runtime contract".into(),
            ));
        }
        self.require_stable_owner_snapshot(&query, &pool, &node, &record)
            .await?;

        RuntimeNodeEvidence::from_validated_node(ValidatedRuntimeNodeEvidenceProjection {
            organization_id: query.organization_id,
            node_pool_id: pool.id,
            node_pool_aggregate_version: pool.aggregate_version,
            node_pool_spec_digest: pool.spec_digest,
            node_id: node.id,
            node_aggregate_version: node.aggregate_version,
            agent_instance_id: node.agent_instance_id,
            node_capabilities_digest: node.capabilities.digest().into(),
            node_last_observed_at: node.last_observed_at,
            runtime_capabilities,
            runtime_report_id: record.report_id,
            runtime_observed_at: record.observed_at,
            runtime_received_at: record.received_at,
            runtime_observation: record.observation,
        })
        .map(Some)
        .map_err(owner_projection_error)
    }
}

fn concurrent_projection_error(label: &str, error: RepositoryError) -> RepositoryError {
    match error {
        RepositoryError::NotFound | RepositoryError::Conflict(_) => RepositoryError::Conflict(
            format!("Fleet {label} changed during Runtime evidence projection"),
        ),
        error => error,
    }
}

fn owner_snapshot_changed() -> RepositoryError {
    RepositoryError::Conflict("Fleet owner state changed during Runtime evidence projection".into())
}

fn owner_projection_error(error: String) -> RepositoryError {
    RepositoryError::Storage(format!("invalid Fleet Runtime Node projection: {error}"))
}
