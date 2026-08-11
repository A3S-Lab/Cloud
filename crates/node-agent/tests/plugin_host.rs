use a3s_cloud_contracts::{
    NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult, NodePluginHostCapabilitiesRequest,
};
use a3s_cloud_node_agent::{CommandExecutor, FileCommandJournal};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
    RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation,
    RuntimeRemoval,
};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use a3s_use_core::{
    PlanActor, PlanAuthority, PlanPackageChangeKind, PlanPackageRole, PlanPolicyDecision,
    PlanScope, PlanScopeKind, PlannedOperationImpact, PlannedPackageTransition,
    PlannedStateEvidence, PlannedWorkspaceImpact, PluginCatalogRecord, PluginDesiredState,
    PluginHostApplyRequest, PluginHostApplyResult, PluginHostCapabilities,
    PluginHostEnablementPlanRequest, PluginHostEnablementPlanResult,
    PluginHostEnablementPlanStatus, PluginHostManager, PluginHostObservationRequest,
    PluginHostObservationResult, PluginHostObservationStatus, PluginHostPackageState,
    PluginHostPlanRequest, PluginHostPlanResult, PluginManagedScope, PluginObservedState,
    PluginOperationAction, PluginOperationPlanBinding, PluginOperationPlanDraft,
    PluginOperationPlanEnvelope, PluginPackageId, PluginSurfaceKind, PluginSurfaceRef, UseError,
    UseResult, VerifiedCatalogProvenance, VerifiedPluginCatalogRecord,
    PLUGIN_HOST_APPLY_REQUEST_SCHEMA, PLUGIN_HOST_APPLY_RESULT_SCHEMA,
    PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA, PLUGIN_HOST_ENABLEMENT_PLAN_RESULT_SCHEMA,
    PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA, PLUGIN_HOST_OBSERVATION_RESULT_SCHEMA,
    PLUGIN_HOST_PLAN_REQUEST_SCHEMA, PLUGIN_HOST_PLAN_RESULT_SCHEMA, PLUGIN_MANAGED_SCOPE_SCHEMA,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

const CATALOG: &[u8] = include_bytes!("fixtures/plugin-catalog-okf-v3.json");
const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

struct UnusedRuntime;

#[async_trait]
impl RuntimeClient for UnusedRuntime {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Err(RuntimeError::Protocol("unused capabilities call".into()))
    }

    async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        Err(RuntimeError::Protocol("unused apply call".into()))
    }

    async fn inspect(&self, _unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        Err(RuntimeError::Protocol("unused inspect call".into()))
    }

    async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        Err(RuntimeError::Protocol("unused stop call".into()))
    }

    async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        Err(RuntimeError::Protocol("unused remove call".into()))
    }

    async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        Err(RuntimeError::Protocol("unused logs call".into()))
    }

    async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        Err(RuntimeError::Protocol("unused exec call".into()))
    }
}

#[derive(Default)]
struct ManagerCalls {
    capabilities: AtomicUsize,
    plan: AtomicUsize,
    apply: AtomicUsize,
    enablement_plan: AtomicUsize,
    observe: AtomicUsize,
}

struct RecordingPluginHostManager {
    capabilities: PluginHostCapabilities,
    calls: ManagerCalls,
}

impl RecordingPluginHostManager {
    fn new() -> Self {
        Self {
            capabilities: plugin_capabilities(),
            calls: ManagerCalls::default(),
        }
    }
}

#[async_trait]
impl PluginHostManager for RecordingPluginHostManager {
    async fn capabilities(&self) -> UseResult<PluginHostCapabilities> {
        self.calls.capabilities.fetch_add(1, Ordering::SeqCst);
        Ok(self.capabilities.clone())
    }

