use super::*;
use crate::modules::artifacts::domain::test_support::{
    succeeded_external_build_with_output, typed_build_output,
};
use crate::modules::artifacts::domain::BuildRun;
use crate::modules::developer_workflows::domain::{
    AcceptedBuildPlan, AcceptedBuildPlanContract, BuildPlanDetectorKind, BuildPlanProposal,
    BuildPlanProposalSpec, ScheduledTaskCatchUpPolicy, ScheduledTaskHistoryPolicy,
    ScheduledTaskRetryPolicy, ScheduledTaskSchedule, SourceLayoutIdentity, WorkloadHttpHealthCheck,
    WorkloadProcess, WorkloadProfileContract, WorkloadProfileKind, WorkloadProfileResources,
    WorkloadProfileSpec, WorkloadSecretBinding, WorkloadSecretTarget, WorkloadServicePort,
    BUILD_PLAN_DETECTOR_REVISION,
};
use crate::modules::executions::domain::{
    Execution, ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate,
};
use crate::modules::executions::project_execution_task;
use crate::modules::shared_kernel::domain::{
    BuildPlanId, BuildRunId, EnvironmentId, ExecutionId, GitCommitSha, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, Sha256Digest, SourceRevisionId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::sources::published::BuildRecipe;
use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, OciArtifact, SecretBinding, SecretBindingTarget, ServicePort, ServiceProcess,
    ServiceResources, ServiceTemplate, WorkloadRevision,
};
use crate::modules::workloads::project_runtime_spec;
use a3s_cloud_contracts::DURABLE_CELL_BUNDLE_MEDIA_TYPE;
use a3s_runtime::contract::{NetworkMode, RestartPolicy, RuntimeUnitClass};
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn web_profile_compiles_verified_artifact_into_existing_service_rail() {
    let (build_plan, build_run) = plan_and_build();
    let outcome = verified_outcome(&build_plan, &build_run);
    let encoded = serde_json::to_value(&outcome).expect("serialized build outcome");
    assert_eq!(
        serde_json::from_value::<VerifiedWorkloadBuildOutcome>(encoded.clone())
            .expect("deserialized build outcome"),
        outcome
    );
    let mut unknown = encoded;
    unknown["owner_internal_state"] = serde_json::json!("succeeded");
    assert!(serde_json::from_value::<VerifiedWorkloadBuildOutcome>(unknown).is_err());
    let compiler = compiler(Some(outcome.clone()));
    let profile = WorkloadProfileContract::bind(&build_plan, web_profile()).expect("web profile");

    let CompiledWorkloadProfile::Service(compiled) = compiler
        .compile(&build_plan, &profile, outcome.build_run_id)
        .await
        .expect("compiled web profile")
    else {
        panic!("web profile must compile to a Service");
    };
    assert_eq!(compiled.kind, WorkloadProfileKind::Web);
    assert_eq!(compiled.build_plan_id, build_plan.id);
    assert_eq!(compiled.build_run_id, outcome.build_run_id);
    assert_eq!(compiled.profile_digest, *profile.digest());
    assert_eq!(compiled.public_port.as_deref(), Some("http"));
    let template = compiler.service.take_template();
    assert_eq!(template.artifact.digest, outcome.artifact.digest.as_str());
    assert_eq!(
        compiled.admission.owner_contract_digest,
        Sha256Digest::parse(template.digest().expect("Service template digest"))
            .expect("typed Service template digest")
    );

    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        WorkloadId::new(),
        1,
        template,
        outcome.completed_at,
    )
    .expect("Workload revision");
    let runtime = project_runtime_spec(&revision).expect("Runtime Service");
    assert_eq!(runtime.class, RuntimeUnitClass::Service);
    assert_eq!(runtime.network.mode, NetworkMode::Service);
    assert_eq!(runtime.network.ports[0].name, "http");
    assert!(runtime.health.is_some());
    assert_eq!(runtime.restart, RestartPolicy::Always);
}

