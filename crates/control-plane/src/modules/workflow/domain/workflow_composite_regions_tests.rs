use super::workflow_revision_semantic_contracts_tests::{
    composite_bindings, composite_regions, composite_registry, composite_workflow,
    variable_contract,
};
use super::*;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    Sha256Digest, WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use chrono::Utc;
use serde_json::json;

fn iteration(step_id: &str) -> WorkflowCompositeRegionPolicy {
    WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
        step_id: step_id.into(),
        maximum_items: 1_000,
        maximum_concurrency: 10,
        failure_mode: WorkflowIterationFailureMode::ContinueNull,
    })
}

fn loop_region(step_id: &str) -> WorkflowCompositeRegionPolicy {
    WorkflowCompositeRegionPolicy::Loop(WorkflowLoopRegionPolicy {
        step_id: step_id.into(),
        maximum_iterations: 100,
        time_budget_seconds: 3_600,
        termination_path: vec!["done".into()],
    })
}

fn spec() -> WorkflowCompositeRegionsSpec {
    WorkflowCompositeRegionsSpec {
        id: "support.batch".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        regions: vec![loop_region("refine"), iteration("batch")],
    }
}

#[test]
fn composite_regions_round_trip_in_stable_step_order() {
    let value = WorkflowCompositeRegions::from_spec(spec()).expect("composite regions");
    assert!(
        value
            .canonical_acl()
            .find("iteration \"batch\"")
            .expect("iteration block")
            < value
                .canonical_acl()
                .find("loop \"refine\"")
                .expect("loop block")
    );
    assert_eq!(
        WorkflowCompositeRegions::parse_acl(value.canonical_acl()).expect("parse"),
        value
    );
    assert_eq!(
        WorkflowCompositeRegions::restore(value.canonical_acl(), value.digest().as_str())
            .expect("restore"),
        value
    );
    assert_eq!(
        value
            .resolve("batch")
            .map(|region| region.semantic_profile()),
        Some("workflow.iteration")
    );
}

#[test]
fn checked_in_composite_regions_are_canonical_conformance_evidence() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/w0.3/composite-regions.acl"
    ));
    let value = WorkflowCompositeRegions::parse_acl(source).expect("checked-in regions");
    assert_eq!(
        value,
        WorkflowCompositeRegions::from_spec(spec()).expect("expected regions")
    );
}

#[test]
fn composite_regions_reject_duplicates_bounds_and_noncanonical_acl() {
    let mut duplicate = spec();
    duplicate.regions.push(iteration("batch"));
    assert!(WorkflowCompositeRegions::from_spec(duplicate).is_err());

    let mut invalid_concurrency = spec();
    let WorkflowCompositeRegionPolicy::Iteration(region) = &mut invalid_concurrency.regions[1]
    else {
        panic!("iteration fixture");
    };
    region.maximum_concurrency = WORKFLOW_ITERATION_MAX_CONCURRENCY + 1;
    assert!(WorkflowCompositeRegions::from_spec(invalid_concurrency).is_err());

    let mut invalid_loop = spec();
    let WorkflowCompositeRegionPolicy::Loop(region) = &mut invalid_loop.regions[0] else {
        panic!("loop fixture");
    };
    region.time_budget_seconds = WORKFLOW_COMPOSITE_REGION_MAX_TIME_BUDGET_SECONDS + 1;
    assert!(WorkflowCompositeRegions::from_spec(invalid_loop).is_err());

    let value = WorkflowCompositeRegions::from_spec(spec()).expect("composite regions");
    assert!(WorkflowCompositeRegions::parse_acl(
        &value
            .canonical_acl()
            .replace("maximum_items = 1000", "maximum_items   = 1000",)
    )
    .is_err());
    assert!(WorkflowCompositeRegions::restore(
        value.canonical_acl(),
        &format!("sha256:{}", "0".repeat(64))
    )
    .is_err());
}

