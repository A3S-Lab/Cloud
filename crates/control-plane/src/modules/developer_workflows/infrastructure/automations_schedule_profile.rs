use crate::modules::developer_workflows::application::{
    AutomationScheduleProfileAdmissionRequest, AutomationScheduleTargetBinding,
    IAutomationScheduleProfileAdmissionPort,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::AutomationTaskTargetV1;
use async_trait::async_trait;

/// Anti-corruption adapter from a P0 scheduled Task profile to Automations.
///
/// This adapter emits immutable target evidence only. It does not persist an
/// Automation revision, create schedule state, run a timer, or admit an
/// invocation; those responsibilities remain with Automations.
#[derive(Debug, Default)]
pub struct AutomationsScheduledTaskProfileAdapter;

impl AutomationsScheduledTaskProfileAdapter {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl IAutomationScheduleProfileAdmissionPort for AutomationsScheduledTaskProfileAdapter {
    async fn admit_automation_schedule_profile(
        &self,
        request: AutomationScheduleProfileAdmissionRequest,
    ) -> Result<AutomationScheduleTargetBinding, RepositoryError> {
        request
            .validate()
            .map_err(automation_schedule_profile_conflict)?;
        let schedule =
            request.profile.schedule.clone().ok_or_else(|| {
                automation_schedule_profile_conflict("schedule is missing".into())
            })?;
        let binding = AutomationScheduleTargetBinding {
            target: AutomationTaskTargetV1 {
                task_profile_id: request.profile_id.as_uuid(),
                task_revision_id: request.profile_revision_id.as_uuid(),
                revision_digest: request.context.profile_digest.as_str().to_owned(),
            },
            schedule,
        };
        binding
            .validate_for(
                request.profile_id,
                request.profile_revision_id,
                &request.context.profile_digest,
            )
            .map_err(automation_schedule_profile_conflict)?;
        Ok(binding)
    }
}

fn automation_schedule_profile_conflict(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "Automations rejected the immutable scheduled Task target contract: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::application::WorkloadProfileTargetContext;
    use crate::modules::developer_workflows::domain::{
        ScheduledTaskCatchUpPolicy, ScheduledTaskHistoryPolicy, ScheduledTaskRetryPolicy,
        ScheduledTaskSchedule, WorkloadProcess, WorkloadProfileKind, WorkloadProfileResources,
        WorkloadProfileSpec,
    };
    use crate::modules::shared_kernel::domain::{
        BuildPlanId, BuildRunId, EnvironmentId, OrganizationId, ProjectId, Sha256Digest,
        SourceRevisionId, WorkloadProfileId, WorkloadProfileRevisionId,
    };
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn emits_only_the_exact_task_target_and_schedule_policy() {
        let request = request();
        let binding = AutomationsScheduledTaskProfileAdapter::new()
            .admit_automation_schedule_profile(request.clone())
            .await
            .expect("Automation target binding");
        binding
            .validate_for(
                request.profile_id,
                request.profile_revision_id,
                &request.context.profile_digest,
            )
            .expect("exact binding");
        assert_eq!(binding.schedule, request.profile.schedule.clone().unwrap());
        assert_eq!(binding.target.task_profile_id, request.profile_id.as_uuid());
        assert_eq!(
            binding.target.task_revision_id,
            request.profile_revision_id.as_uuid()
        );
        assert_eq!(
            binding.target.revision_digest,
            request.context.profile_digest.as_str()
        );
    }

    #[tokio::test]
    async fn rejects_service_profiles_before_emitting_a_target() {
        let mut request = request();
        request.profile.kind = WorkloadProfileKind::Worker;
        assert!(matches!(
            AutomationsScheduledTaskProfileAdapter::new()
                .admit_automation_schedule_profile(request)
                .await,
            Err(RepositoryError::Conflict(message))
                if message.contains("Automations rejected the immutable scheduled Task target contract")
        ));
    }

    fn request() -> AutomationScheduleProfileAdmissionRequest {
        AutomationScheduleProfileAdmissionRequest {
            context: WorkloadProfileTargetContext {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                build_plan_id: BuildPlanId::new(),
                build_run_id: BuildRunId::new(),
                source_revision_id: SourceRevisionId::new(),
                profile_digest: digest('a'),
            },
            profile_id: WorkloadProfileId::new(),
            profile_revision_id: WorkloadProfileRevisionId::new(),
            profile: WorkloadProfileSpec {
                name: "nightly-backup".into(),
                kind: WorkloadProfileKind::ScheduledTask,
                process: WorkloadProcess {
                    command: vec!["backup".into()],
                    args: vec!["--incremental".into()],
                    working_directory: Some("/workspace".into()),
                    environment: BTreeMap::new(),
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
        }
    }

    fn digest(character: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
    }
}
