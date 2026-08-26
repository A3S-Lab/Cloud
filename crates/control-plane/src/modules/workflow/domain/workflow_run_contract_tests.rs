use super::*;
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use crate::modules::workflow::test_support::{
    agent_workflow_run_input, application_answer_workflow_run_input,
    application_frame_answer_workflow_run_inputs, application_nested_frame_answer_authorities,
    application_variable_workflow_run_input, application_workflow_run_input,
    cancellation_compensating_connector_workflow_run_input, connector_workflow_run_input,
    connector_workflow_run_input_v5, connector_workflow_run_input_v6,
    human_decision_workflow_run_input, routed_agent_workflow_run_input,
    routed_application_answer_and_variable_workflow_run_input,
    routed_application_answer_workflow_run_input,
    routed_application_frame_answer_workflow_run_input,
    routed_application_variable_workflow_run_input, routed_composite_workflow_run_input,
    routed_connector_workflow_run_input, routed_execution_workflow_run_input,
    typed_variable_workflow_run_input, workflow_run_input, TEST_AGENT_STEP_ID, TEST_ANSWER_STEP_ID,
    TEST_APPLICATION_VARIABLE_STEP_ID, TEST_CONNECTOR_STEP_ID, TEST_HUMAN_STEP_ID,
};

#[test]
fn v25_agent_failure_routing_is_exact_and_preserves_v24_replay() {
    let input = routed_agent_workflow_run_input().expect("valid routed Agent WorkflowRun input");
    assert_eq!(input.plan.schema, WORKFLOW_PLAN_SCHEMA_V12);
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V25);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V25
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V25);
    input.validate().expect("valid v25 Agent failure input");

    let mut downgraded = input;
    downgraded.schema = WORKFLOW_RUN_INPUT_SCHEMA_V24.into();
    downgraded.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V24.into();
    downgraded.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V24.into();
    assert!(downgraded.validate().is_err());

    agent_workflow_run_input()
        .expect("historic v24 Agent input remains replayable")
        .validate()
        .expect("valid historic v24 Agent replay");
}

#[test]
fn v24_agent_dispatch_is_exact_and_version_fenced() {
    let input = agent_workflow_run_input().expect("valid Agent WorkflowRun input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V24);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V24
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V24);
    input.validate().expect("valid v24 Agent input");

    let agent = input
        .plan
        .steps
        .iter()
        .find(|step| step.id == TEST_AGENT_STEP_ID)
        .expect("Agent plan step");
    let capability = agent.capability.as_ref().expect("AgentRelease capability");
    assert_eq!(capability.owner, CapabilityOwner::Assets);
    assert_eq!(capability.capability_type, CapabilityType::AgentRelease);
    assert_eq!(capability.capability, "agent.execute");
    assert!(uuid::Uuid::parse_str(&capability.revision).is_ok());

    let mut downgraded = input.clone();
    downgraded.schema = WORKFLOW_RUN_INPUT_SCHEMA_V23.into();
    downgraded.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V23.into();
    downgraded.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V23.into();
    assert!(downgraded.validate().is_err());

    let mut capability_drift = input;
    capability_drift
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_AGENT_STEP_ID)
        .and_then(|step| step.capability.as_mut())
        .expect("AgentRelease capability")
        .capability = "agent.run".into();
    assert!(capability_drift.validate().is_err());
}

#[test]
fn v23_cancellation_compensation_is_exact_and_version_fenced() {
    let input = cancellation_compensating_connector_workflow_run_input()
        .expect("valid cancellation-compensating Connector input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V23);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V23
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V23);
    input.validate().expect("valid v23 input");

    let mut downgraded = input;
    downgraded.schema = WORKFLOW_RUN_INPUT_SCHEMA_V21.into();
    downgraded.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21.into();
    downgraded.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V21.into();
    assert!(downgraded.validate().is_err());
}