#[tokio::test]
async fn worker_profile_has_no_implicit_route_or_service_listener() {
    let (build_plan, build_run) = plan_and_build();
    let outcome = verified_outcome(&build_plan, &build_run);
    let compiler = compiler(Some(outcome.clone()));
    let profile =
        WorkloadProfileContract::bind(&build_plan, worker_profile()).expect("worker profile");
    let CompiledWorkloadProfile::Service(compiled) = compiler
        .compile(&build_plan, &profile, outcome.build_run_id)
        .await
        .expect("compiled worker profile")
    else {
        panic!("worker profile must compile to a Service");
    };
    assert_eq!(compiled.kind, WorkloadProfileKind::Worker);
    assert!(compiled.public_port.is_none());
    let template = compiler.service.take_template();
    assert!(template.ports.is_empty());
    assert_eq!(
        compiled.admission.owner_contract_digest,
        Sha256Digest::parse(template.digest().expect("Service template digest"))
            .expect("typed Service template digest")
    );

    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        WorkloadId::new(),
        1,
        template,
        outcome.completed_at,
    )
    .expect("Workload revision");
    let runtime = project_runtime_spec(&revision).expect("Runtime Service");
    assert_eq!(runtime.class, RuntimeUnitClass::Service);
    assert_eq!(runtime.network.mode, NetworkMode::None);
    assert!(runtime.network.ports.is_empty());
}

#[tokio::test]
async fn scheduled_profile_compiles_to_networkless_existing_execution_task_rail() {
    let (build_plan, build_run) = plan_and_build();
    let outcome = verified_outcome(&build_plan, &build_run);
    let compiler = compiler(Some(outcome.clone()));
    let profile = WorkloadProfileContract::bind(&build_plan, scheduled_profile())
        .expect("scheduled Task profile");
    let CompiledWorkloadProfile::ScheduledTask(compiled) = compiler
        .compile(&build_plan, &profile, outcome.build_run_id)
        .await
        .expect("compiled scheduled Task")
    else {
        panic!("scheduled profile must compile to an Execution Task");
    };
    assert_eq!(compiled.schedule, schedule());
    let template = compiler.scheduled_task.take_template();
    assert_eq!(template.resources.timeout_ms, 60_000);
    assert_eq!(
        compiled.admission.owner_contract_digest,
        Sha256Digest::parse(template.digest().expect("Execution template digest"))
            .expect("typed Execution template digest")
    );

    let execution = Execution::create(
        compiled.organization_id,
        compiled.project_id,
        compiled.environment_id,
        ExecutionId::new(),
        template,
        outcome.completed_at,
    )
    .expect("Execution");
    let runtime = project_execution_task(&execution).expect("Runtime Task");
    assert_eq!(runtime.class, RuntimeUnitClass::Task);
    assert_eq!(runtime.network.mode, NetworkMode::None);
    assert_eq!(runtime.restart, RestartPolicy::Never);
    assert_eq!(runtime.resources.execution_timeout_ms, Some(60_000));
}

#[tokio::test]
async fn compilation_rejects_scope_plan_and_build_evidence_drift() {
    let (build_plan, build_run) = plan_and_build();
    let outcome = verified_outcome(&build_plan, &build_run);
    let profile = WorkloadProfileContract::bind(&build_plan, web_profile()).expect("web profile");

    let mut wrong_scope = outcome.clone();
    wrong_scope.organization_id = OrganizationId::new();
    assert!(compiler(Some(wrong_scope.clone()))
        .compile(&build_plan, &profile, wrong_scope.build_run_id)
        .await
        .is_err());

    let mut invalid_schema = outcome.clone();
    invalid_schema.schema = "a3s.cloud.developer-workflow-build-outcome.invalid".into();
    assert!(compiler(Some(invalid_schema.clone()))
        .compile(&build_plan, &profile, invalid_schema.build_run_id)
        .await
        .is_err());

    let mut changed_source = outcome.clone();
    changed_source.source_content_digest = digest('9');
    assert!(compiler(Some(changed_source.clone()))
        .compile(&build_plan, &profile, changed_source.build_run_id)
        .await
        .is_err());

    let mut changed_plan = outcome.clone();
    changed_plan.build_plan_id = BuildPlanId::new();
    assert!(compiler(Some(changed_plan.clone()))
        .compile(&build_plan, &profile, changed_plan.build_run_id)
        .await
        .is_err());

    assert!(compiler(None)
        .compile(&build_plan, &profile, outcome.build_run_id)
        .await
        .is_err());

    let mut invalid_chronology = outcome.clone();
    invalid_chronology.attested_at = invalid_chronology.completed_at + Duration::seconds(1);
    assert!(compiler(Some(invalid_chronology.clone()))
        .compile(&build_plan, &profile, invalid_chronology.build_run_id)
        .await
        .is_err());

    let mut unpinned_artifact = outcome.clone();
    unpinned_artifact.artifact.uri = "oci://registry.example/a3s/workload:latest".into();
    assert!(compiler(Some(unpinned_artifact.clone()))
        .compile(&build_plan, &profile, unpinned_artifact.build_run_id)
        .await
        .is_err());

    let mut invalid_repository = outcome.clone();
    invalid_repository.artifact.uri = format!(
        "oci://registry.example@{}",
        invalid_repository.artifact.digest.as_str()
    );
    assert!(compiler(Some(invalid_repository.clone()))
        .compile(&build_plan, &profile, invalid_repository.build_run_id)
        .await
        .is_err());

    let mut non_runtime_artifact = outcome.clone();
    non_runtime_artifact.artifact.media_type = DURABLE_CELL_BUNDLE_MEDIA_TYPE.into();
    assert!(compiler(Some(non_runtime_artifact.clone()))
        .compile(&build_plan, &profile, non_runtime_artifact.build_run_id)
        .await
        .is_err());

    let mut noncanonical_time = outcome.clone();
    noncanonical_time.completed_at += Duration::nanoseconds(1);
    assert!(compiler(Some(noncanonical_time.clone()))
        .compile(&build_plan, &profile, noncanonical_time.build_run_id)
        .await
        .is_err());

    let other_plan = accepted_plan(
        OrganizationId::new(),
        build_plan.project_id,
        build_plan.environment_id,
        build_plan.source_revision_id,
        build_plan.accepted_at,
    );
    assert!(compiler(Some(outcome.clone()))
        .compile(&other_plan, &profile, outcome.build_run_id)
        .await
        .is_err());
}

