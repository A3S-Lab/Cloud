use super::metadata;
use crate::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    NodePluginHostCapabilitiesRequest,
};
use a3s_use_core::{
    PlanScopeKind, PluginDesiredState, PluginHostApplyRequest, PluginHostApplyResult,
    PluginHostCapabilities, PluginHostEnablementPlanRequest, PluginHostEnablementPlanResult,
    PluginHostEnablementPlanStatus, PluginHostObservationRequest, PluginHostObservationResult,
    PluginHostObservationStatus, PluginHostPackageState, PluginHostPlanRequest, PluginManagedScope,
    PluginObservedState, PluginOperationAction, PluginPackageId, PluginSurfaceKind,
    PluginSurfaceRef, PLUGIN_HOST_APPLY_REQUEST_SCHEMA, PLUGIN_HOST_APPLY_RESULT_SCHEMA,
    PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA, PLUGIN_HOST_ENABLEMENT_PLAN_RESULT_SCHEMA,
    PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA, PLUGIN_HOST_OBSERVATION_RESULT_SCHEMA,
    PLUGIN_HOST_PLAN_REQUEST_SCHEMA, PLUGIN_MANAGED_SCOPE_SCHEMA_V2,
};
use chrono::{DateTime, Duration, Utc};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const HOST_CAPABILITIES_DIGEST: &str =
    "sha256:5d0b242024397727a858cc66c319396271388bb89ac36352ae5354335072f1bb";

fn plugin_capabilities() -> PluginHostCapabilities {
    PluginHostCapabilities::v6("host:node-01", "0.2.2", "use:0.2.2:linux-x86_64")
        .expect("Plugin Host capabilities")
}

fn capabilities_digest() -> String {
    plugin_capabilities()
        .descriptor_digest()
        .expect("capabilities digest")
}

fn managed_scope() -> PluginManagedScope {
    PluginManagedScope {
        schema: PLUGIN_MANAGED_SCOPE_SCHEMA_V2.into(),
        host_id: "host:node-01".into(),
        scope_kind: PlanScopeKind::Workspace,
        scope_id: "workspace:research".into(),
        authority_id: "cloud:organization-01".into(),
        fence_generation: 7,
        fence_digest: DIGEST_A.into(),
    }
}

fn package_id() -> PluginPackageId {
    PluginPackageId::parse("acme/knowledge").expect("package ID")
}

fn observed_request(assignment_generation: u64) -> PluginHostObservationRequest {
    PluginHostObservationRequest {
        schema: PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA.into(),
        request_id: "request:observe:0001".into(),
        assignment_generation,
        capabilities_digest: capabilities_digest(),
        scope: managed_scope(),
        package_id: package_id(),
    }
}

fn command(payload: NodeCommandPayload) -> NodeCommandEnvelope {
    let mut command_metadata = metadata(1);
    let issued_at_ms = Utc::now().timestamp_millis() - 1_000;
    command_metadata.issued_at = DateTime::from_timestamp_millis(issued_at_ms).expect("issued at");
    command_metadata.not_after = command_metadata.issued_at + Duration::minutes(1);
    NodeCommandEnvelope::new(command_metadata, payload).expect("Plugin Host command")
}