#[test]
fn v22_parallel_iteration_is_exact_and_preserves_legacy_serial_replay() {
    let input = routed_composite_workflow_run_input(
        WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
            step_id: "batch".into(),
            maximum_items: 2,
            maximum_concurrency: 2,
            failure_mode: WorkflowIterationFailureMode::Terminate,
        }),
        serde_json::json!([{"item": 1}, {"item": 2}]),
    )
    .expect("valid parallel composite input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V22);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V22);
    input.validate().expect("valid v22 input");

    let mut historic = input.clone();
    historic.schema = WORKFLOW_RUN_INPUT_SCHEMA_V19.into();
    historic.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19.into();
    historic.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V19.into();
    historic
        .validate()
        .expect("pre-v22 serial history remains replayable");

    let mut unjustified = input;
    let regions = unjustified
        .composite_regions
        .as_ref()
        .expect("regions")
        .restore()
        .expect("restore regions");
    let mut spec = regions.spec().clone();
    let WorkflowCompositeRegionPolicy::Iteration(policy) = &mut spec.regions[0] else {
        panic!("iteration fixture")
    };
    policy.maximum_concurrency = 1;
    let serial = WorkflowCompositeRegions::from_spec(spec).expect("serial regions");
    unjustified.plan.composite_regions_digest = Some(serial.digest().clone());
    unjustified.composite_regions = Some(ResolvedWorkflowCompositeRegions::from_regions(&serial));
    unjustified.plan_digest = Sha256Digest::from_bytes(
        &canonical_json_bounded(&unjustified.plan, WORKFLOW_PLAN_MAX_BYTES, "test plan")
            .expect("plan bytes"),
    );
    assert!(unjustified.validate().is_err());
}

#[test]
fn v19_composite_failure_route_is_exact_and_version_fenced() {
    let input = routed_composite_workflow_run_input(
        WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
            step_id: "batch".into(),
            maximum_items: 1,
            maximum_concurrency: 1,
            failure_mode: WorkflowIterationFailureMode::Terminate,
        }),
        serde_json::json!([{"item": 1}]),
    )
    .expect("valid routed composite input");
    assert_eq!(input.plan.schema, WORKFLOW_PLAN_SCHEMA_V11);
    assert_eq!(
        input.plan.compiler_revision,
        WORKFLOW_PLAN_COMPILER_REVISION_V11
    );
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V19);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V19);
    input.validate().expect("valid v19 input");

    let mut downgraded = input;
    downgraded.schema = WORKFLOW_RUN_INPUT_SCHEMA_V18.into();
    downgraded.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V18.into();
    downgraded.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V18.into();
    assert!(downgraded.validate().is_err());
}

#[test]
fn v15_application_answer_failure_route_is_exact_frame_capable_and_version_fenced() {
    let input = routed_application_answer_workflow_run_input()
        .expect("valid routed Application Answer input");
    assert_eq!(input.plan.schema, WORKFLOW_PLAN_SCHEMA_V7);
    assert_eq!(
        input.plan.compiler_revision,
        WORKFLOW_PLAN_COMPILER_REVISION_V7
    );
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V15);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V15);
    input.validate().expect("valid v15 input");

    let mut historic_alias = input.clone();
    historic_alias.schema = WORKFLOW_RUN_INPUT_SCHEMA_V11.into();
    historic_alias.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11.into();
    historic_alias.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V11.into();
    assert!(historic_alias.validate().is_err());

    let mut plan_alias = input.clone();
    plan_alias.plan.schema = WORKFLOW_PLAN_SCHEMA_V6.into();
    plan_alias.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V6.into();
    assert!(plan_alias.plan.validate().is_err());

    let mut compiler_alias = input.clone();
    compiler_alias.plan.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V6.into();
    assert!(compiler_alias.plan.validate().is_err());

    let mut descriptor_alias = input.clone();
    descriptor_alias
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_ANSWER_STEP_ID)
        .and_then(|step| step.descriptor.as_mut())
        .expect("Application Answer descriptor")
        .descriptor_id = "application.response".into();
    assert!(descriptor_alias.plan.validate().is_err());

    let (_, frame, child) = routed_application_frame_answer_workflow_run_input(1)
        .expect("valid routed Application frame Answer input");
    assert_eq!(child.schema, WORKFLOW_RUN_INPUT_SCHEMA_V15);
    assert_eq!(
        child.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15
    );
    assert_eq!(child.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V15);
    let projection = child
        .application_projection
        .as_ref()
        .expect("frame projection");
    assert_eq!(
        projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4
    );
    projection
        .frame_authority
        .as_ref()
        .expect("frame authority")
        .validate_for_frame(&frame)
        .expect("exact frame authority");
    child.validate().expect("valid v15 frame input");

    let coexistence = routed_application_answer_and_variable_workflow_run_input()
        .expect("valid routed Answer and variable input");
    assert_eq!(coexistence.plan.schema, WORKFLOW_PLAN_SCHEMA_V7);
    assert_eq!(coexistence.schema, WORKFLOW_RUN_INPUT_SCHEMA_V15);
    let projection = coexistence
        .application_projection
        .as_ref()
        .expect("coexisting Application projection");
    assert_eq!(
        projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
    );
    assert_eq!(projection.answer_step_ids, [TEST_ANSWER_STEP_ID]);
    assert_eq!(
        projection.variable_assignment_step_ids,
        [TEST_APPLICATION_VARIABLE_STEP_ID]
    );
    coexistence.validate().expect("valid v15 coexistence input");
}

