use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodePoolId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::workloads::domain::entities::{
    AgentReleaseAdmission, HttpHealthCheck, OciArtifact, SecretBinding, ServicePort,
    ServiceProcess, ServiceResources, ServiceTemplate,
};
use crate::modules::workloads::domain::repositories::DeploymentBundle;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceWorkloadTemplate {
    pub process: ServiceProcess,
    pub secrets: Vec<SecretBinding>,
    pub resources: ServiceResources,
    pub ports: Vec<ServicePort>,
    pub health: Option<HttpHealthCheck>,
}

impl SourceWorkloadTemplate {
    pub fn resolve(self, artifact: OciArtifact) -> ServiceTemplate {
        ServiceTemplate {
            artifact,
            process: self.process,
            secrets: self.secrets,
            resources: self.resources,
            ports: self.ports,
            health: self.health,
        }
    }

    pub fn resolve_agent(
        self,
        admission: &AgentReleaseAdmission,
    ) -> Result<ServiceTemplate, String> {
        if !self.process.command.is_empty()
            || !self.process.args.is_empty()
            || self.process.working_directory.is_some()
            || !self.process.environment.is_empty()
            || !self.ports.is_empty()
            || self.health.is_some()
        {
            return Err(
                "Agent process, ports, and health are derived from its release manifest".into(),
            );
        }
        admission.resolve_template(self.secrets, self.resources)
    }
}

pub struct CreateSourceWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub name: String,
    pub node_pool_id: Option<NodePoolId>,
    pub template: SourceWorkloadTemplate,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateSourceWorkloadDeployment {
    type Output = crate::modules::shared_kernel::application::ApplicationResult<
        CreateSourceWorkloadDeploymentResult,
    >;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateSourceWorkloadDeploymentResult {
    pub bundle: DeploymentBundle,
}
