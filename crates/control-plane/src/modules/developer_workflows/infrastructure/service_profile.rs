use crate::modules::developer_workflows::application::{
    IServiceProfileAdmissionPort, ServiceProfileAdmissionRequest, WorkloadProfileAdmissionReceipt,
    WorkloadProfileAdmissionTarget,
};
use crate::modules::developer_workflows::domain::{WorkloadSecretBinding, WorkloadSecretTarget};
use crate::modules::shared_kernel::domain::{RepositoryError, Sha256Digest};
use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, OciArtifact, SecretBinding, SecretBindingTarget, ServicePort, ServiceProcess,
    ServiceResources, ServiceTemplate,
};
use async_trait::async_trait;

/// Anti-corruption adapter from an accepted Developer Workflows Service
/// profile into Workloads' existing immutable `ServiceTemplate` contract.
///
/// This component owns no Workload, Deployment, retry, or rollout state. A
/// later owner handoff can use the returned digest as exact admission evidence
/// while Workloads remains the sole lifecycle authority.
#[derive(Debug, Default)]
pub struct WorkloadsServiceProfileAdapter;

impl WorkloadsServiceProfileAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl IServiceProfileAdmissionPort for WorkloadsServiceProfileAdapter {
    async fn admit_service_profile(
        &self,
        request: ServiceProfileAdmissionRequest,
    ) -> Result<WorkloadProfileAdmissionReceipt, RepositoryError> {
        request.validate().map_err(service_profile_conflict)?;
        let template = project_service_template(&request);
        let owner_contract_digest = template
            .digest()
            .and_then(Sha256Digest::parse)
            .map_err(service_profile_conflict)?;

        Ok(WorkloadProfileAdmissionReceipt {
            target: WorkloadProfileAdmissionTarget::Service,
            context: request.context,
            artifact_digest: request.artifact.digest,
            owner_contract_digest,
        })
    }
}

fn project_service_template(request: &ServiceProfileAdmissionRequest) -> ServiceTemplate {
    let profile = &request.profile;
    ServiceTemplate {
        artifact: OciArtifact {
            uri: request.artifact.uri.clone(),
            digest: request.artifact.digest.as_str().to_owned(),
            media_type: request.artifact.media_type.clone(),
        },
        process: ServiceProcess {
            command: profile.process.command.clone(),
            args: profile.process.args.clone(),
            working_directory: profile.process.working_directory.clone(),
            environment: profile.process.environment.clone(),
        },
        secrets: profile.secrets.iter().map(project_secret_binding).collect(),
        resources: ServiceResources {
            cpu_millis: profile.resources.cpu_millis,
            memory_bytes: profile.resources.memory_bytes,
            pids: profile.resources.pids,
            ephemeral_storage_bytes: profile.resources.ephemeral_storage_bytes,
        },
        ports: profile
            .ports
            .iter()
            .map(|port| ServicePort {
                name: port.name.clone(),
                container_port: port.container_port,
            })
            .collect(),
        health: profile.health.as_ref().map(|health| HttpHealthCheck {
            port_name: health.port_name.clone(),
            path: health.path.clone(),
            interval_ms: health.interval_ms,
            timeout_ms: health.timeout_ms,
            healthy_threshold: health.healthy_threshold,
            unhealthy_threshold: health.unhealthy_threshold,
            stabilization_window_ms: health.stabilization_window_ms,
        }),
    }
}

fn project_secret_binding(binding: &WorkloadSecretBinding) -> SecretBinding {
    SecretBinding {
        name: binding.name.clone(),
        secret_id: binding.secret_id,
        version: binding.version,
        target: match &binding.target {
            WorkloadSecretTarget::Environment { variable } => SecretBindingTarget::Environment {
                variable: variable.clone(),
            },
            WorkloadSecretTarget::File { path, mode } => SecretBindingTarget::File {
                path: path.clone(),
                mode: *mode,
            },
            WorkloadSecretTarget::RegistryCredential => SecretBindingTarget::RegistryCredential,
        },
    }
}