    async fn plan(&self, request: PluginHostPlanRequest) -> UseResult<PluginHostPlanResult> {
        self.calls.plan.fetch_add(1, Ordering::SeqCst);
        let candidate = request.candidate.as_ref().ok_or_else(|| {
            UseError::new(
                "use.plugin.test_candidate_missing",
                "The test manager requires an install candidate.",
            )
        })?;
        let transition =
            candidate.install_transition(PlanPackageRole::Root, &request.selected_surfaces)?;
        let draft = PluginOperationPlanDraft::new(
            PluginOperationAction::Install,
            request.package_id.as_str(),
            request.package_id.component_id(),
            vec![transition],
            Vec::new(),
            vec![PlannedWorkspaceImpact {
                scope_id: request.scope.scope_id.clone(),
                grant_before_digest: None,
                grant_after_digest: Some(DIGEST_B.into()),
                enabled_before: false,
                enabled_after: true,
            }],
            PlannedOperationImpact {
                download_bytes: candidate.record.archive.length,
                installed_bytes_after: candidate.record.package.expanded_bytes,
                reclaimed_bytes: 0,
                drain_required: false,
                retained_data: false,
                okf_changes: Vec::new(),
            },
            PlannedStateEvidence {
                state_revision: 3,
                capability_generation: 12,
                receipt_digest: None,
            },
        )?;
        let created_at_ms = now_ms();
        let plan = draft.bind(PluginOperationPlanBinding {
            operation_id: "use-operation:plan:0001".into(),
            created_at_ms,
            expires_at_ms: created_at_ms + 10 * 60 * 1_000,
            scope: PlanScope {
                kind: PlanScopeKind::Workspace,
                id: request.scope.scope_id.clone(),
            },
            authority: PlanAuthority {
                actor: PlanActor::User,
                decision: PlanPolicyDecision::Allow,
                policy_digest: DIGEST_C.into(),
                confirmation_required: false,
            },
        })?;
        Ok(PluginHostPlanResult {
            schema: PLUGIN_HOST_PLAN_RESULT_SCHEMA.into(),
            request_id: request.request_id,
            assignment_generation: request.assignment_generation,
            capabilities_digest: request.capabilities_digest,
            scope: request.scope,
            package_id: request.package_id,
            plan: PluginOperationPlanEnvelope::new(plan)?,
            replayed: false,
        })
    }

    async fn apply(&self, request: PluginHostApplyRequest) -> UseResult<PluginHostApplyResult> {
        self.calls.apply.fetch_add(1, Ordering::SeqCst);
        Ok(PluginHostApplyResult {
            schema: PLUGIN_HOST_APPLY_RESULT_SCHEMA.into(),
            request_id: request.request_id,
            assignment_generation: request.assignment_generation,
            capabilities_digest: request.capabilities_digest,
            scope: request.scope,
            package_id: request.package_id,
            operation_id: request.operation_id,
            plan_digest: request.plan_digest,
            completed_at_ms: now_ms(),
            operation_result_digest: DIGEST_A.into(),
            state: installed_state(13),
            replayed: false,
        })
    }

    async fn plan_enablement(
        &self,
        request: PluginHostEnablementPlanRequest,
    ) -> UseResult<PluginHostEnablementPlanResult> {
        self.calls.enablement_plan.fetch_add(1, Ordering::SeqCst);
        let state = installed_state(request.expected_package_generation);
        let planned_at_ms = now_ms();
        if request.enabled {
            return Ok(PluginHostEnablementPlanResult {
                schema: PLUGIN_HOST_ENABLEMENT_PLAN_RESULT_SCHEMA.into(),
                request_id: request.request_id,
                assignment_generation: request.assignment_generation,
                capabilities_digest: request.capabilities_digest,
                scope: request.scope,
                package_id: request.package_id,
                expected_package_generation: request.expected_package_generation,
                enabled: true,
                planned_at_ms,
                status: PluginHostEnablementPlanStatus::NoChange,
                state,
                plan: None,
                replayed: false,
            });
        }

        let catalog = candidate();
        let selected = catalog.selected_state(&[])?;
        let transition = PlannedPackageTransition::resolved(
            request.package_id.as_str(),
            PlanPackageRole::Root,
            PlanPackageChangeKind::Retain,
            Some(selected.clone()),
            Some(selected),
            None,
        )?;
        let draft = PluginOperationPlanDraft::new(
            PluginOperationAction::Disable,
            request.package_id.as_str(),
            request.package_id.component_id(),
            vec![transition],
            Vec::new(),
            vec![PlannedWorkspaceImpact {
                scope_id: request.scope.scope_id.clone(),
                grant_before_digest: Some(DIGEST_B.into()),
                grant_after_digest: None,
                enabled_before: true,
                enabled_after: false,
            }],
            PlannedOperationImpact {
                download_bytes: 0,
                installed_bytes_after: catalog.record.package.expanded_bytes,
                reclaimed_bytes: 0,
                drain_required: false,
                retained_data: true,
                okf_changes: Vec::new(),
            },
            PlannedStateEvidence {
                state_revision: 3,
                capability_generation: state.capability_generation,
                receipt_digest: state.receipt_digest.clone(),
            },
        )?;
        let plan = draft.bind(PluginOperationPlanBinding {
            operation_id: "use-enablement-operation:0001".into(),
            created_at_ms: planned_at_ms,
            expires_at_ms: planned_at_ms + 10 * 60 * 1_000,
            scope: request.scope.plan_scope(),
            authority: PlanAuthority {
                actor: PlanActor::User,
                decision: PlanPolicyDecision::Allow,
                policy_digest: DIGEST_C.into(),
                confirmation_required: false,
            },
        })?;
        Ok(PluginHostEnablementPlanResult {
            schema: PLUGIN_HOST_ENABLEMENT_PLAN_RESULT_SCHEMA.into(),
            request_id: request.request_id,
            assignment_generation: request.assignment_generation,
            capabilities_digest: request.capabilities_digest,
            scope: request.scope,
            package_id: request.package_id,
            expected_package_generation: request.expected_package_generation,
            enabled: false,
            planned_at_ms,
            status: PluginHostEnablementPlanStatus::Planned,
            state,
            plan: Some(PluginOperationPlanEnvelope::new(plan)?),
            replayed: false,
        })
    }

