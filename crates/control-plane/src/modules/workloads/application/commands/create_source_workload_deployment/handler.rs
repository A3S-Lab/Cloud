use super::{CreateSourceWorkloadDeployment, CreateSourceWorkloadDeploymentResult};
use crate::modules::artifacts::{BuildRunStatus, IBuildRunRepository};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotencyRequest, OperationId, RepositoryError, ResourceName, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::sources::domain::ISourceRevisionRepository;
use crate::modules::workloads::application::commands::validate_secret_bindings;
use crate::modules::workloads::domain::entities::{
    Deployment, ExternalBuildReference, OciArtifact, Workload, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateSourceWorkloadDeploymentHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    sources: Arc<dyn ISourceRevisionRepository>,
    builds: Arc<dyn IBuildRunRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    secrets: Arc<dyn ISecretRepository>,
}

impl CreateSourceWorkloadDeploymentHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        sources: Arc<dyn ISourceRevisionRepository>,
        builds: Arc<dyn IBuildRunRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
    ) -> Self {
        Self {
            environments,
            sources,
            builds,
            workloads,
            secrets,
        }
    }
}

impl CommandHandler<CreateSourceWorkloadDeployment> for CreateSourceWorkloadDeploymentHandler {
    fn execute(
        &self,
        command: CreateSourceWorkloadDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CreateSourceWorkloadDeploymentResult>>,
    > {
        let environments = Arc::clone(&self.environments);
        let sources = Arc::clone(&self.sources);
        let builds = Arc::clone(&self.builds);
        let workloads = Arc::clone(&self.workloads);
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let source = match sources
                .find(command.organization_id, command.source_revision_id)
                .await
            {
                Ok(source)
                    if source.organization_id == command.organization_id
                        && source.project_id == command.project_id
                        && source.environment_id == command.environment_id
                        && source.id == command.source_revision_id =>
                {
                    source
                }
                Ok(_) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "source revision not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let build = match builds
                .find_by_source_revision(command.organization_id, source.id)
                .await
            {
                Ok(Some(build))
                    if build.organization_id == command.organization_id
                        && build.project_id == command.project_id
                        && build.environment_id == command.environment_id
                        && build.source_revision_id == source.id =>
                {
                    build
                }
                Ok(Some(_)) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "source revision build not found".into(),
                    )))
                }
                Ok(None) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "source revision build is not ready for deployment".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if build.status != BuildRunStatus::Succeeded {
                return Ok(Err(ApplicationError::Conflict(
                    "source revision build has not succeeded".into(),
                )));
            }
            let published = match build.published_artifact.as_ref() {
                Some(published) => published,
                None => {
                    return Ok(Err(ApplicationError::Internal(
                        "successful source revision build omitted its published OCI artifact"
                            .into(),
                    )))
                }
            };
            let name = match ResourceName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "environmentId": command.environment_id,
                "sourceRevisionId": source.id,
                "buildRunId": build.id,
                "publishedArtifactDigest": published.digest,
                "name": name.as_str(),
                "template": &command.template,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/source-revisions/{}/workloads",
                    command.organization_id, command.project_id, command.environment_id, source.id,
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let artifact = OciArtifact {
                uri: published.uri.clone(),
                digest: published.digest.clone(),
                media_type: published.media_type.clone(),
            };
            let workload = Workload::create(
                WorkloadId::new(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                name,
                command.requested_at,
            );
            let revision = match WorkloadRevision::create_from_external_build(
                WorkloadRevisionId::new(),
                workload.id,
                1,
                command.template.resolve(artifact),
                ExternalBuildReference {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                    environment_id: command.environment_id,
                    source_revision_id: source.id,
                    build_run_id: build.id,
                },
                command.requested_at,
            ) {
                Ok(revision) => revision,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Err(error) = validate_secret_bindings(
                secrets.as_ref(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                &revision.request,
            )
            .await
            {
                return Ok(Err(error));
            }
            let deployment = Deployment::create(
                DeploymentId::new(),
                workload.organization_id,
                workload.id,
                revision.id,
                OperationId::new(),
                command.requested_at,
            );
            let operation = OperationRequest::new(
                deployment.operation_id,
                workload.organization_id,
                OperationSubject::new("deployment", deployment.id.as_uuid())
                    .map_err(BootError::Internal)?,
                WorkflowIdentity::new("cloud.deployment", "2").map_err(BootError::Internal)?,
                serde_json::json!({
                    "deploymentId": deployment.id,
                    "organizationId": workload.organization_id,
                    "revisionId": revision.id,
                    "workloadId": workload.id,
                    "externalSourceRevisionId": source.id,
                    "buildRunId": build.id,
                    "publishedArtifactDigest": published.digest,
                }),
                command.requested_at,
            );
            let event = DeploymentRequested::envelope(&deployment, &revision, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let bundle = match workloads
                .create_deployment(CreateDeploymentBundle {
                    workload,
                    revision,
                    deployment,
                    operation,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(bundle) => bundle,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateSourceWorkloadDeploymentResult { bundle }))
        })
    }
}
