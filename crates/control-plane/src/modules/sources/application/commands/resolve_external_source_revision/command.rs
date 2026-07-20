use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::domain::ExternalSourceRevision;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DockerfileBuildRecipeInput {
    pub schema: String,
    pub kind: String,
    pub context_path: String,
    pub dockerfile_path: String,
    pub target: Option<String>,
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolveExternalSourceRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub repository_provider: String,
    pub repository_url: String,
    pub reference_kind: String,
    pub reference_value: String,
    pub recipe: DockerfileBuildRecipeInput,
    pub webhook_delivery_id: Option<String>,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub accepted_at: DateTime<Utc>,
}

impl Command for ResolveExternalSourceRevision {
    type Output = ApplicationResult<ResolveExternalSourceRevisionResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveExternalSourceRevisionResult {
    pub revision: ExternalSourceRevision,
    pub replayed: bool,
}