    async fn observe(
        &self,
        request: PluginHostObservationRequest,
    ) -> UseResult<PluginHostObservationResult> {
        self.calls.observe.fetch_add(1, Ordering::SeqCst);
        Ok(PluginHostObservationResult {
            schema: PLUGIN_HOST_OBSERVATION_RESULT_SCHEMA.into(),
            request_id: request.request_id,
            assignment_generation: request.assignment_generation,
            capabilities_digest: request.capabilities_digest,
            scope: request.scope,
            package_id: request.package_id,
            observed_at_ms: now_ms(),
            status: PluginHostObservationStatus::Available {
                state: installed_state(13),
            },
        })
    }
}

fn plugin_capabilities() -> PluginHostCapabilities {
    PluginHostCapabilities::v4("host:node-01", "0.2.2", "use:0.2.2:linux-x86_64")
        .expect("Plugin Host capabilities")
}

fn managed_scope() -> PluginManagedScope {
    PluginManagedScope {
        schema: PLUGIN_MANAGED_SCOPE_SCHEMA.into(),
        host_id: "host:node-01".into(),
        scope_id: "workspace:research".into(),
        authority_id: "cloud:organization-01".into(),
        fence_generation: 7,
        fence_digest: DIGEST_A.into(),
    }
}

fn package_id() -> PluginPackageId {
    PluginPackageId::parse("acme/knowledge").expect("package ID")
}

fn candidate() -> VerifiedPluginCatalogRecord {
    let record = PluginCatalogRecord::from_json(CATALOG).expect("catalog fixture");
    let catalog_record_digest = record.descriptor_digest().expect("catalog digest");
    VerifiedPluginCatalogRecord::new(
        record,
        VerifiedCatalogProvenance {
            registry_name: "official".into(),
            registry_url: "https://plugins.a3s.dev/catalog".into(),
            root_sha256: DIGEST_D.into(),
            root_version: 7,
            timestamp_version: 42,
            snapshot_version: 41,
            targets_version: 39,
            catalog_record_digest,
        },
    )
    .expect("verified catalog record")
}

fn installed_state(package_generation: u64) -> PluginHostPackageState {
    PluginHostPackageState {
        version: Some("1.0.0".into()),
        package_generation: Some(package_generation),
        package_digest: Some(DIGEST_A.into()),
        manifest_digest: Some(DIGEST_B.into()),
        receipt_digest: Some(DIGEST_C.into()),
        capability_generation: 14,
        capability_revision: DIGEST_D.into(),
        desired: PluginDesiredState::Enabled,
        observed: PluginObservedState::Ready,
        selected_surfaces: vec![PluginSurfaceRef {
            kind: PluginSurfaceKind::Skill,
            id: "research".into(),
        }],
    }
}

fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).expect("current time")
}

fn capabilities_digest() -> String {
    plugin_capabilities()
        .descriptor_digest()
        .expect("capabilities digest")
}

fn envelope(
    node_id: Uuid,
    aggregate_id: Uuid,
    sequence: u64,
    payload: NodeCommandPayload,
) -> NodeCommandEnvelope {
    let issued_at = Utc::now() - Duration::milliseconds(10);
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
    .expect("Plugin Host command")
}

fn executor(
    root: &std::path::Path,
    node_id: Uuid,
    manager: Option<Arc<RecordingPluginHostManager>>,
) -> CommandExecutor {
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(root, node_id).expect("command journal"),
        Arc::new(UnusedRuntime),
    );
    match manager {
        Some(manager) => executor.with_plugin_host(manager),
        None => executor,
    }
}

