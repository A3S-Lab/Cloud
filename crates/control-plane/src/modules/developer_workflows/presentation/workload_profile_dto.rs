use crate::modules::developer_workflows::{
    AcceptWorkloadProfileResult, AcceptedWorkloadProfileRevision, ScheduledTaskSchedule,
    WorkloadHttpHealthCheck, WorkloadProcess, WorkloadProfileResources, WorkloadProfileSpec,
    WorkloadSecretBinding, WorkloadSecretTarget, WorkloadServicePort,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptWorkloadProfileRequest {
    pub build_plan_id: Uuid,
    pub profile_acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadProcessResponse {
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub working_directory: Option<String>,
    pub environment: BTreeMap<String, String>,
}

impl From<&WorkloadProcess> for WorkloadProcessResponse {
    fn from(process: &WorkloadProcess) -> Self {
        Self {
            command: process.command.clone(),
            args: process.args.clone(),
            working_directory: process.working_directory.clone(),
            environment: process.environment.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkloadSecretTargetResponse {
    Environment { variable: String },
    File { path: String, mode: u32 },
    RegistryCredential,
}

impl From<&WorkloadSecretTarget> for WorkloadSecretTargetResponse {
    fn from(target: &WorkloadSecretTarget) -> Self {
        match target {
            WorkloadSecretTarget::Environment { variable } => Self::Environment {
                variable: variable.clone(),
            },
            WorkloadSecretTarget::File { path, mode } => Self::File {
                path: path.clone(),
                mode: *mode,
            },
            WorkloadSecretTarget::RegistryCredential => Self::RegistryCredential,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSecretBindingResponse {
    pub name: String,
    pub secret_id: Uuid,
    pub version: u64,
    pub target: WorkloadSecretTargetResponse,
}

impl From<&WorkloadSecretBinding> for WorkloadSecretBindingResponse {
    fn from(binding: &WorkloadSecretBinding) -> Self {
        Self {
            name: binding.name.clone(),
            secret_id: binding.secret_id.as_uuid(),
            version: binding.version,
            target: (&binding.target).into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadProfileResourcesResponse {
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub ephemeral_storage_bytes: Option<u64>,
    pub execution_timeout_ms: Option<u64>,
}

impl From<&WorkloadProfileResources> for WorkloadProfileResourcesResponse {
    fn from(resources: &WorkloadProfileResources) -> Self {
        Self {
            cpu_millis: resources.cpu_millis,
            memory_bytes: resources.memory_bytes,
            pids: resources.pids,
            ephemeral_storage_bytes: resources.ephemeral_storage_bytes,
            execution_timeout_ms: resources.execution_timeout_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadServicePortResponse {
    pub name: String,
    pub container_port: u16,
}

impl From<&WorkloadServicePort> for WorkloadServicePortResponse {
    fn from(port: &WorkloadServicePort) -> Self {
        Self {
            name: port.name.clone(),
            container_port: port.container_port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadHttpHealthCheckResponse {
    pub port_name: String,
    pub path: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub healthy_threshold: u16,
    pub unhealthy_threshold: u16,
    pub stabilization_window_ms: u64,
}

impl From<&WorkloadHttpHealthCheck> for WorkloadHttpHealthCheckResponse {
    fn from(health: &WorkloadHttpHealthCheck) -> Self {
        Self {
            port_name: health.port_name.clone(),
            path: health.path.clone(),
            interval_ms: health.interval_ms,
            timeout_ms: health.timeout_ms,
            healthy_threshold: health.healthy_threshold,
            unhealthy_threshold: health.unhealthy_threshold,
            stabilization_window_ms: health.stabilization_window_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskRetryPolicyResponse {
    pub maximum_attempts: u16,
    pub initial_backoff_ms: u64,
    pub maximum_backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskHistoryPolicyResponse {
    pub successful_limit: u16,
    pub failed_limit: u16,
    pub maximum_age_days: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskScheduleResponse {
    pub expression: String,
    pub timezone: String,
    pub catch_up: String,
    pub maximum_concurrency: u16,
    pub misfire_grace_ms: u64,
    pub retry: ScheduledTaskRetryPolicyResponse,
    pub history: ScheduledTaskHistoryPolicyResponse,
}

impl From<&ScheduledTaskSchedule> for ScheduledTaskScheduleResponse {
    fn from(schedule: &ScheduledTaskSchedule) -> Self {
        Self {
            expression: schedule.expression.clone(),
            timezone: schedule.timezone.clone(),
            catch_up: schedule.catch_up.as_str().into(),
            maximum_concurrency: schedule.maximum_concurrency,
            misfire_grace_ms: schedule.misfire_grace_ms,
            retry: ScheduledTaskRetryPolicyResponse {
                maximum_attempts: schedule.retry.maximum_attempts,
                initial_backoff_ms: schedule.retry.initial_backoff_ms,
                maximum_backoff_ms: schedule.retry.maximum_backoff_ms,
            },
            history: ScheduledTaskHistoryPolicyResponse {
                successful_limit: schedule.history.successful_limit,
                failed_limit: schedule.history.failed_limit,
                maximum_age_days: schedule.history.maximum_age_days,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadProfileSpecResponse {
    pub name: String,
    pub kind: String,
    pub process: WorkloadProcessResponse,
    pub secrets: Vec<WorkloadSecretBindingResponse>,
    pub resources: WorkloadProfileResourcesResponse,
    pub ports: Vec<WorkloadServicePortResponse>,
    pub health: Option<WorkloadHttpHealthCheckResponse>,
    pub public_port: Option<String>,
    pub schedule: Option<ScheduledTaskScheduleResponse>,
}

impl From<&WorkloadProfileSpec> for WorkloadProfileSpecResponse {
    fn from(profile: &WorkloadProfileSpec) -> Self {
        Self {
            name: profile.name.clone(),
            kind: profile.kind.as_str().into(),
            process: (&profile.process).into(),
            secrets: profile.secrets.iter().map(Into::into).collect(),
            resources: (&profile.resources).into(),
            ports: profile.ports.iter().map(Into::into).collect(),
            health: profile.health.as_ref().map(Into::into),
            public_port: profile.public_port.clone(),
            schedule: profile.schedule.as_ref().map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedWorkloadProfileRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub workload_profile_id: Uuid,
    pub workload_profile_revision_id: Uuid,
    pub revision_number: u64,
    pub build_plan_id: Uuid,
    pub source_revision_id: Uuid,
    pub contract_schema: String,
    pub contract_acl: String,
    pub contract_digest: String,
    pub build_plan_digest: String,
    pub project_root: String,
    pub profile: WorkloadProfileSpecResponse,
    pub accepted_by: Uuid,
    pub accepted_at: DateTime<Utc>,
}

impl From<AcceptedWorkloadProfileRevision> for AcceptedWorkloadProfileRevisionResponse {
    fn from(revision: AcceptedWorkloadProfileRevision) -> Self {
        let spec = revision.contract.spec();
        Self {
            organization_id: revision.organization_id.as_uuid(),
            project_id: revision.project_id.as_uuid(),
            environment_id: revision.environment_id.as_uuid(),
            workload_profile_id: revision.profile_id.as_uuid(),
            workload_profile_revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            build_plan_id: revision.build_plan_id.as_uuid(),
            source_revision_id: revision.source_revision_id.as_uuid(),
            contract_schema: revision.contract.schema().into(),
            contract_acl: revision.contract.canonical_acl().into(),
            contract_digest: revision.contract.digest().as_str().into(),
            build_plan_digest: spec.build_plan_digest.as_str().into(),
            project_root: spec.project_root.clone(),
            profile: (&spec.profile).into(),
            accepted_by: revision.accepted_by.as_uuid(),
            accepted_at: revision.accepted_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadProfileMutationResponse {
    pub workload_profile_revision: AcceptedWorkloadProfileRevisionResponse,
    pub replayed: bool,
}

impl From<AcceptWorkloadProfileResult> for WorkloadProfileMutationResponse {
    fn from(result: AcceptWorkloadProfileResult) -> Self {
        Self {
            workload_profile_revision: result.revision.into(),
            replayed: result.replayed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::WorkloadProfileContract;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, PrincipalId, ProjectId,
    };

    const PROFILE_FIXTURE: &str =
        include_str!("../../../../../../contracts/p0.2/workload-profile.acl");

    #[test]
    fn workload_profile_acceptance_request_is_closed_and_acl_only() {
        let build_plan_id = Uuid::now_v7();
        let request: AcceptWorkloadProfileRequest = serde_json::from_value(serde_json::json!({
            "buildPlanId": build_plan_id,
            "profileAcl": PROFILE_FIXTURE
        }))
        .expect("closed workload-profile request");
        assert_eq!(request.build_plan_id, build_plan_id);
        assert_eq!(request.profile_acl, PROFILE_FIXTURE);
        assert!(
            serde_json::from_value::<AcceptWorkloadProfileRequest>(serde_json::json!({
                "buildPlanId": build_plan_id,
                "profileAcl": PROFILE_FIXTURE,
                "profile": {}
            }))
            .is_err()
        );
    }

    #[test]
    fn workload_profile_response_preserves_canonical_acl_and_typed_intent() {
        let contract = WorkloadProfileContract::parse_acl(PROFILE_FIXTURE).expect("profile ACL");
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let profile_id = AcceptedWorkloadProfileRevision::profile_id_for(
            organization_id,
            project_id,
            environment_id,
            &contract,
        )
        .expect("profile identity");
        let revision_id =
            AcceptedWorkloadProfileRevision::revision_id_for(profile_id, 1, &contract)
                .expect("revision identity");
        let revision = AcceptedWorkloadProfileRevision::restore(
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision_id,
            1,
            contract.spec().build_plan_id,
            contract.spec().source_revision_id,
            contract.canonical_acl(),
            contract.digest().as_str(),
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("accepted workload-profile revision");

        let response = AcceptedWorkloadProfileRevisionResponse::from(revision);
        assert_eq!(response.workload_profile_id, profile_id.as_uuid());
        assert_eq!(response.workload_profile_revision_id, revision_id.as_uuid());
        assert_eq!(response.contract_acl, PROFILE_FIXTURE);
        assert_eq!(response.profile.kind, "web");
        assert_eq!(response.profile.public_port.as_deref(), Some("http"));
        assert_eq!(response.profile.ports[0].container_port, 8080);

        let json = serde_json::to_value(response).expect("response JSON");
        assert!(json.get("contractAcl").is_some());
        assert!(json["profile"].get("publicPort").is_some());
        assert!(json["profile"]["resources"].get("cpuMillis").is_some());
        assert!(json.get("secretValue").is_none());
    }
}
