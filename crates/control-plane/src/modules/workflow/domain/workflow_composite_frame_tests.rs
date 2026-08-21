use super::*;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, OntologyId, OntologyRevisionId, OrganizationId, PlanRevisionId,
    ProjectId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId, WorkflowRunId,
};
use serde_json::json;
use std::collections::BTreeMap;

pub(super) struct Fixture {
    pub(super) plan: WorkflowPlan,
    pub(super) regions: WorkflowCompositeRegions,
    pub(super) variables: WorkflowVariableContract,
    pub(super) request: WorkflowCompositeFrameRequest,
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

pub(super) fn plan_digest(plan: &WorkflowPlan) -> Sha256Digest {
    Sha256Digest::from_bytes(
        &canonical_json_bounded(plan, WORKFLOW_PLAN_MAX_BYTES, "test plan").expect("plan bytes"),
    )
}

fn descriptor(step_id: &str, descriptor_id: &str) -> WorkflowStepDescriptorBinding {
    WorkflowStepDescriptorBinding {
        step_id: step_id.into(),
        descriptor_id: descriptor_id.into(),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: digest('f'),
    }
}

fn declaration(
    name: &str,
    scope: WorkflowVariableScope,
    value_type: WorkflowDataType,
    value_schema: char,
) -> WorkflowVariableDeclaration {
    WorkflowVariableDeclaration {
        name: name.into(),
        scope,
        value_type,
        value_schema_digest: digest(value_schema),
        source_schema_digest: None,
        storage_class: WorkflowVariableStorageClass::Inline,
        mutation_mode: WorkflowVariableMutationMode::Immutable,
        required: true,
        source_step_id: None,
        source_path: Vec::new(),
        region_id: None,
        default_value_digest: None,
    }
}

pub(super) fn fixture() -> Fixture {
    let definition_id = WorkflowDefinitionId::new();
    let child_definition_id = WorkflowDefinitionId::new();
    let child_revision_id = WorkflowRevisionId::new();
    let workflow_revision_id = WorkflowRevisionId::new();

    let regions = WorkflowCompositeRegions::from_spec(WorkflowCompositeRegionsSpec {
        id: "support.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        regions: vec![WorkflowCompositeRegionPolicy::Iteration(
            WorkflowIterationRegionPolicy {
                step_id: "iteration".into(),
                maximum_items: 2,
                maximum_concurrency: 2,
                failure_mode: WorkflowIterationFailureMode::Terminate,
            },
        )],
    })
    .expect("regions");

    let mut request = declaration(
        "request",
        WorkflowVariableScope::InvocationInput,
        WorkflowDataType::Object,
        'a',
    );
    request.source_schema_digest = Some(digest('a'));

    let mut item = declaration(
        "item",
        WorkflowVariableScope::CompositeLocal,
        WorkflowDataType::String,
        'c',
    );
    item.source_schema_digest = Some(digest('a'));
    item.source_step_id = Some("iteration".into());
    item.source_path = vec!["item".into()];
    item.region_id = Some("iteration".into());

    let mut raw_result = declaration(
        "raw_result",
        WorkflowVariableScope::NodeOutput,
        WorkflowDataType::String,
        'c',
    );
    raw_result.source_schema_digest = Some(digest('c'));
    raw_result.source_step_id = Some("iteration".into());

    let mut normalized = declaration(
        "normalized",
        WorkflowVariableScope::CompositeLocal,
        WorkflowDataType::String,
        'c',
    );
    normalized.mutation_mode = WorkflowVariableMutationMode::Deterministic;
    normalized.region_id = Some("iteration".into());

    let mut iteration_result = declaration(
        "iteration_result",
        WorkflowVariableScope::NodeOutput,
        WorkflowDataType::String,
        'c',
    );
    iteration_result.source_schema_digest = Some(digest('c'));
    iteration_result.source_step_id = Some("iteration".into());

    let mut summary = declaration(
        "summary",
        WorkflowVariableScope::Run,
        WorkflowDataType::String,
        'c',
    );
    summary.mutation_mode = WorkflowVariableMutationMode::Deterministic;

    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        declarations: vec![
            request,
            item,
            raw_result,
            normalized,
            iteration_result,
            summary,
        ],
        reads: vec![WorkflowVariableRead {
            id: "iteration-item".into(),
            variable: "item".into(),
            consumer_step_id: "iteration".into(),
            consumer_region_id: Some("iteration".into()),
            target_port: "item".into(),
            path: Vec::new(),
            expected_type: WorkflowDataType::String,
            expected_schema_digest: digest('c'),
            required: true,
            mode: WorkflowVariableReadMode::DirectValue,
        }],
        assignments: vec![
            WorkflowVariableAssignment {
                id: "normalize-result".into(),
                target_variable: "normalized".into(),
                source_variable: "raw_result".into(),
                writer_step_id: "iteration".into(),
                writer_region_id: Some("iteration".into()),
                source_path: Vec::new(),
                value_type: WorkflowDataType::String,
                value_schema_digest: digest('c'),
                mutation_order: 1,
                expected_revision_variable: None,
                idempotency_key_variable: None,
            },
            WorkflowVariableAssignment {
                id: "update-summary".into(),
                target_variable: "summary".into(),
                source_variable: "raw_result".into(),
                writer_step_id: "iteration".into(),
                writer_region_id: None,
                source_path: Vec::new(),
                value_type: WorkflowDataType::String,
                value_schema_digest: digest('c'),
                mutation_order: 1,
                expected_revision_variable: None,
                idempotency_key_variable: None,
            },
        ],
        exports: vec![WorkflowVariableExport {
            id: "export-result".into(),
            region_id: "iteration".into(),
            source_variable: "normalized".into(),
            target_variable: "iteration_result".into(),
            source_path: Vec::new(),
            value_type: WorkflowDataType::String,
            value_schema_digest: digest('c'),
        }],
    })
    .expect("variables");

    let plan = WorkflowPlan {
        schema: WORKFLOW_PLAN_SCHEMA_V2.into(),
        compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION_V2.into(),
        workflow_definition_id: definition_id,
        workflow_revision_id,
        workflow_digest: digest('1'),
        workflow_payload_set_digest: digest('2'),
        semantic_contract_set_digest: Some(digest('3')),
        variable_contract_digest: Some(variables.digest().clone()),
        composite_regions_digest: Some(regions.digest().clone()),
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: digest('4'),
        environment_id: None,
        input_digest: digest('5'),
        steps: vec![
            WorkflowPlanStep {
                id: "input".into(),
                kind: WorkflowStepKind::Input,
                configuration_digest: digest('6'),
                input_schema_digest: digest('a'),
                output_schema_digest: digest('a'),
                policy_digest: None,
                capability: None,
                descriptor: Some(descriptor("input", "workflow.input")),
                failure: None,
                default_output: None,
            },
            WorkflowPlanStep {
                id: "iteration".into(),
                kind: WorkflowStepKind::Subworkflow,
                configuration_digest: digest('7'),
                input_schema_digest: digest('a'),
                output_schema_digest: digest('c'),
                policy_digest: None,
                capability: Some(CapabilityReference {
                    owner: CapabilityOwner::Workflow,
                    capability_type: CapabilityType::WorkflowRevision,
                    resource_id: child_definition_id.as_uuid(),
                    revision: child_revision_id.to_string(),
                    digest: digest('8'),
                    capability: "workflow.run".into(),
                }),
                descriptor: Some(descriptor("iteration", "workflow.iteration")),
                failure: None,
                default_output: None,
            },
            WorkflowPlanStep {
                id: "output".into(),
                kind: WorkflowStepKind::Output,
                configuration_digest: digest('9'),
                input_schema_digest: digest('c'),
                output_schema_digest: digest('c'),
                policy_digest: None,
                capability: None,
                descriptor: Some(descriptor("output", "workflow.output")),
                failure: None,
                default_output: None,
            },
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-iteration".into(),
                source: "input".into(),
                target: "iteration".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "iteration-output".into(),
                source: "iteration".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };
    plan.validate().expect("plan");
    variables
        .validate_graph_bindings(&plan.workflow_spec().expect("workflow spec"))
        .expect("variable graph");
    let plan_digest = plan_digest(&plan);

    Fixture {
        plan,
        regions,
        variables,
        request: WorkflowCompositeFrameRequest {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            workflow_run_id: WorkflowRunId::new(),
            plan_revision_id: PlanRevisionId::new(),
            plan_digest,
            region_step_id: "iteration".into(),
            ordinal: 0,
            effective_input: json!({"item": "A"}),
            available_variables: BTreeMap::from([("request".into(), json!({"items": ["A", "B"]}))]),
        },
    }
}