#[tokio::test]
async fn capabilities_inspection_uses_the_existing_journal_for_exact_replay() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let manager = Arc::new(RecordingPluginHostManager::new());
    let executor = executor(directory.path(), node_id, Some(manager.clone()));
    let command = envelope(
        node_id,
        node_id,
        1,
        NodeCommandPayload::PluginHostCapabilitiesInspect {
            request: NodePluginHostCapabilitiesRequest::new(1).expect("capabilities request"),
        },
    );

    let acknowledgement = executor
        .execute(command.clone())
        .await
        .expect("capabilities inspection");
    let NodeCommandOutcome::Succeeded { result } = &acknowledgement.outcome else {
        panic!("capabilities inspection must succeed");
    };
    assert!(matches!(
        result.as_ref(),
        NodeCommandResult::PluginHostCapabilitiesInspected { .. }
    ));

    let mut redelivered = command;
    redelivered.lease_id = Uuid::now_v7();
    let replayed = executor.execute(redelivered).await.expect("journal replay");
    assert_eq!(replayed.outcome, acknowledgement.outcome);
    assert_eq!(manager.calls.capabilities.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_plugin_manager_is_explicitly_rejected_without_another_adapter() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let executor = executor(directory.path(), node_id, None);
    let command = envelope(
        node_id,
        node_id,
        1,
        NodeCommandPayload::PluginHostCapabilitiesInspect {
            request: NodePluginHostCapabilitiesRequest::new(1).expect("capabilities request"),
        },
    );

    let acknowledgement = executor
        .execute(command)
        .await
        .expect("unavailable manager result");
    let NodeCommandOutcome::Rejected { failure } = acknowledgement.outcome else {
        panic!("missing Plugin Manager must be rejected");
    };
    assert_eq!(failure.code, "plugin_host_unavailable");
    assert!(!failure.retryable);
}