#[tokio::test]
async fn compilation_rejects_an_owner_receipt_from_another_artifact_binding() {
    let (build_plan, build_run) = plan_and_build();
    let outcome = verified_outcome(&build_plan, &build_run);
    let profile = WorkloadProfileContract::bind(&build_plan, web_profile()).expect("web profile");
    let compiler = WorkloadProfileCompilationService::new(
        Arc::new(FakeBuildOutcomePort {
            outcome: Some(outcome.clone()),
        }),
        Arc::new(DriftingServiceAdmissionPort),
        Arc::new(FakeScheduledTaskAdmissionPort::default()),
    );

    assert!(compiler
        .compile(&build_plan, &profile, outcome.build_run_id)
        .await
        .is_err());
}

#[derive(Clone)]
struct FakeBuildOutcomePort {
    outcome: Option<VerifiedWorkloadBuildOutcome>,
}

#[async_trait]
impl IWorkloadBuildOutcomePort for FakeBuildOutcomePort {
    async fn verified_outcome(
        &self,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
    ) -> Result<Option<VerifiedWorkloadBuildOutcome>, RepositoryError> {
        Ok(self.outcome.clone().filter(|outcome| {
            outcome.organization_id == organization_id && outcome.build_run_id == build_run_id
        }))
    }
}

#[derive(Default)]
struct FakeServiceAdmissionPort {
    template: Mutex<Option<ServiceTemplate>>,
}

impl FakeServiceAdmissionPort {
    fn take_template(&self) -> ServiceTemplate {
        self.template
            .lock()
            .expect("Service admission lock")
            .take()
            .expect("admitted Service template")
    }
}

#[async_trait]
impl IServiceProfileAdmissionPort for FakeServiceAdmissionPort {
    async fn admit_service_profile(
        &self,
        request: ServiceProfileAdmissionRequest,
    ) -> Result<WorkloadProfileAdmissionReceipt, RepositoryError> {
        request.validate().map_err(RepositoryError::Conflict)?;
        let template = adapt_service_template(&request);
        let digest = template
            .digest()
            .and_then(Sha256Digest::parse)
            .map_err(RepositoryError::Conflict)?;
        *self
            .template
            .lock()
            .map_err(|_| RepositoryError::Storage("Service admission lock poisoned".into()))? =
            Some(template);
        Ok(WorkloadProfileAdmissionReceipt {
            target: WorkloadProfileAdmissionTarget::Service,
            context: request.context,
            artifact_digest: request.artifact.digest,
            owner_contract_digest: digest,
        })
    }
}

struct DriftingServiceAdmissionPort;

#[async_trait]
impl IServiceProfileAdmissionPort for DriftingServiceAdmissionPort {
    async fn admit_service_profile(
        &self,
        request: ServiceProfileAdmissionRequest,
    ) -> Result<WorkloadProfileAdmissionReceipt, RepositoryError> {
        request.validate().map_err(RepositoryError::Conflict)?;
        Ok(WorkloadProfileAdmissionReceipt {
            target: WorkloadProfileAdmissionTarget::Service,
            context: request.context,
            artifact_digest: digest('f'),
            owner_contract_digest: digest('e'),
        })
    }
}