#[test]
fn v14_application_variable_failure_route_is_exact_and_version_fenced() {
    let input = routed_application_variable_workflow_run_input()
        .expect("valid routed Application variable input");
    assert_eq!(input.plan.schema, WORKFLOW_PLAN_SCHEMA_V6);
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V14);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V14);
    input.validate().expect("valid v14 input");

    let mut historic_alias = input.clone();
    historic_alias.schema = WORKFLOW_RUN_INPUT_SCHEMA_V12.into();
    historic_alias.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12.into();
    historic_alias.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V12.into();
    assert!(historic_alias.validate().is_err());

    let mut descriptor_alias = input.clone();
    descriptor_alias
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_APPLICATION_VARIABLE_STEP_ID)
        .and_then(|step| step.descriptor.as_mut())
        .expect("Application variable descriptor")
        .descriptor_id = "application.variable-assign".into();
    assert!(descriptor_alias.plan.validate().is_err());
}

#[test]
fn v13_application_frames_pin_root_path_and_repeated_answer_ordinals() {
    let (parent, frames) = application_frame_answer_workflow_run_inputs()
        .expect("valid repeated Application Answer frames");
    assert_eq!(parent.schema, WORKFLOW_RUN_INPUT_SCHEMA_V13);
    assert_eq!(
        parent.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13
    );
    assert_eq!(parent.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V13);
    let parent_projection = parent
        .application_projection
        .as_ref()
        .expect("root Application projection");
    assert_eq!(
        parent_projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
    );
    assert!(parent_projection.projects_application_lifecycle());
    assert!(parent_projection.supports_application_frames());
    assert!(parent_projection.frame_authority.is_none());
    parent.validate().expect("valid v13 Application root");

    let [(frame_zero, child_zero), (frame_one, child_one)] = frames.as_slice() else {
        panic!("expected two repeated Application frames, got {frames:#?}")
    };
    let mut effect_step_ids = Vec::new();
    for (expected_ordinal, frame, child) in [(0, frame_zero, child_zero), (1, frame_one, child_one)]
    {
        assert_eq!(frame.ordinal, expected_ordinal);
        assert_eq!(child.schema, WORKFLOW_RUN_INPUT_SCHEMA_V13);
        assert_eq!(
            child.runtime_contract_revision,
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13
        );
        assert_eq!(child.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V13);
        let projection = child
            .application_projection
            .as_ref()
            .expect("frame Application projection");
        assert_eq!(
            projection.schema,
            WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4
        );
        assert!(!projection.projects_application_lifecycle());
        assert!(projection.supports_application_frames());
        assert_eq!(projection.answer_step_ids, [TEST_ANSWER_STEP_ID]);
        let authority = projection
            .frame_authority
            .as_ref()
            .expect("frame authority");
        authority
            .validate_for_frame(frame)
            .expect("exact parent frame authority");
        authority
            .validate_for_child(
                child.organization_id,
                child.project_id,
                child.workflow_run_id,
                &child.plan,
            )
            .expect("exact child authority");
        assert_eq!(
            authority.application_workflow_run_id,
            parent.workflow_run_id
        );
        assert_eq!(authority.parent_workflow_run_id, parent.workflow_run_id);
        assert_eq!(authority.frame_ordinal, expected_ordinal);
        assert_eq!(authority.child_workflow_run_id, child.workflow_run_id);
        effect_step_ids.push(
            authority
                .answer_effect_step_id(TEST_ANSWER_STEP_ID)
                .expect("stable frame Answer step"),
        );
        child.validate().expect("valid v13 Application frame");
    }
    assert_eq!(effect_step_ids[0], effect_step_ids[1]);
    assert!(effect_step_ids[0].starts_with("frame-answer-"));
    assert_ne!(frame_zero.frame_digest, frame_one.frame_digest);
    assert_ne!(child_zero.workflow_run_id, child_one.workflow_run_id);
    let authority_zero = child_zero
        .application_projection
        .as_ref()
        .and_then(|projection| projection.frame_authority.as_ref())
        .expect("frame zero authority");
    let authority_one = child_one
        .application_projection
        .as_ref()
        .and_then(|projection| projection.frame_authority.as_ref())
        .expect("frame one authority");
    assert_eq!(
        authority_zero.logical_path_digest,
        authority_one.logical_path_digest
    );
    assert_ne!(
        authority_zero.execution_path_digest,
        authority_one.execution_path_digest
    );

    let mut legacy_alias = child_zero.clone();
    legacy_alias.schema = WORKFLOW_RUN_INPUT_SCHEMA_V12.into();
    legacy_alias.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12.into();
    legacy_alias.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V12.into();
    assert!(legacy_alias.validate().is_err());

    let mut missing_authority = child_one.clone();
    missing_authority
        .application_projection
        .as_mut()
        .expect("frame projection")
        .frame_authority = None;
    assert!(missing_authority.validate().is_err());

    let mut cross_tenant = child_zero.clone();
    cross_tenant.organization_id = crate::modules::shared_kernel::domain::OrganizationId::new();
    assert!(cross_tenant.validate().is_err());
}