fn service_profile_conflict(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "Workloads rejected the immutable Service profile contract: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::application::{
        VerifiedOciArtifact, WorkloadProfileTargetContext,
    };
    use crate::modules::developer_workflows::domain::{
        WorkloadHttpHealthCheck, WorkloadProcess, WorkloadProfileKind, WorkloadProfileResources,
        WorkloadProfileSpec, WorkloadServicePort,
    };
    use crate::modules::shared_kernel::domain::{
        BuildPlanId, BuildRunId, EnvironmentId, OrganizationId, ProjectId, SecretId,
        SourceRevisionId,
    };
    use a3s_cloud_contracts::OCI_IMAGE_MANIFEST_MEDIA_TYPE;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn admits_an_exact_web_profile_without_copying_workloads_lifecycle() {
        let request = service_request(WorkloadProfileKind::Web);
        let expected_template = project_service_template(&request);
        let expected_digest = Sha256Digest::parse(
            expected_template
                .digest()
                .expect("Workloads template digest"),
        )
        .expect("typed Workloads template digest");

        assert_eq!(expected_template.artifact.uri, request.artifact.uri);
        assert_eq!(
            expected_template.artifact.digest,
            request.artifact.digest.as_str()
        );
        assert_eq!(
            expected_template.process,
            ServiceProcess {
                command: vec!["server".into()],
                args: vec!["--port".into(), "8080".into()],
                working_directory: Some("/workspace".into()),
                environment: BTreeMap::from([("RUST_LOG".into(), "info".into())]),
            }
        );
        assert_eq!(expected_template.secrets.len(), 3);
        assert!(matches!(
            expected_template.secrets[0].target,
            SecretBindingTarget::Environment { ref variable } if variable == "DATABASE_URL"
        ));
        assert!(matches!(
            expected_template.secrets[1].target,
            SecretBindingTarget::File { ref path, mode } if path == "/run/secrets/token" && mode == 0o400
        ));
        assert!(matches!(
            expected_template.secrets[2].target,
            SecretBindingTarget::RegistryCredential
        ));
        assert_eq!(expected_template.ports[0].name, "http");
        assert_eq!(
            expected_template
                .health
                .as_ref()
                .expect("web health check")
                .path,
            "/healthz"
        );

        let receipt = WorkloadsServiceProfileAdapter::new()
            .admit_service_profile(request.clone())
            .await
            .expect("Workloads admission");
        receipt
            .validate_for(
                WorkloadProfileAdmissionTarget::Service,
                &request.context,
                &request.artifact.digest,
            )
            .expect("exact admission receipt");
        assert_eq!(receipt.owner_contract_digest, expected_digest);
    }

    #[tokio::test]
    async fn admits_a_networkless_worker_contract() {
        let request = service_request(WorkloadProfileKind::Worker);
        let template = project_service_template(&request);
        assert!(template.ports.is_empty());
        assert!(template.health.is_none());

        let receipt = WorkloadsServiceProfileAdapter::new()
            .admit_service_profile(request)
            .await
            .expect("worker admission");
        assert_eq!(
            receipt.owner_contract_digest,
            Sha256Digest::parse(template.digest().expect("worker template digest"))
                .expect("typed worker template digest")
        );
    }

    #[tokio::test]
    async fn rejects_non_service_and_artifact_drift_before_returning_a_receipt() {
        let mut scheduled = service_request(WorkloadProfileKind::Worker);
        scheduled.profile.kind = WorkloadProfileKind::ScheduledTask;
        assert!(matches!(
            WorkloadsServiceProfileAdapter::new()
                .admit_service_profile(scheduled)
                .await,
            Err(RepositoryError::Conflict(message))
                if message.contains("Workloads rejected the immutable Service profile contract")
        ));

        let mut artifact_drift = service_request(WorkloadProfileKind::Web);
        artifact_drift.artifact.uri =
            "oci://registry.example.test/team/app@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .into();
        assert!(matches!(
            WorkloadsServiceProfileAdapter::new()
                .admit_service_profile(artifact_drift)
                .await,
            Err(RepositoryError::Conflict(message)) if message.contains("digest-pinned")
        ));
    }

    fn service_request(kind: WorkloadProfileKind) -> ServiceProfileAdmissionRequest {
        let web = kind == WorkloadProfileKind::Web;
        ServiceProfileAdmissionRequest {
            context: WorkloadProfileTargetContext {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                build_plan_id: BuildPlanId::new(),
                build_run_id: BuildRunId::new(),
                source_revision_id: SourceRevisionId::new(),
                profile_digest: digest('c'),
            },
            profile: WorkloadProfileSpec {
                name: if web { "preview-web" } else { "preview-worker" }.into(),
                kind,
                process: WorkloadProcess {
                    command: vec!["server".into()],
                    args: vec!["--port".into(), "8080".into()],
                    working_directory: Some("/workspace".into()),
                    environment: BTreeMap::from([("RUST_LOG".into(), "info".into())]),
                },
                secrets: vec![
                    WorkloadSecretBinding {
                        name: "database".into(),
                        secret_id: SecretId::new(),
                        version: 3,
                        target: WorkloadSecretTarget::Environment {
                            variable: "DATABASE_URL".into(),
                        },
                    },
                    WorkloadSecretBinding {
                        name: "token".into(),
                        secret_id: SecretId::new(),
                        version: 5,
                        target: WorkloadSecretTarget::File {
                            path: "/run/secrets/token".into(),
                            mode: 0o400,
                        },
                    },
                    WorkloadSecretBinding {
                        name: "registry".into(),
                        secret_id: SecretId::new(),
                        version: 7,
                        target: WorkloadSecretTarget::RegistryCredential,
                    },
                ],
                resources: WorkloadProfileResources {
                    cpu_millis: 500,
                    memory_bytes: 512 * 1024 * 1024,
                    pids: 128,
                    ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
                    execution_timeout_ms: None,
                },
                ports: web
                    .then(|| WorkloadServicePort {
                        name: "http".into(),
                        container_port: 8080,
                    })
                    .into_iter()
                    .collect(),
                health: web.then(|| WorkloadHttpHealthCheck {
                    port_name: "http".into(),
                    path: "/healthz".into(),
                    interval_ms: 5_000,
                    timeout_ms: 1_000,
                    healthy_threshold: 2,
                    unhealthy_threshold: 3,
                    stabilization_window_ms: 10_000,
                }),
                public_port: web.then(|| "http".into()),
                schedule: None,
            },
            artifact: VerifiedOciArtifact {
                uri: format!(
                    "oci://registry.example.test/team/app@{}",
                    digest('a').as_str()
                ),
                digest: digest('a'),
                media_type: OCI_IMAGE_MANIFEST_MEDIA_TYPE.into(),
            },
        }
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }
}
