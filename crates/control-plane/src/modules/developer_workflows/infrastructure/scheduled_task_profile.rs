use crate::modules::developer_workflows::application::{
    IScheduledTaskProfileAdmissionPort, ScheduledTaskProfileAdmissionRequest,
    WorkloadProfileAdmissionReceipt, WorkloadProfileAdmissionTarget,
};
use crate::modules::executions::domain::{
    ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate,
};
use crate::modules::shared_kernel::domain::{RepositoryError, Sha256Digest};
use async_trait::async_trait;

/// Anti-corruption adapter from an accepted Developer Workflows scheduled
/// profile into Executions' existing immutable `ExecutionTemplate` contract.
///
/// This component owns no template revision, Execution, scheduler, retry, or
/// Operation state. A later owner handoff can use the returned digest as exact
/// admission evidence while those lifecycles remain with their existing owners.
#[derive(Debug, Default)]
pub struct ExecutionsScheduledTaskProfileAdapter;

impl ExecutionsScheduledTaskProfileAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl IScheduledTaskProfileAdmissionPort for ExecutionsScheduledTaskProfileAdapter {
    async fn admit_scheduled_task_profile(
        &self,
        request: ScheduledTaskProfileAdmissionRequest,
    ) -> Result<WorkloadProfileAdmissionReceipt, RepositoryError> {
        request
            .validate()
            .map_err(scheduled_task_profile_conflict)?;
        let template =
            project_execution_template(&request).map_err(scheduled_task_profile_conflict)?;
        let owner_contract_digest = template
            .digest()
            .and_then(Sha256Digest::parse)
            .map_err(scheduled_task_profile_conflict)?;

        Ok(WorkloadProfileAdmissionReceipt {
            target: WorkloadProfileAdmissionTarget::ScheduledTask,
            context: request.context,
            artifact_digest: request.artifact.digest,
            owner_contract_digest,
        })
    }
}

fn project_execution_template(
    request: &ScheduledTaskProfileAdmissionRequest,
) -> Result<ExecutionTemplate, String> {
    let profile = &request.profile;
    let timeout_ms = profile
        .resources
        .execution_timeout_ms
        .ok_or_else(|| "scheduled Task profile requires an execution timeout".to_owned())?;
    Ok(ExecutionTemplate {
        artifact: ExecutionArtifact {
            uri: request.artifact.uri.clone(),
            digest: request.artifact.digest.as_str().to_owned(),
            media_type: request.artifact.media_type.clone(),
        },
        process: ExecutionProcess {
            command: profile.process.command.clone(),
            args: profile.process.args.clone(),
            working_directory: profile.process.working_directory.clone(),
            environment: profile.process.environment.clone(),
        },
        input: serde_json::Value::Null,
        resources: ExecutionResources {
            cpu_millis: profile.resources.cpu_millis,
            memory_bytes: profile.resources.memory_bytes,
            pids: profile.resources.pids,
            ephemeral_storage_bytes: profile.resources.ephemeral_storage_bytes,
            timeout_ms,
        },
    })
}