#[test]
fn v13_nested_application_frames_partition_outer_execution_paths() {
    let [outer_zero_inner_zero, outer_zero_inner_one, outer_one_inner_zero] =
        application_nested_frame_answer_authorities()
            .expect("valid nested Application Answer authorities");
    for authority in [
        &outer_zero_inner_zero,
        &outer_zero_inner_one,
        &outer_one_inner_zero,
    ] {
        authority.validate().expect("valid nested frame authority");
        assert_eq!(
            authority.application_workflow_run_id,
            outer_zero_inner_zero.application_workflow_run_id
        );
    }
    assert_eq!(outer_zero_inner_zero.frame_ordinal, 0);
    assert_eq!(outer_zero_inner_one.frame_ordinal, 1);
    assert_eq!(
        outer_zero_inner_zero.logical_path_digest,
        outer_zero_inner_one.logical_path_digest
    );
    assert_eq!(
        outer_zero_inner_zero
            .answer_effect_step_id(TEST_ANSWER_STEP_ID)
            .expect("outer-zero inner-zero Answer step"),
        outer_zero_inner_one
            .answer_effect_step_id(TEST_ANSWER_STEP_ID)
            .expect("outer-zero inner-one Answer step")
    );
    assert_ne!(
        outer_zero_inner_zero.execution_path_digest,
        outer_zero_inner_one.execution_path_digest
    );

    assert_eq!(outer_one_inner_zero.frame_ordinal, 0);
    assert_ne!(
        outer_zero_inner_zero.parent_execution_path_digest,
        outer_one_inner_zero.parent_execution_path_digest
    );
    assert_ne!(
        outer_zero_inner_zero.logical_path_digest,
        outer_one_inner_zero.logical_path_digest
    );
    assert_ne!(
        outer_zero_inner_zero
            .answer_effect_step_id(TEST_ANSWER_STEP_ID)
            .expect("outer-zero Answer step"),
        outer_one_inner_zero
            .answer_effect_step_id(TEST_ANSWER_STEP_ID)
            .expect("outer-one Answer step")
    );
}

#[test]
fn legacy_application_answer_projection_remains_frame_free() {
    let input = application_answer_workflow_run_input().expect("valid v11 Answer input");
    let projection = input
        .application_projection
        .as_ref()
        .expect("legacy Application projection");
    assert!(projection.projects_application_lifecycle());
    assert!(!projection.supports_application_frames());
    assert!(projection.frame_authority.is_none());
    let canonical = String::from_utf8(input.canonical_bytes().expect("canonical v11 input"))
        .expect("UTF-8 v11 input");
    assert!(!canonical.contains("frame_authority"));
    assert!(!canonical.contains("application-frame-authority"));
}