#[derive(Default)]
struct FakeScheduledTaskAdmissionPort {
    template: Mutex<Option<ExecutionTemplate>>,
}

impl FakeScheduledTaskAdmissionPort {
    fn take_template(&self) -> ExecutionTemplate {
        self.template
            .lock()
            .expect("scheduled Task admission lock")
            .take()
            .expect("admitted Execution template")
    }
}

#[async_trait]
impl IScheduledTaskProfileAdmissionPort for FakeScheduledTaskAdmissionPort {
    async fn admit_scheduled_task_profile(
        &self,
        request: ScheduledTaskProfileAdmissionRequest,
    ) -> Result<WorkloadProfileAdmissionReceipt, RepositoryError> {
        request.validate().map_err(RepositoryError::Conflict)?;
        let template = adapt_execution_template(&request).map_err(RepositoryError::Conflict)?;
        let digest = template
            .digest()
            .and_then(Sha256Digest::parse)
            .map_err(RepositoryError::Conflict)?;
        *self.template.lock().map_err(|_| {
            RepositoryError::Storage("scheduled Task admission lock poisoned".into())
        })? = Some(template);
        Ok(WorkloadProfileAdmissionReceipt {
            target: WorkloadProfileAdmissionTarget::ScheduledTask,
            context: request.context,
            artifact_digest: request.artifact.digest,
            owner_contract_digest: digest,
        })
    }
}

struct CompilerFixture {
    compiler: WorkloadProfileCompilationService,
    service: Arc<FakeServiceAdmissionPort>,
    scheduled_task: Arc<FakeScheduledTaskAdmissionPort>,
}

impl std::ops::Deref for CompilerFixture {
    type Target = WorkloadProfileCompilationService;

    fn deref(&self) -> &Self::Target {
        &self.compiler
    }
}

fn compiler(outcome: Option<VerifiedWorkloadBuildOutcome>) -> CompilerFixture {
    let service = Arc::new(FakeServiceAdmissionPort::default());
    let scheduled_task = Arc::new(FakeScheduledTaskAdmissionPort::default());
    CompilerFixture {
        compiler: WorkloadProfileCompilationService::new(
            Arc::new(FakeBuildOutcomePort { outcome }),
            service.clone(),
            scheduled_task.clone(),
        ),
        service,
        scheduled_task,
    }
}

fn adapt_service_template(request: &ServiceProfileAdmissionRequest) -> ServiceTemplate {
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
        secrets: profile.secrets.iter().map(adapt_secret_binding).collect(),
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

fn adapt_execution_template(
    request: &ScheduledTaskProfileAdmissionRequest,
) -> Result<ExecutionTemplate, String> {
    let profile = &request.profile;
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
            timeout_ms: profile
                .resources
                .execution_timeout_ms
                .ok_or_else(|| "scheduled Task profile requires an execution timeout".to_owned())?,
        },
    })
}