fn scheduled_task_profile_conflict(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "Executions rejected the immutable scheduled Task profile contract: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::application::{
        VerifiedOciArtifact, WorkloadProfileTargetContext,
    };
    use crate::modules::developer_workflows::domain::{
        ScheduledTaskCatchUpPolicy, ScheduledTaskHistoryPolicy, ScheduledTaskRetryPolicy,
        ScheduledTaskSchedule, WorkloadProcess, WorkloadProfileKind, WorkloadProfileResources,
        WorkloadProfileSpec,
    };
    use crate::modules::shared_kernel::domain::{
        BuildPlanId, BuildRunId, EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
    };
    use a3s_cloud_contracts::OCI_IMAGE_MANIFEST_MEDIA_TYPE;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn admits_one_exact_execution_template_without_copying_execution_lifecycle() {
        let request = scheduled_request();
        let expected_template =
            project_execution_template(&request).expect("scheduled Execution template");
        let expected_digest = Sha256Digest::parse(
            expected_template
                .digest()
                .expect("Executions template digest"),
        )
        .expect("typed Executions template digest");

        assert_eq!(expected_template.artifact.uri, request.artifact.uri);
        assert_eq!(
            expected_template.artifact.digest,
            request.artifact.digest.as_str()
        );
        assert_eq!(expected_template.process.command, ["backup"]);
        assert_eq!(expected_template.process.args, ["--incremental"]);
        assert_eq!(
            expected_template.process.working_directory.as_deref(),
            Some("/workspace")
        );
        assert_eq!(
            expected_template
                .process
                .environment
                .get("RUST_LOG")
                .map(String::as_str),
            Some("info")
        );
        assert_eq!(expected_template.input, serde_json::Value::Null);
        assert_eq!(expected_template.resources.cpu_millis, 500);
        assert_eq!(expected_template.resources.memory_bytes, 512 * 1024 * 1024);
        assert_eq!(expected_template.resources.pids, 128);
        assert_eq!(
            expected_template.resources.ephemeral_storage_bytes,
            Some(1024 * 1024 * 1024)
        );
        assert_eq!(expected_template.resources.timeout_ms, 120_000);

        let receipt = ExecutionsScheduledTaskProfileAdapter::new()
            .admit_scheduled_task_profile(request.clone())
            .await
            .expect("Executions admission");
        receipt
            .validate_for(
                WorkloadProfileAdmissionTarget::ScheduledTask,
                &request.context,
                &request.artifact.digest,
            )
            .expect("exact admission receipt");
        assert_eq!(receipt.owner_contract_digest, expected_digest);
    }

    #[tokio::test]
    async fn rejects_non_scheduled_shape_and_executions_owner_rule_drift() {
        let mut service = scheduled_request();
        service.profile.kind = WorkloadProfileKind::Worker;
        assert!(matches!(
            ExecutionsScheduledTaskProfileAdapter::new()
                .admit_scheduled_task_profile(service)
                .await,
            Err(RepositoryError::Conflict(message))
                if message.contains("Executions rejected the immutable scheduled Task profile contract")
        ));

        let mut empty_command = scheduled_request();
        empty_command.profile.process.command = vec![String::new()];
        empty_command
            .validate()
            .expect("consumer profile permits an empty process value");
        assert!(matches!(
            ExecutionsScheduledTaskProfileAdapter::new()
                .admit_scheduled_task_profile(empty_command)
                .await,
            Err(RepositoryError::Conflict(message))
                if message.contains("execution process configuration is invalid")
        ));
    }

    fn scheduled_request() -> ScheduledTaskProfileAdmissionRequest {
        ScheduledTaskProfileAdmissionRequest {
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
                name: "nightly-backup".into(),
                kind: WorkloadProfileKind::ScheduledTask,
                process: WorkloadProcess {
                    command: vec!["backup".into()],
                    args: vec!["--incremental".into()],
                    working_directory: Some("/workspace".into()),
                    environment: BTreeMap::from([("RUST_LOG".into(), "info".into())]),
                },
                secrets: Vec::new(),
                resources: WorkloadProfileResources {
                    cpu_millis: 500,
                    memory_bytes: 512 * 1024 * 1024,
                    pids: 128,
                    ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
                    execution_timeout_ms: Some(120_000),
                },
                ports: Vec::new(),
                health: None,
                public_port: None,
                schedule: Some(ScheduledTaskSchedule {
                    expression: "0 0 2 * * * *".into(),
                    timezone: "UTC".into(),
                    catch_up: ScheduledTaskCatchUpPolicy::Latest,
                    maximum_concurrency: 1,
                    misfire_grace_ms: 60_000,
                    retry: ScheduledTaskRetryPolicy {
                        maximum_attempts: 3,
                        initial_backoff_ms: 1_000,
                        maximum_backoff_ms: 30_000,
                    },
                    history: ScheduledTaskHistoryPolicy {
                        successful_limit: 10,
                        failed_limit: 10,
                        maximum_age_days: 30,
                    },
                }),
            },
            artifact: VerifiedOciArtifact {
                uri: format!(
                    "oci://registry.example.test/team/backup@{}",
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