#[test]
fn v12_run_input_pins_exact_application_variable_projection_without_aliasing_v11() {
    let input = application_variable_workflow_run_input()
        .expect("valid Application-variable WorkflowRun input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V12);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V12);
    let projection = input
        .application_projection
        .as_ref()
        .expect("Application projection");
    assert_eq!(
        projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3
    );
    assert_eq!(projection.final_output_step_id, "output");
    assert!(projection.answer_step_ids.is_empty());
    assert_eq!(
        projection.variable_step_ids,
        [TEST_APPLICATION_VARIABLE_STEP_ID]
    );
    assert_eq!(
        projection.variable_assignment_step_ids,
        [TEST_APPLICATION_VARIABLE_STEP_ID]
    );
    input.validate().expect("valid v12 input");

    let canonical = String::from_utf8(input.canonical_bytes().expect("canonical v12 input"))
        .expect("UTF-8 v12 input");
    assert!(canonical.contains("variable_step_ids"));
    assert!(canonical.contains("variable_assignment_step_ids"));

    let mut v11_alias = input.clone();
    v11_alias.schema = WORKFLOW_RUN_INPUT_SCHEMA_V11.into();
    v11_alias.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11.into();
    v11_alias.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V11.into();
    assert!(v11_alias.validate().is_err());

    let mut missing_assignment = input.clone();
    missing_assignment
        .application_projection
        .as_mut()
        .expect("Application projection")
        .variable_assignment_step_ids
        .clear();
    assert!(missing_assignment.validate().is_err());

    let mut descriptor_alias = input;
    descriptor_alias
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_APPLICATION_VARIABLE_STEP_ID)
        .and_then(|step| step.descriptor.as_mut())
        .expect("Application variable descriptor")
        .descriptor_id = "application.variable-assign".into();
    refresh_plan_digest(&mut descriptor_alias);
    assert!(descriptor_alias.validate().is_err());
}

#[test]
fn v11_run_input_partitions_answer_from_final_output_without_reinterpreting_v10() {
    let input = application_answer_workflow_run_input().expect("valid Answer WorkflowRun input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V11);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V11);
    let projection = input
        .application_projection
        .as_ref()
        .expect("Application projection");
    assert_eq!(
        projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2
    );
    assert_eq!(projection.final_output_step_id, "output");
    assert_eq!(projection.answer_step_ids, [TEST_ANSWER_STEP_ID]);
    input.validate().expect("valid v11 input");

    let mut v10_alias = input.clone();
    v10_alias.schema = WORKFLOW_RUN_INPUT_SCHEMA_V10.into();
    v10_alias.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10.into();
    v10_alias.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V10.into();
    assert!(v10_alias.validate().is_err());

    let mut reordered = input;
    reordered
        .application_projection
        .as_mut()
        .expect("Application projection")
        .answer_step_ids = vec!["output".into()];
    assert!(reordered.validate().is_err());
}

#[test]
fn v10_run_input_requires_exact_application_final_output_projection() {
    let input = application_workflow_run_input().expect("valid Application WorkflowRun input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V10);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V10);
    let projection = input
        .application_projection
        .as_ref()
        .expect("Application projection");
    assert_eq!(
        projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA
    );
    assert_eq!(projection.final_output_step_id, "output");
    input.validate().expect("valid v10 input");
    let canonical = String::from_utf8(input.canonical_bytes().expect("canonical v10 input"))
        .expect("UTF-8 v10 input");
    assert!(!canonical.contains("answer_step_ids"));
    assert!(!canonical.contains("variable_step_ids"));
    assert!(!canonical.contains("variable_assignment_step_ids"));

    let mut drifted = input.clone();
    drifted
        .application_projection
        .as_mut()
        .expect("Application projection")
        .final_output_step_id = "input".into();
    assert!(drifted.validate().is_err());

    let mut missing = input;
    missing.application_projection = None;
    assert!(missing.validate().is_err());
}

#[test]
fn v10_application_generation_composes_connector_and_non_connector_plans() {
    let mut connector = connector_workflow_run_input().expect("Connector input");
    connector.schema = WORKFLOW_RUN_INPUT_SCHEMA_V10.into();
    connector.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10.into();
    connector.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V10.into();
    connector.application_projection = Some(
        WorkflowRunApplicationProjection::from_plan(&connector.plan)
            .expect("Connector final Output"),
    );
    connector.validate().expect("v10 Connector input");

    application_workflow_run_input()
        .expect("non-Connector Application input")
        .validate()
        .expect("v10 non-Connector input");
}

#[test]
fn v8_run_input_admits_only_exact_connector_service_authority() {
    let input = connector_workflow_run_input().expect("valid Connector WorkflowRun input");
    input.validate().expect("valid v8 input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V8);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V8);

    let mut missing_environment = input.clone();
    missing_environment.plan.environment_id = None;
    refresh_plan_digest(&mut missing_environment);
    assert!(missing_environment.validate().is_err());

    let mut wrong_kind = input;
    wrong_kind
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_CONNECTOR_STEP_ID)
        .expect("Connector step")
        .kind = WorkflowStepKind::Transform;
    refresh_plan_digest(&mut wrong_kind);
    assert!(wrong_kind.validate().is_err());
}

#[test]
fn v9_run_input_admits_descriptor_bound_connector_failure_routes() {
    let input = routed_connector_workflow_run_input().expect("valid routed Connector input");
    assert_eq!(input.plan.schema, WORKFLOW_PLAN_SCHEMA_V5);
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V9);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V9);
    input.validate().expect("valid v9 Connector failure route");

    let mut legacy_failure = input.plan.clone();
    legacy_failure.schema = WORKFLOW_PLAN_SCHEMA_V3.into();
    legacy_failure.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V3.into();
    assert!(legacy_failure.validate().is_err());

    let mut legacy_default = input.plan.clone();
    legacy_default.schema = WORKFLOW_PLAN_SCHEMA_V4.into();
    legacy_default.compiler_revision = WORKFLOW_PLAN_COMPILER_REVISION_V4.into();
    assert!(legacy_default.validate().is_err());

    let mut missing_connector_route = input.plan;
    missing_connector_route
        .edges
        .retain(|edge| edge.source_handle.is_none());
    assert!(missing_connector_route.validate().is_err());
}