#[tokio::test]
async fn stale_capabilities_fail_before_plan_delegation_and_replay_the_same_failure() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let assignment_id = Uuid::now_v7();
    let manager = Arc::new(RecordingPluginHostManager::new());
    let executor = executor(directory.path(), node_id, Some(manager.clone()));
    let request = PluginHostPlanRequest {
        schema: PLUGIN_HOST_PLAN_REQUEST_SCHEMA.into(),
        request_id: "request:plan:stale".into(),
        assignment_generation: 1,
        capabilities_digest: DIGEST_B.into(),
        scope: managed_scope(),
        action: PluginOperationAction::Install,
        package_id: package_id(),
        candidate: Some(candidate()),
        package_lock: None,
        selected_surfaces: Vec::new(),
    };
    let command = envelope(
        node_id,
        assignment_id,
        1,
        NodeCommandPayload::PluginHostPlan {
            request: Box::new(request),
        },
    );

    let acknowledgement = executor
        .execute(command.clone())
        .await
        .expect("stale capability failure");
    let NodeCommandOutcome::Failed { failure } = &acknowledgement.outcome else {
        panic!("stale capabilities must fail");
    };
    assert_eq!(failure.code, "use.plugin.host_capabilities_mismatch");
    assert_eq!(manager.calls.capabilities.load(Ordering::SeqCst), 1);
    assert_eq!(manager.calls.plan.load(Ordering::SeqCst), 0);

    let mut redelivered = command;
    redelivered.lease_id = Uuid::now_v7();
    let replayed = executor.execute(redelivered).await.expect("failure replay");
    assert_eq!(replayed.outcome, acknowledgement.outcome);
    assert_eq!(manager.calls.capabilities.load(Ordering::SeqCst), 1);
    assert_eq!(manager.calls.plan.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn same_generation_plugin_stages_dispatch_through_only_the_shared_manager_port() {
    let directory = tempfile::tempdir().expect("journal directory");
    let node_id = Uuid::now_v7();
    let assignment_id = Uuid::now_v7();
    let manager = Arc::new(RecordingPluginHostManager::new());
    let executor = executor(directory.path(), node_id, Some(manager.clone()));
    let capabilities_digest = capabilities_digest();

    let plan_request = PluginHostPlanRequest {
        schema: PLUGIN_HOST_PLAN_REQUEST_SCHEMA.into(),
        request_id: "request:plan:0001".into(),
        assignment_generation: 1,
        capabilities_digest: capabilities_digest.clone(),
        scope: managed_scope(),
        action: PluginOperationAction::Install,
        package_id: package_id(),
        candidate: Some(candidate()),
        package_lock: None,
        selected_surfaces: Vec::new(),
    };
    let plan_ack = executor
        .execute(envelope(
            node_id,
            assignment_id,
            1,
            NodeCommandPayload::PluginHostPlan {
                request: Box::new(plan_request),
            },
        ))
        .await
        .expect("plan command");
    let NodeCommandOutcome::Succeeded { result } = plan_ack.outcome else {
        panic!("plan command must succeed");
    };
    let NodeCommandResult::PluginHostPlanned { plan, .. } = result.as_ref() else {
        panic!("plan result");
    };

    let apply_request = PluginHostApplyRequest {
        schema: PLUGIN_HOST_APPLY_REQUEST_SCHEMA.into(),
        request_id: "request:apply:0001".into(),
        assignment_generation: 1,
        capabilities_digest: capabilities_digest.clone(),
        scope: managed_scope(),
        package_id: package_id(),
        operation_id: plan.plan.plan.operation_id.clone(),
        plan_digest: plan.plan.plan_digest.clone(),
        confirmation: None,
    };
    let apply_ack = executor
        .execute(envelope(
            node_id,
            assignment_id,
            2,
            NodeCommandPayload::PluginHostApply {
                request: Box::new(apply_request),
            },
        ))
        .await
        .expect("apply command");
    assert!(matches!(
        apply_ack.outcome,
        NodeCommandOutcome::Succeeded { result }
            if matches!(result.as_ref(), NodeCommandResult::PluginHostApplied { .. })
    ));

    let enablement_request = PluginHostEnablementPlanRequest {
        schema: PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA.into(),
        request_id: "request:enable:0001".into(),
        assignment_generation: 1,
        capabilities_digest: capabilities_digest.clone(),
        scope: managed_scope(),
        package_id: package_id(),
        expected_package_generation: 13,
        enabled: false,
    };
    let enablement_command = envelope(
        node_id,
        assignment_id,
        3,
        NodeCommandPayload::PluginHostPlanEnablement {
            request: Box::new(enablement_request),
        },
    );
    let enablement_ack = executor
        .execute(enablement_command.clone())
        .await
        .expect("enablement plan command");
    let NodeCommandOutcome::Succeeded { result } = &enablement_ack.outcome else {
        panic!("same-generation enablement planning must succeed after apply");
    };
    let NodeCommandResult::PluginHostEnablementPlanned {
        enablement_plan, ..
    } = result.as_ref()
    else {
        panic!("enablement plan result");
    };
    assert_eq!(enablement_plan.assignment_generation, 1);
    assert_eq!(
        enablement_plan.status,
        PluginHostEnablementPlanStatus::Planned
    );
    assert_eq!(enablement_plan.state.desired, PluginDesiredState::Enabled);
    assert_eq!(
        enablement_plan.plan.as_ref().map(|plan| plan.plan.action),
        Some(PluginOperationAction::Disable)
    );

    let mut redelivered_enablement = enablement_command;
    redelivered_enablement.lease_id = Uuid::now_v7();
    let replayed_enablement = executor
        .execute(redelivered_enablement)
        .await
        .expect("enablement plan journal replay");
    assert_eq!(replayed_enablement.outcome, enablement_ack.outcome);
    assert_eq!(manager.calls.enablement_plan.load(Ordering::SeqCst), 1);

    let observation_request = PluginHostObservationRequest {
        schema: PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA.into(),
        request_id: "request:observe:0001".into(),
        assignment_generation: 1,
        capabilities_digest,
        scope: managed_scope(),
        package_id: package_id(),
    };
    let observation_ack = executor
        .execute(envelope(
            node_id,
            assignment_id,
            4,
            NodeCommandPayload::PluginHostObserve {
                request: Box::new(observation_request),
            },
        ))
        .await
        .expect("observation command");
    assert!(matches!(
        observation_ack.outcome,
        NodeCommandOutcome::Succeeded { result }
            if matches!(result.as_ref(), NodeCommandResult::PluginHostObserved { .. })
    ));

    assert_eq!(manager.calls.capabilities.load(Ordering::SeqCst), 4);
    assert_eq!(manager.calls.plan.load(Ordering::SeqCst), 1);
    assert_eq!(manager.calls.apply.load(Ordering::SeqCst), 1);
    assert_eq!(manager.calls.enablement_plan.load(Ordering::SeqCst), 1);
    assert_eq!(manager.calls.observe.load(Ordering::SeqCst), 1);
}
