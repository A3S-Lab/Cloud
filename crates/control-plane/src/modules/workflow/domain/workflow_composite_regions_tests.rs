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
fn runtime_plan_requires_the_exact_composite_semantic_profile() {
    let policy = loop_region("refine");
    let mut input = crate::modules::workflow::test_support::composite_workflow_run_input(
        policy,
        json!({"done": false, "iteration": 0}),
    )
    .expect("loop WorkflowRun input");
    let regions = input
        .composite_regions
        .as_ref()
        .expect("resolved composite regions")
        .restore()
        .expect("restored composite regions");

    regions
        .validate_plan(&input.plan)
        .expect("exact loop descriptor profile");

    input
        .plan
        .steps
        .iter_mut()
        .find(|step| step.id == "refine")
        .expect("loop step")
        .descriptor
        .as_mut()
        .expect("loop descriptor")
        .descriptor_id = "workflow.iteration".into();
    let error = regions
        .validate_plan(&input.plan)
        .expect_err("mismatched composite profile must fail closed");
    assert!(
        error.contains("does not match its immutable workflow.loop region policy"),
        "unexpected profile error: {error}"
    );
}

#[test]
fn compiler_pins_parallel_composite_regions_to_runtime_v22_and_v2_remains_fail_closed() {
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
            data_schema.clone(),
            input_configuration.clone(),
            iteration_configuration.clone(),
            output_configuration.clone(),
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
        workflow.name.clone(),
        workflow.description.clone(),
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
    .expect("runtime v22 composite execution input");
    assert_eq!(
        compiled_run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V22
    );
    assert_eq!(
        compiled_run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
    );
    assert_eq!(
        compiled_run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V22
    );

    let mut routed_workflow = workflow.clone();
    let mut failure_output = routed_workflow
        .steps
        .iter()
        .find(|step| step.id == "output")
        .cloned()
        .expect("composite Output step");
    failure_output.id = "failure_output".into();
    routed_workflow.steps.insert(2, failure_output);
    routed_workflow.edges = vec![
        WorkflowEdgeSpec {
            id: "input-iteration".into(),
            source: "input".into(),
            target: "iteration".into(),
            source_handle: None,
        },
        WorkflowEdgeSpec {
            id: "iteration-failure".into(),
            source: "iteration".into(),
            target: "failure_output".into(),
            source_handle: Some("error".into()),
        },
        WorkflowEdgeSpec {
            id: "iteration-output".into(),
            source: "iteration".into(),
            target: "output".into(),
            source_handle: None,
        },
    ];
    let base_registry = composite_registry();
    let mut descriptor_specs = base_registry
        .descriptors()
        .iter()
        .map(|descriptor| descriptor.spec().clone())
        .collect::<Vec<_>>();
    descriptor_specs
        .iter_mut()
        .find(|descriptor| descriptor.id == "workflow.iteration")
        .expect("iteration descriptor")
        .failure = WorkflowStepFailureContract {
        error_output: Some(WorkflowStepPort {
            name: "error".into(),
            value_type: WorkflowDataType::Object,
            cardinality: WorkflowStepPortCardinality::Single,
            required: true,
            dynamic: false,
        }),
        retry_classification: WorkflowStepRetryClassification::OwnerClassified,
        fallback: WorkflowStepFallbackMode::FailureBranch,
        failure_branch: true,
    };
    let routed_registry =
        WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
            id: base_registry.id().into(),
            revision: base_registry.revision().into(),
            compiler_schema_version: base_registry.compiler_schema_version(),
            descriptors: descriptor_specs,
        })
        .expect("routed composite registry");
    let routed_bindings =
        WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
            id: "support.bound".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            bindings: [
                ("input", "workflow.input"),
                ("iteration", "workflow.iteration"),
                ("failure_output", "workflow.output"),
                ("output", "workflow.output"),
            ]
            .into_iter()
            .map(|(step_id, descriptor_id)| WorkflowStepDescriptorBinding {
                step_id: step_id.into(),
                descriptor_id: descriptor_id.into(),
                descriptor_revision: "1.0.0".into(),
                semantic_digest: routed_registry
                    .resolve(descriptor_id, "1.0.0")
                    .expect("routed descriptor")
                    .semantic_digest()
                    .clone(),
            })
            .collect(),
        })
        .expect("routed composite bindings");
    let routed_semantics = WorkflowRevisionSemanticContracts::create_with_optional_contracts(
        &routed_workflow,
        routed_bindings,
        routed_registry,
        revision
            .semantic_contracts
            .as_ref()
            .expect("composite semantics")
            .variable_contract()
            .clone(),
        None,
        Some(regions.clone()),
    )
    .expect("routed composite semantic contracts");
    let routed_definition_id = WorkflowDefinitionId::new();
    let routed_revision_id = WorkflowRevisionId::new();
    let routed_contract =
        WorkflowContract::from_spec(routed_workflow.clone()).expect("routed Workflow contract");
    let routed_revision = WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        routed_definition_id,
        routed_revision_id,
        routed_contract.clone(),
        vec![
            data_schema,
            input_configuration,
            iteration_configuration,
            output_configuration,
        ],
        routed_semantics,
        principal_id,
        now,
    )
    .expect("routed composite Workflow revision");
    let routed_definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        routed_definition_id,
        routed_workflow.name,
        routed_workflow.description,
        routed_revision_id,
        routed_contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("routed composite Workflow definition");
    let routed_compiled = WorkflowPlanCompiler::compile_goal(
        WorkflowGoalId::new(),
        PlanRevisionId::new(),
        WorkflowGoalContract::from_spec(WorkflowGoalSpec {
            name: "Routed composite goal".into(),
            workflow_definition_id: routed_definition_id,
            workflow_revision_id: routed_revision_id,
            workflow_digest: routed_contract.digest().clone(),
            ontology_id,
            ontology_revision_id,
            ontology_digest: ontology_contract.digest().clone(),
            environment_id: None,
            input: json!({}),
        })
        .expect("routed composite goal contract"),
        &routed_definition,
        &routed_revision,
        &ontology_revision,
        principal_id,
        now,
    )
    .expect("compiled routed composite plan");
    assert_eq!(
        routed_compiled.plan_revision.plan.schema,
        WORKFLOW_PLAN_SCHEMA_V11
    );
    assert_eq!(
        routed_compiled.plan_revision.plan.compiler_revision,
        WORKFLOW_PLAN_COMPILER_REVISION_V11
    );
    let routed_run = WorkflowRunCompiler::compile(
        WorkflowRunId::new(),
        &routed_compiled.goal,
        &routed_compiled.plan_revision,
        &routed_revision,
        None,
        principal_id,
        now,
    )
    .expect("runtime v22 routed composite input");
    assert_eq!(
        routed_run.run.execution_input.schema,
        WORKFLOW_RUN_INPUT_SCHEMA_V22
    );
    assert_eq!(
        routed_run.run.execution_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
    );
    assert_eq!(
        routed_run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V22
    );
    routed_run
        .run
        .execution_input
        .validate()
        .expect("valid runtime v22 routed composite input");

    let application_run = WorkflowRunCompiler::compile_for_application(
        WorkflowRunId::new(),
        &compiled.goal,
        &compiled.plan_revision,
        &revision,
        None,
        principal_id,
        now,
    )
    .expect("runtime v22 Application composite input");
    let application_input = &application_run.run.execution_input;
    assert_eq!(application_input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V22);
    assert_eq!(
        application_input.runtime_contract_revision,
        WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
    );
    assert_eq!(
        application_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V22
    );
    let application_projection = application_input
        .application_projection
        .as_ref()
        .expect("Application composite projection");
    assert_eq!(
        application_projection.schema,
        WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5
    );
    assert!(application_projection.projects_application_lifecycle());
    assert!(application_projection.supports_application_frames());
    application_input
        .validate()
        .expect("valid runtime v22 Application composite input");

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