#[test]
fn v6_connector_input_remains_valid_with_reference_only_response_semantics() {
    let input = connector_workflow_run_input_v6().expect("historic v6 Connector input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V6);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V6);
    input.validate().expect("valid historic v6 input");
}

#[test]
fn v5_connector_input_remains_valid_without_response_object_semantics() {
    let input = connector_workflow_run_input_v5().expect("historic v5 Connector input");
    assert_eq!(input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V5);
    assert_eq!(
        input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5
    );
    assert_eq!(input.flow_workflow_version, WORKFLOW_RUN_FLOW_VERSION_V5);
    input.validate().expect("valid historic v5 input");
}

#[test]
fn connector_generation_does_not_alias_failure_routing_v4() {
    let routed = routed_execution_workflow_run_input().expect("valid v4 failure-routing input");
    assert_eq!(routed.schema, WORKFLOW_RUN_INPUT_SCHEMA_V4);
    routed.validate().expect("v4 failure-routing input");

    let mut connector = connector_workflow_run_input().expect("valid v8 Connector input");
    connector.schema = WORKFLOW_RUN_INPUT_SCHEMA_V4.into();
    connector.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4.into();
    connector.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V4.into();
    assert!(connector.validate().is_err());
}

#[test]
fn run_input_retry_budget_remains_bound_to_an_exact_connector_revision() {
    let input = workflow_run_input().expect("run input");
    let mut step = input
        .resolved_steps()
        .expect("resolved steps")
        .into_iter()
        .next()
        .expect("step");
    step.policy = Some(WorkflowPolicy {
        mode: WorkflowPolicyMode::Static,
        expression: None,
        candidates: Vec::new(),
        retry: Some(WorkflowRetryPolicy {
            maximum_attempts: 3,
            default_delay_seconds: 5,
        }),
        default_output: None,
        cancellation_compensation: None,
    });
    assert!(super::workflow_run_contract::validate_runtime_retry_policy(&step).is_err());

    step.plan.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Connectors,
        capability_type: CapabilityType::ConnectorRevision,
        resource_id: uuid::Uuid::now_v7(),
        revision: uuid::Uuid::now_v7().to_string(),
        digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
        capability: "connector.http".into(),
    });
    super::workflow_run_contract::validate_runtime_retry_policy(&step)
        .expect("connector retry budget");
    step.policy.as_mut().expect("policy").retry = None;
    assert!(super::workflow_run_contract::validate_runtime_retry_policy(&step).is_err());
}

#[test]
fn immutable_run_input_rejects_plan_input_payload_and_branch_drift() {
    let input = workflow_run_input().expect("valid WorkflowRun input");
    input.validate().expect("valid input");

    let mut goal_drift = input.clone();
    goal_drift.goal_input["priority"] = serde_json::json!("normal");
    assert!(goal_drift.validate().is_err());

    let mut payload_order_drift = input.clone();
    payload_order_drift.payloads.swap(0, 1);
    assert!(payload_order_drift.validate().is_err());

    let mut branch_drift = input;
    branch_drift
        .plan
        .edges
        .iter_mut()
        .find(|edge| edge.id == "route-high")
        .expect("high branch edge")
        .source_handle = Some("unexpected".into());
    refresh_plan_digest(&mut branch_drift);
    assert!(branch_drift.validate().is_err());
}

