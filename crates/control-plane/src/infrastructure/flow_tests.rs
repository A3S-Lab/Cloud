use super::*;
use a3s_flow::{WorkflowRunStatus, WorkflowSpec};
use chrono::{DateTime, Utc};
use serde_json::json;

#[test]
fn runtime_build_policy_pins_one_current_generation_and_admits_only_declared_legacy_builds(
) -> Result<(), FlowError> {
    let compatibility = cloud_runtime_build_compatibility()?;

    assert_eq!(
        compatibility.current_build_id().as_str(),
        CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID
    );
    let compatible_build_ids = compatibility
        .compatible_build_ids()
        .map(RuntimeBuildId::as_str)
        .collect::<Vec<_>>();
    let declared_build_ids = REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS
        .iter()
        .copied()
        .chain(std::iter::once(CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        compatible_build_ids,
        declared_build_ids.iter().copied().collect::<Vec<_>>()
    );
    assert_eq!(
        declared_build_ids.len(),
        REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS.len() + 1,
        "current and replay-compatible runtime build identities must be unique"
    );
    assert!(compatibility.accepts_unpinned());
    assert!(compatibility.supports(Some(compatibility.current_build_id())));
    assert!(compatibility.supports(Some(&RuntimeBuildId::new(
        REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS[0]
    )?)));
    assert!(!compatibility.supports(Some(&RuntimeBuildId::new("a3s-cloud-workflows@unknown")?)));
    assert!(compatibility.supports(None));
    Ok(())
}

#[derive(Clone, Copy)]
struct StubRuntime(&'static str);

#[async_trait::async_trait]
impl FlowRuntime for StubRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        Ok(invocation.context().complete(json!(self.0)))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        Ok(json!(self.0))
    }
}

#[derive(Clone, Copy)]
struct SuspendingRuntime;

#[async_trait::async_trait]
impl FlowRuntime for SuspendingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        let resume_at = "2099-01-01T00:00:00Z"
            .parse::<DateTime<Utc>>()
            .map_err(|error| FlowError::Runtime(error.to_string()))?;
        Ok(invocation
            .context()
            .wait_until("retirement-probe", resume_at))
    }

    async fn run_step(&self, _invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        Err(FlowError::Runtime(
            "retirement probe does not execute steps".into(),
        ))
    }
}

fn workflow(name: &str, version: &str) -> WorkflowInvocation {
    WorkflowInvocation::new(
        "run-1",
        WorkflowSpec::rust_embedded(name, version, "cloud", "run"),
        json!({}),
        Vec::new(),
    )
}

fn step(name: &str) -> StepInvocation {
    StepInvocation::new("run-1", "step-1", name, json!({}), Vec::new())
}

fn router() -> FlowRuntimeRouter {
    FlowRuntimeRouter::from_registrations(production_runtime_registrations(
        Arc::new(StubRuntime("deployment")),
        Arc::new(StubRuntime("build")),
        Arc::new(StubRuntime("execution")),
        Arc::new(StubRuntime("agent_execution")),
        Arc::new(StubRuntime("workflow_run")),
        Arc::new(StubRuntime("object_namespace_recovery")),
    ))
    .expect("production Flow runtime registry must be valid")
}

