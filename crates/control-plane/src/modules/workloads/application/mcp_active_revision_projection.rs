use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError, Sha256Digest, WorkloadId,
};
use crate::modules::workloads::domain::entities::{
    Workload, WorkloadDesiredState, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::published::{
    ActiveMcpWorkloadRevisionProjection, ValidatedActiveMcpWorkloadRevisionProjection,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Exact MCP profile identity accepted by the Workloads owner boundary when
/// materializing an active revision projection for Gateway compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadMcpActiveRevisionProjectionQuery {
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    profile_digest: Sha256Digest,
    runtime_port: String,
    health_path: String,
}

impl WorkloadMcpActiveRevisionProjectionQuery {
    pub fn new(
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        profile_digest: Sha256Digest,
        runtime_port: impl Into<String>,
        health_path: impl Into<String>,
    ) -> Result<Self, String> {
        let query = Self {
            organization_id,
            workload_id,
            asset_id,
            asset_release_id,
            profile_digest,
            runtime_port: runtime_port.into(),
            health_path: health_path.into(),
        };
        query.validate()?;
        Ok(query)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.runtime_port.is_empty()
            || self.health_path.is_empty()
        {
            return Err("Workload MCP active revision projection query is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub fn profile_digest(&self) -> &Sha256Digest {
        &self.profile_digest
    }

    pub fn runtime_port(&self) -> &str {
        &self.runtime_port
    }

    pub fn health_path(&self) -> &str {
        &self.health_path
    }
}

#[async_trait]
pub trait IWorkloadMcpActiveRevisionProjectionQueryPort: Send + Sync {
    async fn find_active_projection(
        &self,
        query: WorkloadMcpActiveRevisionProjectionQuery,
    ) -> Result<ActiveMcpWorkloadRevisionProjection, RepositoryError>;
}

/// Workloads owner-side policy service. It is the sole component that
/// interprets running Workload authority, active revision MCP release bindings,
/// and template port/health facts for Gateway projection.
pub struct WorkloadMcpActiveRevisionProjectionQueryService {
    workloads: Arc<dyn IWorkloadRepository>,
}

impl WorkloadMcpActiveRevisionProjectionQueryService {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
        Self { workloads }
    }
}

#[async_trait]
impl IWorkloadMcpActiveRevisionProjectionQueryPort
    for WorkloadMcpActiveRevisionProjectionQueryService
{
    async fn find_active_projection(
        &self,
        query: WorkloadMcpActiveRevisionProjectionQuery,
    ) -> Result<ActiveMcpWorkloadRevisionProjection, RepositoryError> {
        query
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let workload = self
            .workloads
            .find_workload(query.organization_id(), query.workload_id())
            .await
            .map_err(|error| {
                missing_as_storage(
                    error,
                    "active MCP route policy lost its referenced Workload",
                )
            })?;
        let revision_id = workload.active_revision_id.ok_or_else(|| {
            RepositoryError::Conflict(
                "active MCP route policy Workload has no active revision".into(),
            )
        })?;
        let revision = self
            .workloads
            .find_revision(query.organization_id(), revision_id)
            .await
            .map_err(|error| {
                missing_as_storage(error, "active MCP Workload lost its active revision")
            })?;
        admit_active_mcp_workload_revision_projection(&workload, &revision, &query)
            .map_err(RepositoryError::Conflict)
    }
}

fn admit_active_mcp_workload_revision_projection(
    workload: &Workload,
    revision: &WorkloadRevision,
    query: &WorkloadMcpActiveRevisionProjectionQuery,
) -> Result<ActiveMcpWorkloadRevisionProjection, String> {
    query.validate()?;
    if workload.id.as_uuid().is_nil()
        || workload.organization_id.as_uuid().is_nil()
        || workload.project_id.as_uuid().is_nil()
        || workload.environment_id.as_uuid().is_nil()
        || workload.desired_state != WorkloadDesiredState::Running
        || workload.aggregate_version == 0
        || workload.active_revision_id != Some(revision.id)
        || revision.workload_id != workload.id
        || revision.id.as_uuid().is_nil()
        || revision.generation == 0
    {
        return Err("active MCP route does not resolve to its running Workload revision".into());
    }
    let binding = revision
        .mcp_binding()
        .ok_or_else(|| "active MCP route Workload revision is not release-bound".to_owned())?;
    let template = revision.resolved_template()?;
    template.validate()?;
    if !template
        .ports
        .iter()
        .any(|port| port.name == query.runtime_port())
    {
        return Err("MCP Workload does not declare the bound profile Runtime port".into());
    }
    let health = template
        .health
        .as_ref()
        .ok_or_else(|| "MCP Workload requires the bound profile HTTP health check".to_owned())?;
    if health.port_name != query.runtime_port() || health.path != query.health_path() {
        return Err("MCP Workload health check differs from the bound Service profile".into());
    }
    if binding.organization_id() != workload.organization_id
        || binding.organization_id() != query.organization_id()
        || binding.asset_id() != query.asset_id()
        || binding.asset_release_id() != query.asset_release_id()
        || binding.profile_digest() != query.profile_digest()
    {
        return Err(
            "active MCP route Workload release binding differs from its Service profile".into(),
        );
    }

    ActiveMcpWorkloadRevisionProjection::from_validated(
        ValidatedActiveMcpWorkloadRevisionProjection {
            revision_id: revision.id,
            workload_id: revision.workload_id,
            generation: revision.generation,
            created_at: revision.created_at,
            organization_id: binding.organization_id(),
            asset_id: binding.asset_id(),
            asset_release_id: binding.asset_release_id(),
            profile_digest: binding.profile_digest().clone(),
            runtime_port: query.runtime_port().to_owned(),
            health_port_name: health.port_name.clone(),
            health_path: health.path.clone(),
            workload_aggregate_version: workload.aggregate_version,
            workload_updated_at: workload.updated_at,
        },
    )
}

fn missing_as_storage(error: RepositoryError, message: &str) -> RepositoryError {
    match error {
        RepositoryError::NotFound => RepositoryError::Storage(message.into()),
        error => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, ProjectId, ResourceName, WorkloadRevisionId,
    };
    use crate::modules::workloads::domain::entities::{
        HttpHealthCheck, McpWorkloadRevisionBinding, OciArtifact, ServicePort, ServiceProcess,
        ServiceResources, ServiceTemplate,
    };
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn uuid(value: &str) -> Uuid {
        Uuid::parse_str(value).expect("UUID")
    }

    #[test]
    fn owner_admission_accepts_running_mcp_revision_matching_profile() {
        let organization_id =
            OrganizationId::from_uuid(uuid("11111111-1111-4111-8111-111111111111"));
        let workload_id = WorkloadId::from_uuid(uuid("22222222-2222-4222-8222-222222222222"));
        let asset_id = AssetId::from_uuid(uuid("33333333-3333-4333-8333-333333333333"));
        let asset_release_id =
            AssetReleaseId::from_uuid(uuid("44444444-4444-4444-8444-444444444444"));
        let revision_id =
            WorkloadRevisionId::from_uuid(uuid("55555555-5555-4555-8555-555555555555"));
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).single().unwrap();
        let digest = Sha256Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap();
        let mut revision = WorkloadRevision::create(
            revision_id,
            workload_id,
            7,
            ServiceTemplate {
                artifact: OciArtifact {
                    uri: format!("oci://registry.example/mcp@{}", digest.as_str()),
                    digest: digest.as_str().to_owned(),
                    media_type: "application/vnd.oci.image.manifest.v1+json".into(),
                },
                process: ServiceProcess {
                    command: vec!["/app/mcp".into()],
                    args: vec!["serve".into()],
                    working_directory: Some("/app".into()),
                    environment: BTreeMap::new(),
                },
                secrets: Vec::new(),
                resources: ServiceResources {
                    cpu_millis: 500,
                    memory_bytes: 256 * 1024 * 1024,
                    pids: 128,
                    ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
                },
                ports: vec![ServicePort {
                    name: "mcp".into(),
                    container_port: 8080,
                }],
                health: Some(HttpHealthCheck {
                    port_name: "mcp".into(),
                    path: "/health".into(),
                    interval_ms: 10_000,
                    timeout_ms: 2_000,
                    healthy_threshold: 1,
                    unhealthy_threshold: 3,
                    stabilization_window_ms: 30_000,
                }),
            },
            now,
        )
        .expect("revision");
        revision
            .restore_mcp_binding(
                McpWorkloadRevisionBinding::restore(
                    organization_id,
                    asset_id,
                    asset_release_id,
                    digest.clone(),
                    "mcp",
                    "/health",
                )
                .expect("binding"),
            )
            .expect("restore");
        let mut workload = Workload::create(
            workload_id,
            organization_id,
            ProjectId::from_uuid(uuid("77777777-7777-4777-8777-777777777777")),
            EnvironmentId::from_uuid(uuid("88888888-8888-4888-8888-888888888888")),
            ResourceName::parse("MCP runtime").expect("name"),
            now,
        );
        workload.activate(revision_id, now).expect("activate");
        let query = WorkloadMcpActiveRevisionProjectionQuery::new(
            organization_id,
            workload_id,
            asset_id,
            asset_release_id,
            digest,
            "mcp",
            "/health",
        )
        .expect("query");
        let projection =
            admit_active_mcp_workload_revision_projection(&workload, &revision, &query)
                .expect("projection");
        assert_eq!(projection.revision_id(), revision_id);
        assert_eq!(projection.workload_aggregate_version(), 2);
        assert_eq!(projection.runtime_port(), "mcp");
    }
}