#[test]
fn v1_run_input_remains_byte_stable_without_v2_contract_fields() {
    let input = workflow_run_input().expect("valid WorkflowRun input");
    let encoded =
        String::from_utf8(input.canonical_bytes().expect("canonical v1 input")).expect("UTF-8");
    assert!(!encoded.contains("variable_contract"));
    assert!(!encoded.contains("composite_regions"));
    assert!(!encoded.contains("application_projection"));
    assert!(encoded.contains("\"schema\":\"cloud.workflow-run.input.v1\""));
    assert!(encoded.contains("\"flow_workflow_version\":\"1\""));
}

#[test]
fn v2_run_input_rejects_version_and_variable_contract_drift() {
    let input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    input.validate().expect("valid v2 input");
    let encoded =
        String::from_utf8(input.canonical_bytes().expect("canonical v2 input")).expect("UTF-8");
    assert!(!encoded.contains("composite_regions"));
    assert!(!encoded.contains("composite_regions_digest"));

    let mut version_drift = input.clone();
    version_drift.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION.into();
    assert!(version_drift.validate().is_err());

    let mut contract_drift = input;
    contract_drift
        .variable_contract
        .as_mut()
        .expect("variable contract")
        .digest = test_digest('f');
    assert!(contract_drift.validate().is_err());
}

#[test]
fn runtime_v2_admits_composite_frames_but_rejects_external_variable_ownership() {
    let input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    let base = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .restore()
        .expect("restored variable contract")
        .spec()
        .clone();

    let default = WorkflowVariableDefault::new("fallback", serde_json::json!("normal"))
        .expect("default material");
    let mut default_spec = base.clone();
    default_spec.declarations.push(WorkflowVariableDeclaration {
        name: "fallback".into(),
        scope: WorkflowVariableScope::Run,
        value_type: WorkflowDataType::String,
        value_schema_digest: test_digest('a'),
        source_schema_digest: None,
        storage_class: WorkflowVariableStorageClass::Inline,
        mutation_mode: WorkflowVariableMutationMode::Deterministic,
        required: false,
        source_step_id: None,
        source_path: Vec::new(),
        region_id: None,
        default_value_digest: Some(default.digest.clone()),
    });
    let default_contract = WorkflowVariableContract::from_spec(default_spec)
        .expect("valid digest-backed default contract");
    let default_error = validate_runtime_variable_contract(&default_contract, None, &input.plan)
        .expect_err("runtime must reject a digest without materialized default bytes");
    assert!(default_error.contains("digest-only default"));
    let defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: default_contract.id().into(),
        revision: default_contract.revision().into(),
        values: vec![default],
    })
    .expect("default set");
    validate_runtime_variable_contract(&default_contract, Some(&defaults), &input.plan)
        .expect("digest-backed default material");

    let mut composite_spec = base.clone();
    composite_spec
        .declarations
        .push(WorkflowVariableDeclaration {
            name: "local_value".into(),
            scope: WorkflowVariableScope::CompositeLocal,
            value_type: WorkflowDataType::String,
            value_schema_digest: test_digest('c'),
            source_schema_digest: None,
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Deterministic,
            required: false,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: Some("normalize".into()),
            default_value_digest: None,
        });
    let composite_contract = WorkflowVariableContract::from_spec(composite_spec)
        .expect("valid composite-local contract");
    validate_runtime_variable_contract(&composite_contract, None, &input.plan)
        .expect("composite-local state is admitted for the deterministic frame reducer");

    let mut application_spec = base;
    application_spec
        .declarations
        .push(WorkflowVariableDeclaration {
            name: "conversation".into(),
            scope: WorkflowVariableScope::Application,
            value_type: WorkflowDataType::String,
            value_schema_digest: test_digest('d'),
            source_schema_digest: None,
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::OptimisticApplicationPort,
            required: false,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: None,
            default_value_digest: None,
        });
    let application_contract = WorkflowVariableContract::from_spec(application_spec)
        .expect("valid application-owned contract");
    let application_error =
        validate_runtime_variable_contract(&application_contract, None, &input.plan)
            .expect_err("runtime must reject application-owned state");
    assert!(application_error.contains("application"));
}