#[tokio::test]
async fn runtime_router_preserves_all_production_workflow_identities() -> Result<(), FlowError> {
    for (name, version, expected) in [
        ("cloud.deployment", "1", "deployment"),
        ("cloud.deployment", "2", "deployment"),
        ("cloud.deployment", "3", "deployment"),
        ("cloud.deployment", "4", "deployment"),
        ("cloud.placement-group-deployment", "1", "deployment"),
        ("cloud.placement-group-deployment", "2", "deployment"),
        ("cloud.workload.stop", "1", "deployment"),
        ("cloud.build", "5", "build"),
        ("cloud.execution", "1", "execution"),
        ("cloud.agent-execution", "1", "agent_execution"),
        ("cloud.workflow-run", "1", "workflow_run"),
        ("cloud.workflow-run", "2", "workflow_run"),
        ("cloud.workflow-run", "3", "workflow_run"),
        ("cloud.workflow-run", "4", "workflow_run"),
        ("cloud.workflow-run", "5", "workflow_run"),
        ("cloud.workflow-run", "6", "workflow_run"),
        ("cloud.workflow-run", "7", "workflow_run"),
        (
            "cloud.object-namespace.seal",
            "1",
            "object_namespace_recovery",
        ),
        (
            "cloud.object-namespace.restore",
            "1",
            "object_namespace_recovery",
        ),
        (
            "cloud.object-namespace.delete",
            "1",
            "object_namespace_recovery",
        ),
        (
            "cloud.object-namespace.seal",
            "2",
            "object_namespace_recovery",
        ),
        (
            "cloud.object-namespace.restore",
            "2",
            "object_namespace_recovery",
        ),
        (
            "cloud.object-namespace.delete",
            "2",
            "object_namespace_recovery",
        ),
    ] {
        assert_eq!(
            router().run_workflow(workflow(name, version)).await?,
            RuntimeCommand::Complete {
                output: json!(expected)
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn runtime_router_rejects_retired_build_workflows() {
    for version in crate::modules::artifacts::application::RETIRED_BUILD_WORKFLOW_VERSIONS {
        let error = router()
            .run_workflow(workflow("cloud.build", version))
            .await
            .expect_err("retired build workflow must be rejected");
        assert_eq!(
            error.to_string(),
            format!("runtime error: Cloud has no workflow runtime for cloud.build@{version}")
        );
    }
}

#[tokio::test]
async fn startup_retires_only_known_incompatible_build_histories() -> Result<(), FlowError> {
    let engine = FlowEngine::in_memory(Arc::new(SuspendingRuntime));
    for (run_id, name, version) in [
        ("legacy-build-1", "cloud.build", "1"),
        ("legacy-build-4", "cloud.build", "4"),
        ("current-build", "cloud.build", "5"),
        ("future-build", "cloud.build", "6"),
        ("deployment", "cloud.deployment", "1"),
    ] {
        engine
            .start_with_id(
                run_id,
                WorkflowSpec::rust_embedded(name, version, "cloud", "run"),
                json!({}),
            )
            .await?;
    }

    assert_eq!(retire_incompatible_build_workflows(&engine).await?, 2);
    assert_eq!(retire_incompatible_build_workflows(&engine).await?, 0);

    for version in ["1", "4"] {
        let snapshot = engine.snapshot(&format!("legacy-build-{version}")).await?;
        assert_eq!(snapshot.status, WorkflowRunStatus::Cancelled);
        assert_eq!(
            snapshot.error.as_deref(),
            Some(
                format!(
                    "cloud.build@{version} predates the sole Box-native build workflow; rebuild with cloud.build@5"
                )
                .as_str()
            )
        );
    }
    for run_id in ["current-build", "future-build", "deployment"] {
        assert_eq!(
            engine.snapshot(run_id).await?.status,
            WorkflowRunStatus::Suspended
        );
    }

    let due = engine.list_due_waits(DateTime::<Utc>::MAX_UTC).await?;
    assert_eq!(
        due,
        vec![
            ("current-build".into(), "retirement-probe".into()),
            ("deployment".into(), "retirement-probe".into()),
            ("future-build".into(), "retirement-probe".into()),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn runtime_router_routes_every_registered_step_to_its_exact_owner() -> Result<(), FlowError> {
    async fn assert_routes(
        router: &FlowRuntimeRouter,
        step_names: impl IntoIterator<Item = &'static str>,
        expected: &str,
    ) -> Result<(), FlowError> {
        for step_name in step_names {
            assert_eq!(
                router.run_step(step(step_name)).await?,
                json!(expected),
                "step {step_name:?} routed to the wrong runtime"
            );
        }
        Ok(())
    }

    let router = router();
    assert_routes(
        &router,
        crate::modules::workloads::infrastructure::deployment_flow_step_names(),
        "deployment",
    )
    .await?;
    assert_routes(
        &router,
        crate::modules::artifacts::infrastructure::build_flow_step_names(),
        "build",
    )
    .await?;
    assert_routes(
        &router,
        crate::modules::executions::infrastructure::execution_flow_step_names(),
        "execution",
    )
    .await?;
    assert_routes(
        &router,
        crate::modules::agents::infrastructure::agent_execution_flow_step_names(),
        "agent_execution",
    )
    .await?;
    assert_routes(
        &router,
        crate::modules::workflow::infrastructure::workflow_run_flow_step_names(),
        "workflow_run",
    )
    .await?;
    assert_routes(
        &router,
        crate::modules::data::object_namespace_recovery_flow_step_names(),
        "object_namespace_recovery",
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn runtime_router_rejects_unknown_steps_instead_of_using_a_prefix_or_default_owner() {
    for step_name in [
        "build_future_step",
        "execution_future_step",
        "agent_execution_future_step",
        "workflow_run_future_step",
        "object_namespace_future_step",
        "unscoped_future_step",
    ] {
        let error = router()
            .run_step(step(step_name))
            .await
            .expect_err("unregistered step must be rejected");
        assert_eq!(
            error.to_string(),
            format!("runtime error: Cloud has no step runtime for {step_name:?}")
        );
    }
}

#[test]
fn runtime_registry_rejects_workflow_identity_collisions_at_startup() {
    let error = FlowRuntimeRouter::from_registrations([
        FlowRuntimeRegistration::new(
            "first",
            Arc::new(StubRuntime("first")),
            [("cloud.shared", "1")],
            ["first_step"],
        ),
        FlowRuntimeRegistration::new(
            "second",
            Arc::new(StubRuntime("second")),
            [("cloud.shared", "1")],
            ["second_step"],
        ),
    ])
    .err()
    .expect("duplicate workflow identity must fail registry construction");
    assert_eq!(
        error,
        FlowRuntimeRegistryError::DuplicateWorkflowIdentity {
            name: "cloud.shared".into(),
            version: "1".into(),
            first_owner: "first".into(),
            conflicting_owner: "second".into(),
        }
    );
}

#[test]
fn runtime_registry_rejects_step_name_collisions_at_startup() {
    let error = FlowRuntimeRouter::from_registrations([
        FlowRuntimeRegistration::new(
            "first",
            Arc::new(StubRuntime("first")),
            [("cloud.first", "1")],
            ["shared_step"],
        ),
        FlowRuntimeRegistration::new(
            "second",
            Arc::new(StubRuntime("second")),
            [("cloud.second", "1")],
            ["shared_step"],
        ),
    ])
    .err()
    .expect("duplicate step name must fail registry construction");
    assert_eq!(
        error,
        FlowRuntimeRegistryError::DuplicateStepName {
            step_name: "shared_step".into(),
            first_owner: "first".into(),
            conflicting_owner: "second".into(),
        }
    );
}

#[test]
fn runtime_registry_rejects_empty_registration_metadata_at_startup() {
    let empty = FlowRuntimeRouter::from_registrations(std::iter::empty())
        .err()
        .expect("empty registry must fail");
    assert_eq!(empty, FlowRuntimeRegistryError::EmptyRegistry);

    let empty_owner = FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
        "",
        Arc::new(StubRuntime("empty-owner")),
        [("cloud.valid", "1")],
        std::iter::empty(),
    )])
    .err()
    .expect("empty owner must fail");
    assert_eq!(empty_owner, FlowRuntimeRegistryError::EmptyOwner);

    let missing_workflow = FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
        "owner",
        Arc::new(StubRuntime("missing-workflow")),
        std::iter::empty(),
        ["valid_step"],
    )])
    .err()
    .expect("missing workflow identity must fail");
    assert_eq!(
        missing_workflow,
        FlowRuntimeRegistryError::MissingWorkflowIdentity {
            owner: "owner".into(),
        }
    );

    for (name, version) in [("", "1"), ("cloud.valid", "")] {
        let invalid = FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
            "owner",
            Arc::new(StubRuntime("invalid-workflow")),
            [(name, version)],
            ["valid_step"],
        )])
        .err()
        .expect("empty workflow identity component must fail");
        assert_eq!(
            invalid,
            FlowRuntimeRegistryError::InvalidWorkflowIdentity {
                owner: "owner".into(),
                name: name.into(),
                version: version.into(),
            }
        );
    }

    let empty_step = FlowRuntimeRouter::from_registrations([FlowRuntimeRegistration::new(
        "owner",
        Arc::new(StubRuntime("empty-step")),
        [("cloud.valid", "1")],
        [""],
    )])
    .err()
    .expect("empty step name must fail");
    assert_eq!(
        empty_step,
        FlowRuntimeRegistryError::InvalidStepName {
            owner: "owner".into(),
            step_name: String::new(),
        }
    );
}