fn acknowledgement(
    command: &NodeCommandEnvelope,
    result: NodeCommandResult,
    completed_at: DateTime<Utc>,
) -> NodeCommandAck {
    NodeCommandAck {
        schema: NodeCommandAck::SCHEMA.into(),
        command_id: command.command_id,
        lease_id: command.lease_id,
        node_id: command.node_id,
        sequence: command.sequence,
        payload_digest: command.payload_digest.clone(),
        completed_at,
        outcome: NodeCommandOutcome::Succeeded {
            result: Box::new(result),
        },
    }
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

#[test]
fn plugin_host_commands_reuse_only_the_versioned_use_contracts() {
    plugin_capabilities().validate().expect("v6 capabilities");
    assert_eq!(capabilities_digest(), HOST_CAPABILITIES_DIGEST);

    let capability_request =
        NodePluginHostCapabilitiesRequest::new(2).expect("capabilities request");
    let capability_payload = NodeCommandPayload::PluginHostCapabilitiesInspect {
        request: capability_request,
    };
    assert_eq!(
        capability_payload.schema(),
        NodePluginHostCapabilitiesRequest::SCHEMA
    );
    assert_eq!(
        capability_payload.kind(),
        "plugin_host_capabilities_inspect"
    );
    assert_eq!(capability_payload.generation(), 2);

    let plan = PluginHostPlanRequest {
        schema: PLUGIN_HOST_PLAN_REQUEST_SCHEMA.into(),
        request_id: "request:plan:0001".into(),
        assignment_generation: 3,
        capabilities_digest: capabilities_digest(),
        scope: managed_scope(),
        action: PluginOperationAction::Uninstall,
        package_id: package_id(),
        candidate: None,
        package_lock: None,
        selected_surfaces: Vec::new(),
    };
    let plan_payload = NodeCommandPayload::PluginHostPlan {
        request: Box::new(plan),
    };
    plan_payload.validate().expect("plan payload");
    assert_eq!(plan_payload.kind(), "plugin_host_plan");
    assert_eq!(plan_payload.schema(), PLUGIN_HOST_PLAN_REQUEST_SCHEMA);
    assert_eq!(plan_payload.generation(), 3);

    let apply = PluginHostApplyRequest {
        schema: PLUGIN_HOST_APPLY_REQUEST_SCHEMA.into(),
        request_id: "request:apply:0001".into(),
        assignment_generation: 3,
        capabilities_digest: capabilities_digest(),
        scope: managed_scope(),
        package_id: package_id(),
        operation_id: "use-operation:0001".into(),
        plan_digest: DIGEST_B.into(),
        confirmation: None,
    };
    let apply_payload = NodeCommandPayload::PluginHostApply {
        request: Box::new(apply),
    };
    apply_payload.validate().expect("apply payload");
    assert_eq!(apply_payload.kind(), "plugin_host_apply");
    assert_eq!(apply_payload.schema(), PLUGIN_HOST_APPLY_REQUEST_SCHEMA);

    let enablement = PluginHostEnablementPlanRequest {
        schema: PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA.into(),
        request_id: "request:enable:0001".into(),
        assignment_generation: 4,
        capabilities_digest: capabilities_digest(),
        scope: managed_scope(),
        package_id: package_id(),
        expected_package_generation: 13,
        enabled: true,
    };
    let enablement_payload = NodeCommandPayload::PluginHostPlanEnablement {
        request: Box::new(enablement),
    };
    enablement_payload.validate().expect("enablement payload");
    assert_eq!(enablement_payload.kind(), "plugin_host_plan_enablement");
    assert_eq!(
        enablement_payload.schema(),
        PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA
    );

    let observation_payload = NodeCommandPayload::PluginHostObserve {
        request: Box::new(observed_request(4)),
    };
    observation_payload.validate().expect("observation payload");
    assert_eq!(observation_payload.kind(), "plugin_host_observe");
    assert_eq!(
        observation_payload.schema(),
        PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA
    );

    for payload in [
        capability_payload,
        plan_payload,
        apply_payload,
        enablement_payload,
        observation_payload,
    ] {
        let expected_kind = payload.kind();
        let encoded = serde_json::to_value(payload).expect("encode Plugin Host payload");
        assert_eq!(encoded["kind"], expected_kind);
        assert_ne!(encoded["kind"], "plugin_host_execute");
        assert!(encoded.get("action").is_none());
    }
    assert!(
        serde_json::from_value::<NodeCommandPayload>(serde_json::json!({
            "kind": "plugin_host_set_enablement",
            "request": {}
        }))
        .is_err()
    );
}

#[test]
fn plugin_host_acknowledgements_reject_stale_capabilities_and_result_substitution() {
    let request = observed_request(4);
    let command = command(NodeCommandPayload::PluginHostObserve {
        request: Box::new(request.clone()),
    });
    let observed_at = command.issued_at + Duration::milliseconds(100);
    let observation = PluginHostObservationResult {
        schema: PLUGIN_HOST_OBSERVATION_RESULT_SCHEMA.into(),
        request_id: request.request_id.clone(),
        assignment_generation: request.assignment_generation,
        capabilities_digest: request.capabilities_digest.clone(),
        scope: request.scope.clone(),
        package_id: request.package_id.clone(),
        observed_at_ms: u64::try_from(observed_at.timestamp_millis()).expect("observation time"),
        status: PluginHostObservationStatus::Available {
            state: installed_state(13),
        },
    };
    let acknowledgement = acknowledgement(
        &command,
        NodeCommandResult::PluginHostObserved {
            capabilities: plugin_capabilities(),
            observation: Box::new(observation.clone()),
        },
        observed_at + Duration::milliseconds(100),
    );
    acknowledgement
        .validate_against(&command)
        .expect("exact observation acknowledgement");

    let mut substituted = acknowledgement.clone();
    let NodeCommandOutcome::Succeeded { result } = &mut substituted.outcome else {
        panic!("successful acknowledgement");
    };
    let NodeCommandResult::PluginHostObserved { observation, .. } = result.as_mut() else {
        panic!("Plugin Host observation");
    };
    observation.request_id = "request:observe:substituted".into();
    assert!(substituted.validate_against(&command).is_err());

    let mut stale = acknowledgement;
    let NodeCommandOutcome::Succeeded { result } = &mut stale.outcome else {
        panic!("successful acknowledgement");
    };
    let NodeCommandResult::PluginHostObserved { capabilities, .. } = result.as_mut() else {
        panic!("Plugin Host observation");
    };
    capabilities.manager_build_id = "use:0.2.2:changed".into();
    assert!(stale.validate_against(&command).is_err());
}

#[test]
fn plugin_host_apply_and_enablement_plan_results_bind_the_exact_request_generation() {
    let apply_request = PluginHostApplyRequest {
        schema: PLUGIN_HOST_APPLY_REQUEST_SCHEMA.into(),
        request_id: "request:apply:0001".into(),
        assignment_generation: 5,
        capabilities_digest: capabilities_digest(),
        scope: managed_scope(),
        package_id: package_id(),
        operation_id: "use-operation:0001".into(),
        plan_digest: DIGEST_B.into(),
        confirmation: None,
    };
    let apply_command = command(NodeCommandPayload::PluginHostApply {
        request: Box::new(apply_request.clone()),
    });
    let apply_completed_at = apply_command.issued_at + Duration::milliseconds(100);
    let applied = PluginHostApplyResult {
        schema: PLUGIN_HOST_APPLY_RESULT_SCHEMA.into(),
        request_id: apply_request.request_id.clone(),
        assignment_generation: apply_request.assignment_generation,
        capabilities_digest: apply_request.capabilities_digest.clone(),
        scope: apply_request.scope.clone(),
        package_id: apply_request.package_id.clone(),
        operation_id: apply_request.operation_id.clone(),
        plan_digest: apply_request.plan_digest.clone(),
        completed_at_ms: u64::try_from(apply_completed_at.timestamp_millis())
            .expect("apply completion"),
        operation_result_digest: DIGEST_C.into(),
        state: installed_state(13),
        replayed: false,
    };
    acknowledgement(
        &apply_command,
        NodeCommandResult::PluginHostApplied {
            capabilities: plugin_capabilities(),
            applied: Box::new(applied),
        },
        apply_completed_at + Duration::milliseconds(100),
    )
    .validate_against(&apply_command)
    .expect("exact apply acknowledgement");

    let enablement_request = PluginHostEnablementPlanRequest {
        schema: PLUGIN_HOST_ENABLEMENT_PLAN_REQUEST_SCHEMA.into(),
        request_id: "request:enable:0001".into(),
        assignment_generation: 6,
        capabilities_digest: capabilities_digest(),
        scope: managed_scope(),
        package_id: package_id(),
        expected_package_generation: 13,
        enabled: true,
    };
    let enablement_command = command(NodeCommandPayload::PluginHostPlanEnablement {
        request: Box::new(enablement_request.clone()),
    });
    let enablement_planned_at = enablement_command.issued_at + Duration::milliseconds(100);
    let enablement_plan = PluginHostEnablementPlanResult {
        schema: PLUGIN_HOST_ENABLEMENT_PLAN_RESULT_SCHEMA.into(),
        request_id: enablement_request.request_id.clone(),
        assignment_generation: enablement_request.assignment_generation,
        capabilities_digest: enablement_request.capabilities_digest.clone(),
        scope: enablement_request.scope.clone(),
        package_id: enablement_request.package_id.clone(),
        expected_package_generation: enablement_request.expected_package_generation,
        enabled: enablement_request.enabled,
        planned_at_ms: u64::try_from(enablement_planned_at.timestamp_millis())
            .expect("enablement plan creation"),
        status: PluginHostEnablementPlanStatus::NoChange,
        state: installed_state(13),
        plan: None,
        replayed: false,
    };
    acknowledgement(
        &enablement_command,
        NodeCommandResult::PluginHostEnablementPlanned {
            capabilities: plugin_capabilities(),
            enablement_plan: Box::new(enablement_plan),
        },
        enablement_planned_at + Duration::milliseconds(100),
    )
    .validate_against(&enablement_command)
    .expect("exact enablement plan acknowledgement");
}

#[test]
fn plugin_host_capabilities_require_the_current_command_acknowledgement_schema() {
    let command = command(NodeCommandPayload::PluginHostCapabilitiesInspect {
        request: NodePluginHostCapabilitiesRequest::new(1).expect("capabilities request"),
    });
    let mut acknowledgement = acknowledgement(
        &command,
        NodeCommandResult::PluginHostCapabilitiesInspected {
            capabilities: plugin_capabilities(),
        },
        command.issued_at + Duration::milliseconds(100),
    );
    acknowledgement
        .validate_against(&command)
        .expect("current acknowledgement");
    acknowledgement.schema = NodeCommandAck::LEGACY_SCHEMA.into();
    assert!(acknowledgement.validate_against(&command).is_err());
}