#[test]
fn runtime_v2_rejects_reads_for_steps_without_projected_input_support() {
    let input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    let contract = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .restore()
        .expect("restored variable contract");
    let mut spec = contract.spec().clone();
    let request = spec
        .declarations
        .iter()
        .find(|declaration| declaration.name == "request")
        .expect("request declaration")
        .clone();
    spec.reads.push(WorkflowVariableRead {
        id: "input-request".into(),
        variable: request.name.clone(),
        consumer_step_id: "input".into(),
        consumer_region_id: None,
        target_port: "invocation".into(),
        path: Vec::new(),
        expected_type: request.value_type.clone(),
        expected_schema_digest: request.value_schema_digest.clone(),
        required: true,
        mode: WorkflowVariableReadMode::DirectValue,
    });
    let contract = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        compiler_schema_version: WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
        ..spec
    })
    .expect("valid input-read contract");
    let error = validate_runtime_variable_contract(&contract, None, &input.plan)
        .expect_err("runtime must reject projection into Input");
    assert!(error.contains("input step"));
}

#[test]
fn runtime_v2_explicit_reads_cannot_bypass_the_typed_projection() {
    let mut input = typed_variable_workflow_run_input().expect("valid v2 WorkflowRun input");
    let contract = input
        .variable_contract
        .as_ref()
        .expect("variable contract")
        .restore()
        .expect("restored variable contract");
    let mut spec = contract.spec().clone();
    let request = spec
        .declarations
        .iter()
        .find(|declaration| declaration.name == "request")
        .expect("request declaration");
    spec.reads.push(WorkflowVariableRead {
        id: "high-request".into(),
        variable: request.name.clone(),
        consumer_step_id: "high".into(),
        consumer_region_id: None,
        target_port: "request".into(),
        path: Vec::new(),
        expected_type: request.value_type.clone(),
        expected_schema_digest: request.value_schema_digest.clone(),
        required: true,
        mode: WorkflowVariableReadMode::DirectValue,
    });
    let contract = WorkflowVariableContract::from_spec(spec).expect("bypass test contract");
    input.plan.variable_contract_digest = Some(contract.digest().clone());
    input.variable_contract = Some(ResolvedWorkflowVariableContract::from_contract(&contract));
    refresh_plan_digest(&mut input);

    let error = input
        .validate()
        .expect_err("legacy template token must not bypass typed reads");
    assert!(error.contains("bypasses their typed projection"));
}

#[test]
fn human_decision_run_requires_an_exact_form_release_binding() {
    let input = human_decision_workflow_run_input().expect("valid human-decision input");
    input.validate().expect("human-decision input");

    let mut missing = input.clone();
    missing
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_HUMAN_STEP_ID)
        .expect("human-decision step")
        .capability = None;
    refresh_plan_digest(&mut missing);
    assert!(missing.validate().is_err());

    let mut floating_release = input;
    floating_release
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == TEST_HUMAN_STEP_ID)
        .and_then(|step| step.capability.as_mut())
        .expect("FormRelease capability")
        .revision = "latest".into();
    refresh_plan_digest(&mut floating_release);
    assert!(floating_release.validate().is_err());
}

#[test]
fn workflow_run_timeout_is_strictly_bounded() {
    assert_eq!(
        workflow_run_timeout_seconds(None).expect("default timeout"),
        WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS
    );
    assert_eq!(workflow_run_timeout_seconds(Some(1)).expect("minimum"), 1);
    assert_eq!(
        workflow_run_timeout_seconds(Some(WORKFLOW_RUN_MAX_TIMEOUT_SECONDS)).expect("maximum"),
        WORKFLOW_RUN_MAX_TIMEOUT_SECONDS
    );
    assert!(workflow_run_timeout_seconds(Some(0)).is_err());
    assert!(workflow_run_timeout_seconds(Some(WORKFLOW_RUN_MAX_TIMEOUT_SECONDS + 1)).is_err());
}

fn refresh_plan_digest(input: &mut WorkflowRunInput) {
    input.plan_digest = Sha256Digest::parse(sha256_digest(
        &canonical_json_bounded(
            &input.plan,
            WORKFLOW_PLAN_MAX_BYTES,
            "WorkflowRun test plan",
        )
        .expect("canonical plan"),
    ))
    .expect("plan digest");
}

fn test_digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
        .expect("test digest")
}