#[test]
fn frame_materializes_exact_inputs_assignments_and_exports() {
    let fixture = fixture();
    validate_runtime_variable_contract(&fixture.variables, None, &fixture.plan)
        .expect("frame and export contract is admitted before Flow dispatch");
    let frame = WorkflowCompositeFrame::open(
        fixture.request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("frame");

    assert_eq!(frame.mode, WorkflowCompositeFrameMode::Iteration);
    assert_eq!(frame.ordinal, 0);
    assert!(frame.typed_projection_authoritative);
    assert_eq!(frame.child_input, json!({"item": "A"}));
    assert_eq!(frame.captured_variables.get("item"), Some(&json!("A")));
    frame
        .validate(&fixture.plan, &fixture.regions, &fixture.variables)
        .expect("valid frame");

    let result = frame
        .resolve(
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            json!("A!"),
        )
        .expect("frame result");
    assert_eq!(
        result.local_variables,
        BTreeMap::from([
            ("item".into(), json!("A")),
            ("normalized".into(), json!("A!")),
        ])
    );
    assert_eq!(
        result.exported_variables,
        BTreeMap::from([("iteration_result".into(), json!("A!"))])
    );
    assert_eq!(
        result.run_variable_updates,
        BTreeMap::from([("summary".into(), json!("A!"))])
    );
    result
        .validate(&frame, &fixture.variables)
        .expect("valid result");
}

#[test]
fn frame_allows_unrelated_application_variables_but_rejects_region_access() {
    let mut fixture = fixture();
    let mut spec = fixture.variables.spec().clone();
    let mut application = declaration(
        "conversation_topic",
        WorkflowVariableScope::Application,
        WorkflowDataType::String,
        'c',
    );
    application.mutation_mode = WorkflowVariableMutationMode::OptimisticApplicationPort;
    application.required = false;
    spec.declarations.push(application);
    fixture.variables = WorkflowVariableContract::from_spec(spec.clone())
        .expect("contract with unrelated Application variable");
    fixture.plan.variable_contract_digest = Some(fixture.variables.digest().clone());
    fixture.request.plan_digest = plan_digest(&fixture.plan);

    WorkflowCompositeFrame::open(
        fixture.request.clone(),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("unrelated Application authority remains outside the frame");

    spec.reads.push(WorkflowVariableRead {
        id: "iteration-application-variable".into(),
        variable: "conversation_topic".into(),
        consumer_step_id: "iteration".into(),
        consumer_region_id: Some("iteration".into()),
        target_port: "applicationValue".into(),
        path: Vec::new(),
        expected_type: WorkflowDataType::String,
        expected_schema_digest: digest('c'),
        required: false,
        mode: WorkflowVariableReadMode::ApplicationPort,
    });
    fixture.variables =
        WorkflowVariableContract::from_spec(spec).expect("Application read contract");
    fixture.plan.variable_contract_digest = Some(fixture.variables.digest().clone());
    fixture.request.plan_digest = plan_digest(&fixture.plan);
    let error = WorkflowCompositeFrame::open(
        fixture.request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect_err("Application variable crossing the region boundary");
    assert!(error.contains("Application variable"), "{error}");
}

#[test]
fn frame_is_replay_stable_and_detects_stored_state_drift() {
    let fixture = fixture();
    let first = WorkflowCompositeFrame::open(
        fixture.request.clone(),
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("first frame");
    let replay = WorkflowCompositeFrame::open(
        fixture.request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("replayed frame");
    assert_eq!(first, replay);

    let encoded = serde_json::to_value(&first).expect("frame JSON");
    let restored: WorkflowCompositeFrame =
        serde_json::from_value(encoded).expect("restored frame JSON");
    assert_eq!(restored, first);
    restored
        .validate(&fixture.plan, &fixture.regions, &fixture.variables)
        .expect("restored frame");

    let mut drifted = restored;
    drifted.captured_variables.insert("item".into(), json!("B"));
    assert!(drifted
        .validate(&fixture.plan, &fixture.regions, &fixture.variables)
        .is_err());
}

#[test]
fn frame_rejects_policy_digest_bounds_and_required_input_drift() {
    let fixture = fixture();

    let mut outside_bound = fixture.request.clone();
    outside_bound.ordinal = 2;
    assert!(WorkflowCompositeFrame::open(
        outside_bound,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .is_err());

    let mut plan_drift = fixture.request.clone();
    plan_drift.plan_digest = digest('0');
    assert!(WorkflowCompositeFrame::open(
        plan_drift,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .is_err());

    let mut missing_item = fixture.request;
    missing_item.effective_input = json!({});
    assert!(WorkflowCompositeFrame::open(
        missing_item,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .is_err());
}

#[test]
fn loop_frames_use_the_same_zero_based_bounded_authority() {
    let mut fixture = fixture();
    fixture.regions = WorkflowCompositeRegions::from_spec(WorkflowCompositeRegionsSpec {
        id: "support.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        regions: vec![WorkflowCompositeRegionPolicy::Loop(
            WorkflowLoopRegionPolicy {
                step_id: "iteration".into(),
                maximum_iterations: 3,
                time_budget_seconds: 60,
                termination_path: vec!["done".into()],
            },
        )],
    })
    .expect("loop regions");
    fixture.plan.composite_regions_digest = Some(fixture.regions.digest().clone());
    fixture.request.plan_digest = plan_digest(&fixture.plan);
    fixture.request.ordinal = 2;

    let frame = WorkflowCompositeFrame::open(
        fixture.request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("last admitted loop frame");
    assert_eq!(frame.mode, WorkflowCompositeFrameMode::Loop);
    assert_eq!(frame.ordinal, 2);
}

#[test]
fn frame_result_digest_rejects_export_tampering() {
    let fixture = fixture();
    let frame = WorkflowCompositeFrame::open(
        fixture.request,
        &fixture.plan,
        &fixture.regions,
        &fixture.variables,
        None,
    )
    .expect("frame");
    let mut result = frame
        .resolve(
            &fixture.plan,
            &fixture.regions,
            &fixture.variables,
            json!("A!"),
        )
        .expect("result");
    let mut drifted_spec = fixture.variables.spec().clone();
    drifted_spec.revision = "1.0.1".into();
    let drifted_contract =
        WorkflowVariableContract::from_spec(drifted_spec).expect("drifted contract");
    assert!(result.validate(&frame, &drifted_contract).is_err());

    result
        .exported_variables
        .insert("iteration_result".into(), json!("tampered"));
    assert!(result.validate(&frame, &fixture.variables).is_err());
}

#[test]
fn composite_frame_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowCompositeFrameRequest>();
    assert_send_sync::<WorkflowCompositeFrame>();
    assert_send_sync::<WorkflowCompositeFrameResult>();
}