#[test]
fn compiler_pins_composite_regions_to_runtime_v3_and_v2_remains_fail_closed() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let principal_id = PrincipalId::new();
    let now = Utc::now();

    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Object,
            fields: Vec::new(),
        }))
        .expect("data schema");
    let input_configuration = WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
        WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
    ))
    .expect("input configuration");
    let iteration_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Subworkflow),
        ))
        .expect("iteration configuration");
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))
        .expect("output configuration");

    let mut workflow = composite_workflow();
    for step in &mut workflow.steps {
        step.input_schema_digest = data_schema.digest().clone();
        step.output_schema_digest = data_schema.digest().clone();
        step.configuration_digest = match step.id.as_str() {
            "input" => input_configuration.digest().clone(),
            "iteration" => iteration_configuration.digest().clone(),
            "output" => output_configuration.digest().clone(),
            id => panic!("unexpected composite step {id:?}"),
        };
    }
    let mut variable_spec = variable_contract().spec().clone();
    variable_spec.declarations[0].value_schema_digest = data_schema.digest().clone();
    variable_spec.declarations[0].source_schema_digest = Some(data_schema.digest().clone());
    variable_spec.reads[0].expected_schema_digest = data_schema.digest().clone();
    let variables = WorkflowVariableContract::from_spec(variable_spec).expect("variables");
    let regions = composite_regions();
    let registry = composite_registry();
    let semantics = WorkflowRevisionSemanticContracts::create_with_optional_contracts(
        &workflow,
        composite_bindings(&registry),
        registry,
        variables,
        None,
        Some(regions.clone()),
    )
    .expect("composite semantic contracts");
    let workflow_contract =
        WorkflowContract::from_spec(workflow.clone()).expect("Workflow contract");
    let revision = WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        workflow_contract.clone(),
        vec![
            data_schema,
            input_configuration,
            iteration_configuration,
            output_configuration,
        ],
        semantics,
        principal_id,
        now,
    )
    .expect("Workflow revision");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        workflow.name,
        workflow.description,
        revision_id,
        workflow_contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("Workflow definition");
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Composite ontology".into(),
        description: String::new(),
        object_types: vec![OntologyObjectType {
            id: "request".into(),
            label: "Request".into(),
            schema_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                .expect("ontology schema digest"),
            key_fields: vec!["id".into()],
        }],
        relation_types: Vec::new(),
        rules: Vec::new(),
    })
    .expect("ontology contract");
    let ontology_revision = OntologyRevision::initial(
        organization_id,
        project_id,
        ontology_id,
        ontology_revision_id,
        ontology_contract.clone(),
        principal_id,
        now,
    );
    let compiled = WorkflowPlanCompiler::compile_goal(
        WorkflowGoalId::new(),
        PlanRevisionId::new(),
        WorkflowGoalContract::from_spec(WorkflowGoalSpec {
            name: "Composite goal".into(),
            workflow_definition_id: definition_id,
            workflow_revision_id: revision_id,
            workflow_digest: workflow_contract.digest().clone(),
            ontology_id,
            ontology_revision_id,
            ontology_digest: ontology_contract.digest().clone(),
            environment_id: None,
            input: json!({}),
        })
        .expect("goal contract"),
        &definition,
        &revision,
        &ontology_revision,
        principal_id,
        now,
    )
    .expect("compiled composite plan");
    assert_eq!(
        compiled
            .plan_revision
            .plan
            .composite_regions_digest
            .as_ref(),
        Some(regions.digest())
    );

    let compiled_run = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("runtime v3 composite execution input");
    assert_eq!(
        compiled_run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V3
    );
    assert_eq!(
        compiled_run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3
    );
    assert_eq!(
        compiled_run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V3
    );

    let mut downgraded = compiled_run.run.execution_input;
    downgraded.schema = WORKFLOW_RUN_INPUT_SCHEMA_V2.into();
    downgraded.runtime_contract_revision = WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2.into();
    downgraded.flow_workflow_version = WORKFLOW_RUN_FLOW_VERSION_V2.into();
    let error = downgraded
        .validate()
        .expect_err("runtime v2 must keep composite execution fail closed");
    assert!(
        error.contains("does not execute subworkflow step \"iteration\""),
        "unexpected runtime error: {error}"
    );
}

#[test]
fn composite_region_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WorkflowCompositeRegions>();
    assert_send_sync::<WorkflowCompositeRegionsSpec>();
    assert_send_sync::<WorkflowCompositeRegionPolicy>();
}