fn adapt_secret_binding(secret: &WorkloadSecretBinding) -> SecretBinding {
    SecretBinding {
        name: secret.name.clone(),
        secret_id: secret.secret_id,
        version: secret.version,
        target: match &secret.target {
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

fn verified_outcome(
    build_plan: &AcceptedBuildPlan,
    build_run: &BuildRun,
) -> VerifiedWorkloadBuildOutcome {
    let evidence = build_run
        .evidence
        .as_deref()
        .expect("verified build evidence");
    let artifact = build_run
        .published_artifact
        .as_ref()
        .expect("published artifact");
    VerifiedWorkloadBuildOutcome {
        schema: WORKLOAD_BUILD_OUTCOME_SCHEMA.into(),
        organization_id: build_plan.organization_id,
        project_id: build_plan.project_id,
        environment_id: build_plan.environment_id,
        build_plan_id: build_plan.id,
        build_plan_digest: build_plan.contract.digest().clone(),
        source_revision_id: build_plan.source_revision_id,
        build_run_id: build_run.id,
        source_commit_sha: GitCommitSha::parse(&evidence.commit_sha).expect("source commit"),
        source_content_digest: Sha256Digest::parse(&evidence.source_content_digest)
            .expect("source content digest"),
        recipe: evidence.recipe.clone(),
        artifact: VerifiedOciArtifact {
            uri: artifact.uri.clone(),
            digest: Sha256Digest::parse(&artifact.digest).expect("artifact digest"),
            media_type: artifact.media_type.clone(),
        },
        requested_at: build_run.requested_at,
        attested_at: evidence.attested_at,
        completed_at: build_run.finished_at.expect("finish time"),
    }
}

fn plan_and_build() -> (AcceptedBuildPlan, BuildRun) {
    let accepted_at = Utc
        .with_ymd_and_hms(2026, 8, 24, 1, 0, 0)
        .single()
        .expect("timestamp");
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let source_revision_id = SourceRevisionId::new();
    let plan = accepted_plan(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        accepted_at,
    );
    let output_digest = digest('d').to_string();
    let build = succeeded_external_build_with_output(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        typed_build_output(&output_digest, DURABLE_CELL_BUNDLE_MEDIA_TYPE, 64),
        accepted_at + Duration::seconds(1),
    );
    (plan, build)
}

fn accepted_plan(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    accepted_at: chrono::DateTime<Utc>,
) -> AcceptedBuildPlan {
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        vec!["linux/amd64".into()],
    )
    .expect("recipe");
    let proposal = BuildPlanProposal::from_spec(BuildPlanProposalSpec {
        source: SourceLayoutIdentity::new(
            digest('1'),
            GitCommitSha::parse("a".repeat(40)).expect("commit"),
            digest('2'),
        )
        .expect("source identity"),
        detector: BuildPlanDetectorKind::Dockerfile,
        detector_revision: BUILD_PLAN_DETECTOR_REVISION.into(),
        project_root: ".".into(),
        evidence_path: "Dockerfile".into(),
        evidence_digest: digest('c'),
        recipe,
    })
    .expect("proposal");
    let contract = AcceptedBuildPlanContract::from_proposal(source_revision_id, proposal)
        .expect("accepted contract");
    AcceptedBuildPlan::accept(
        organization_id,
        project_id,
        environment_id,
        contract,
        PrincipalId::new(),
        accepted_at,
    )
    .expect("accepted BuildPlan")
}

fn digest(seed: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", seed.to_string().repeat(64))).expect("digest")
}

fn process() -> WorkloadProcess {
    WorkloadProcess {
        command: vec!["/app/service".into()],
        args: vec!["--production".into()],
        working_directory: Some("/app".into()),
        environment: BTreeMap::from([("LOG_LEVEL".into(), "info".into())]),
    }
}

fn resources(execution_timeout_ms: Option<u64>) -> WorkloadProfileResources {
    WorkloadProfileResources {
        cpu_millis: 250,
        memory_bytes: 128 * 1024 * 1024,
        pids: 64,
        ephemeral_storage_bytes: None,
        execution_timeout_ms,
    }
}

fn web_profile() -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "api".into(),
        kind: WorkloadProfileKind::Web,
        process: process(),
        secrets: Vec::new(),
        resources: resources(None),
        ports: vec![WorkloadServicePort {
            name: "http".into(),
            container_port: 8_080,
        }],
        health: Some(WorkloadHttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 5_000,
            timeout_ms: 1_000,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
            stabilization_window_ms: 10_000,
        }),
        public_port: Some("http".into()),
        schedule: None,
    }
}

fn worker_profile() -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "events".into(),
        kind: WorkloadProfileKind::Worker,
        process: process(),
        secrets: Vec::new(),
        resources: resources(None),
        ports: Vec::new(),
        health: None,
        public_port: None,
        schedule: None,
    }
}

fn scheduled_profile() -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "cleanup".into(),
        kind: WorkloadProfileKind::ScheduledTask,
        process: process(),
        secrets: Vec::new(),
        resources: resources(Some(60_000)),
        ports: Vec::new(),
        health: None,
        public_port: None,
        schedule: Some(schedule()),
    }
}

fn schedule() -> ScheduledTaskSchedule {
    ScheduledTaskSchedule {
        expression: "0 */5 * * * * *".into(),
        timezone: "Asia/Shanghai".into(),
        catch_up: ScheduledTaskCatchUpPolicy::Skip,
        maximum_concurrency: 1,
        misfire_grace_ms: 60_000,
        retry: ScheduledTaskRetryPolicy {
            maximum_attempts: 3,
            initial_backoff_ms: 1_000,
            maximum_backoff_ms: 30_000,
        },
        history: ScheduledTaskHistoryPolicy {
            successful_limit: 20,
            failed_limit: 20,
            maximum_age_days: 30,
        },
    }
}