#[tokio::test]
async fn runtime_router_rejects_unknown_workflow_identity() {
    let error = router()
        .run_workflow(workflow("cloud.unknown", "1"))
        .await
        .expect_err("unknown workflow must be rejected");
    assert_eq!(
        error.to_string(),
        "runtime error: Cloud has no workflow runtime for cloud.unknown@1"
    );
}

#[test]
fn component_urls_own_isolated_search_paths() -> Result<(), FlowInfrastructureError> {
    let database_url = "postgres://user:secret@localhost/cloud?application_name=a3s";
    let flow_query = scoped_postgres_url(database_url, FLOW_SCHEMA)?
        .query()
        .unwrap_or_default()
        .to_string();
    assert!(flow_query.contains("application_name=a3s"));
    assert!(flow_query.contains("options=-csearch_path%3Da3s_flow"));
    let boot_query = scoped_postgres_url(database_url, BOOT_SCHEMA)?
        .query()
        .unwrap_or_default()
        .to_string();
    assert!(boot_query.contains("application_name=a3s"));
    assert!(boot_query.contains("options=-csearch_path%3Da3s_boot"));
    assert!(matches!(
        scoped_postgres_url(
            "postgres://localhost/cloud?options=-cfoo%3Dbar",
            FLOW_SCHEMA
        ),
        Err(FlowInfrastructureError::ConflictingOptions)
    ));
    Ok(())
}
