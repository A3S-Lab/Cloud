use super::fixture::{
    digest_payload_set, edge, payload, plan_step, ITERATION_STEP_ID, LOOP_STEP_ID,
};
use super::TestResult;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, IdempotencyRequest, OntologyId, OntologyRevisionId,
    OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId,
    WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use a3s_cloud_control_plane::modules::workflow::domain::{
    CreateOntologyWrite, OntologyRecord, OntologyRevisionPublished,
    ResolvedWorkflowCompositeRegions, ResolvedWorkflowPayload, ResolvedWorkflowVariableContract,
    WorkflowCompositeRegionPolicy, WorkflowCompositeRegions, WorkflowCompositeRegionsSpec,
    WorkflowIterationFailureMode, WorkflowIterationRegionPolicy, WorkflowLoopRegionPolicy,
    WorkflowStepDescriptorBinding, WorkflowVariableContract, WorkflowVariableContractSpec,
    WorkflowVariableDeclaration, WorkflowVariableMutationMode, WorkflowVariableScope,
    WorkflowVariableStorageClass,
};
use a3s_cloud_control_plane::modules::workflow::{
    CapabilityOwner, CapabilityReference, CapabilityType, CreateWorkflowDefinitionWrite,
    IOntologyRepository, IWorkflowDefinitionRepository, Ontology, OntologyContract, OntologyName,
    OntologyObjectType, OntologyRevision, OntologySpec, PostgresOntologyRepository,
    PostgresWorkflowDefinitionRepository, WorkflowContract, WorkflowDataSchema, WorkflowDataType,
    WorkflowDefinition, WorkflowDefinitionRecord, WorkflowPayload, WorkflowPayloadContent,
    WorkflowPlan, WorkflowRevision, WorkflowRevisionPublished, WorkflowRunInput, WorkflowSpec,
    WorkflowStepConfiguration, WorkflowStepKind, WorkflowStepSpec,
    WORKFLOW_PLAN_COMPILER_REVISION_V2, WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_PLAN_SCHEMA_V2,
    WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION_V22, WORKFLOW_RUN_FLOW_VERSION_V3,
    WORKFLOW_RUN_INPUT_SCHEMA_V22, WORKFLOW_RUN_INPUT_SCHEMA_V3,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3,
};
use a3s_orm::PostgresExecutor;
use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

pub(super) struct CompositeAuthority {
    ontology_id: OntologyId,
    ontology_revision_id: OntologyRevisionId,
    ontology_digest: Sha256Digest,
    child_workflow_definition_id: WorkflowDefinitionId,
    child_workflow_revision_id: WorkflowRevisionId,
    child_workflow_digest: Sha256Digest,
}

pub(super) fn loop_workflow_run_input(
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    requested_at: DateTime<Utc>,
    authority: &CompositeAuthority,
    goal_input: serde_json::Value,
) -> Result<WorkflowRunInput, String> {
    composite_workflow_run_input(
        organization_id,
        project_id,
        workflow_run_id,
        requested_at,
        authority,
        WorkflowCompositeRegionPolicy::Loop(WorkflowLoopRegionPolicy {
            step_id: LOOP_STEP_ID.into(),
            maximum_iterations: 3,
            time_budget_seconds: 3_600,
            termination_path: vec!["done".into()],
        }),
        goal_input,
    )
}

pub(super) fn iteration_workflow_run_input(
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    requested_at: DateTime<Utc>,
    authority: &CompositeAuthority,
    goal_input: serde_json::Value,
) -> Result<WorkflowRunInput, String> {
    composite_workflow_run_input(
        organization_id,
        project_id,
        workflow_run_id,
        requested_at,
        authority,
        WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
            step_id: ITERATION_STEP_ID.into(),
            maximum_items: 2,
            maximum_concurrency: 2,
            failure_mode: WorkflowIterationFailureMode::Terminate,
        }),
        goal_input,
    )
}

#[allow(clippy::too_many_arguments)]
fn composite_workflow_run_input(
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    requested_at: DateTime<Utc>,
    authority: &CompositeAuthority,
    policy: WorkflowCompositeRegionPolicy,
    goal_input: serde_json::Value,
) -> Result<WorkflowRunInput, String> {
    let step_id = policy.step_id().to_owned();
    let semantic_profile = policy.semantic_profile().to_owned();
    let parallel_iteration = matches!(
        &policy,
        WorkflowCompositeRegionPolicy::Iteration(iteration)
            if iteration.maximum_concurrency > 1
    );
    let input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &goal_input,
        1024 * 1024,
        "WorkflowRun composite process-death input",
    )?))?;
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Input))?;
    let composite_configuration = payload(WorkflowStepConfiguration::empty(
        WorkflowStepKind::Subworkflow,
    ))?;
    let output_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Output))?;
    let schema_digest = data_schema.digest().clone();
    let mut payloads = vec![
        data_schema,
        input_configuration.clone(),
        composite_configuration.clone(),
        output_configuration.clone(),
    ];
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "recovery.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![WorkflowVariableDeclaration {
            name: "request".into(),
            scope: WorkflowVariableScope::InvocationInput,
            value_type: WorkflowDataType::Any,
            value_schema_digest: schema_digest.clone(),
            source_schema_digest: Some(schema_digest.clone()),
            storage_class: WorkflowVariableStorageClass::Inline,
            mutation_mode: WorkflowVariableMutationMode::Immutable,
            required: true,
            source_step_id: None,
            source_path: Vec::new(),
            region_id: None,
            default_value_digest: None,
        }],
        reads: Vec::new(),
        assignments: Vec::new(),
        exports: Vec::new(),
    })?;
    let regions = WorkflowCompositeRegions::from_spec(WorkflowCompositeRegionsSpec {
        id: "recovery.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        regions: vec![policy],
    })?;
    let semantic_digest = Sha256Digest::from_bytes(b"WorkflowRun process-death descriptors");
    let descriptor = |id: &str, kind: WorkflowStepKind| WorkflowStepDescriptorBinding {
        step_id: id.into(),
        descriptor_id: format!("workflow.{}", kind.as_str()),
        descriptor_revision: "1.0.0".into(),
        semantic_digest: semantic_digest.clone(),
    };
    let mut input_step = plan_step(
        "input",
        WorkflowStepKind::Input,
        &input_configuration,
        &schema_digest,
    );
    input_step.descriptor = Some(descriptor("input", WorkflowStepKind::Input));
    let mut composite_step = plan_step(
        &step_id,
        WorkflowStepKind::Subworkflow,
        &composite_configuration,
        &schema_digest,
    );
    composite_step.descriptor = Some(WorkflowStepDescriptorBinding {
        step_id: step_id.clone(),
        descriptor_id: semantic_profile,
        descriptor_revision: "1.0.0".into(),
        semantic_digest: semantic_digest.clone(),
    });
    composite_step.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Workflow,
        capability_type: CapabilityType::WorkflowRevision,
        resource_id: authority.child_workflow_definition_id.as_uuid(),
        revision: authority.child_workflow_revision_id.to_string(),
        digest: authority.child_workflow_digest.clone(),
        capability: "workflow.run".into(),
    });
    let mut output_step = plan_step(
        "output",
        WorkflowStepKind::Output,
        &output_configuration,
        &schema_digest,
    );
    output_step.descriptor = Some(descriptor("output", WorkflowStepKind::Output));
    let plan = WorkflowPlan {
        schema: WORKFLOW_PLAN_SCHEMA_V2.into(),
        compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION_V2.into(),
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_digest: Sha256Digest::from_bytes(
            format!("WorkflowRun process-death parent {step_id}").as_bytes(),
        ),
        workflow_payload_set_digest: digest_payload_set(&payloads)?,
        semantic_contract_set_digest: Some(Sha256Digest::from_bytes(
            format!("WorkflowRun process-death semantic set {step_id}").as_bytes(),
        )),
        variable_contract_digest: Some(variables.digest().clone()),
        composite_regions_digest: Some(regions.digest().clone()),
        ontology_id: authority.ontology_id,
        ontology_revision_id: authority.ontology_revision_id,
        ontology_digest: authority.ontology_digest.clone(),
        environment_id: None,
        input_digest,
        steps: vec![input_step, composite_step, output_step],
        edges: vec![
            edge("input-composite", "input", &step_id),
            edge("composite-output", &step_id, "output"),
        ],
    };
    plan.validate()?;
    regions.validate_plan(&plan)?;
    let plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun composite process-death plan",
    )?))?;
    let input = WorkflowRunInput {
        schema: if parallel_iteration {
            WORKFLOW_RUN_INPUT_SCHEMA_V22
        } else {
            WORKFLOW_RUN_INPUT_SCHEMA_V3
        }
        .into(),
        runtime_contract_revision: if parallel_iteration {
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V22
        } else {
            WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3
        }
        .into(),
        flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
        flow_workflow_version: if parallel_iteration {
            WORKFLOW_RUN_FLOW_VERSION_V22
        } else {
            WORKFLOW_RUN_FLOW_VERSION_V3
        }
        .into(),
        organization_id,
        project_id,
        workflow_run_id,
        workflow_goal_id: WorkflowGoalId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest,
        plan,
        goal_input,
        payloads: payloads
            .iter()
            .map(ResolvedWorkflowPayload::from_payload)
            .collect(),
        variable_contract: Some(ResolvedWorkflowVariableContract {
            canonical_acl: variables.canonical_acl().to_owned(),
            digest: variables.digest().clone(),
        }),
        variable_defaults: None,
        composite_regions: Some(ResolvedWorkflowCompositeRegions {
            canonical_acl: regions.canonical_acl().to_owned(),
            digest: regions.digest().clone(),
        }),
        application_projection: None,
        requested_at,
        deadline_at: requested_at + Duration::hours(1),
    };
    input.validate()?;
    Ok(input)
}

pub(super) async fn publish_composite_authority(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    created_at: DateTime<Utc>,
) -> TestResult<CompositeAuthority> {
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "WorkflowRun recovery composite ontology".into(),
        description: String::new(),
        object_types: vec![OntologyObjectType {
            id: "request".into(),
            label: "Request".into(),
            schema_digest: Sha256Digest::from_bytes(b"WorkflowRun recovery request schema"),
            key_fields: vec!["ticketId".into()],
        }],
        relation_types: Vec::new(),
        rules: Vec::new(),
    })?;
    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_revision = OntologyRevision::initial(
        organization_id,
        project_id,
        ontology_id,
        ontology_revision_id,
        ontology_contract.clone(),
        actor,
        created_at,
    );
    let ontology = Ontology::create(
        organization_id,
        project_id,
        ontology_id,
        OntologyName::parse(ontology_contract.spec().name.clone())?,
        ontology_contract.spec().description.clone(),
        ontology_revision_id,
        ontology_contract.digest().clone(),
        actor,
        created_at,
    )?;
    let ontology_request_id = Uuid::new_v5(
        &ontology_revision_id.as_uuid(),
        b"workflow-run-process-death-composite-ontology",
    );
    let ontology_write = PostgresOntologyRepository::new(executor.clone())
        .create(CreateOntologyWrite {
            event: OntologyRevisionPublished::created(
                &ontology,
                &ontology_revision,
                ontology_request_id,
            )?,
            record: OntologyRecord {
                ontology,
                revision: ontology_revision,
            },
            actor_principal_id: actor,
            request_id: ontology_request_id,
            idempotency: IdempotencyRequest::new(
                format!("organizations/{organization_id}/projects/{project_id}/ontologies"),
                "workflow-run-process-death-composite",
                ontology_contract.canonical_acl().as_bytes(),
            )?,
        })
        .await?;
    if ontology_write.replayed {
        return Err("fresh process-death fixture replayed composite Ontology publication".into());
    }

    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Input))?;
    let output_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Output))?;
    let schema_digest = data_schema.digest().clone();
    let mut child_payloads = vec![
        data_schema,
        input_configuration.clone(),
        output_configuration.clone(),
    ];
    child_payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let child_contract = WorkflowContract::from_spec(WorkflowSpec {
        name: "WorkflowRun recovery composite child".into(),
        description: String::new(),
        steps: vec![
            WorkflowStepSpec {
                id: "input".into(),
                label: "Input".into(),
                kind: WorkflowStepKind::Input,
                configuration_digest: input_configuration.digest().clone(),
                input_schema_digest: schema_digest.clone(),
                output_schema_digest: schema_digest.clone(),
                policy_digest: None,
                capability: None,
            },
            WorkflowStepSpec {
                id: "output".into(),
                label: "Output".into(),
                kind: WorkflowStepKind::Output,
                configuration_digest: output_configuration.digest().clone(),
                input_schema_digest: schema_digest.clone(),
                output_schema_digest: schema_digest,
                policy_digest: None,
                capability: None,
            },
        ],
        edges: vec![edge("input-output", "input", "output")],
    })?;
    let child_workflow_definition_id = WorkflowDefinitionId::new();
    let child_workflow_revision_id = WorkflowRevisionId::new();
    let child_revision = WorkflowRevision::initial(
        organization_id,
        project_id,
        child_workflow_definition_id,
        child_workflow_revision_id,
        child_contract.clone(),
        child_payloads,
        actor,
        created_at,
    )?;
    let child_definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        child_workflow_definition_id,
        child_contract.spec().name.clone(),
        child_contract.spec().description.clone(),
        child_workflow_revision_id,
        child_contract.digest().clone(),
        actor,
        created_at,
    )?;
    let workflow_request_id = Uuid::new_v5(
        &child_workflow_revision_id.as_uuid(),
        b"workflow-run-process-death-composite-child",
    );
    let workflow_write = PostgresWorkflowDefinitionRepository::new(executor.clone())
        .create(CreateWorkflowDefinitionWrite {
            event: WorkflowRevisionPublished::created(
                &child_definition,
                &child_revision,
                workflow_request_id,
            )?,
            record: WorkflowDefinitionRecord {
                definition: child_definition,
                revision: child_revision,
            },
            actor_principal_id: actor,
            request_id: workflow_request_id,
            idempotency: IdempotencyRequest::new(
                format!(
                    "organizations/{organization_id}/projects/{project_id}/workflow-definitions"
                ),
                "workflow-run-process-death-composite-child",
                child_contract.canonical_acl().as_bytes(),
            )?,
        })
        .await?;
    if workflow_write.replayed {
        return Err(
            "fresh process-death fixture replayed composite child Workflow publication".into(),
        );
    }

    Ok(CompositeAuthority {
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        child_workflow_definition_id,
        child_workflow_revision_id,
        child_workflow_digest: child_contract.digest().clone(),
    })
}

#[test]
fn composite_process_death_inputs_are_valid_without_postgres() -> Result<(), String> {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let requested_at = Utc::now();
    let authority = CompositeAuthority {
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: Sha256Digest::from_bytes(b"composite fixture ontology"),
        child_workflow_definition_id: WorkflowDefinitionId::new(),
        child_workflow_revision_id: WorkflowRevisionId::new(),
        child_workflow_digest: Sha256Digest::from_bytes(b"composite fixture child workflow"),
    };
    let loop_input = loop_workflow_run_input(
        organization_id,
        project_id,
        WorkflowRunId::new(),
        requested_at,
        &authority,
        serde_json::json!({"ticketId": "T-LOOP", "done": true}),
    )?;
    let iteration_input = iteration_workflow_run_input(
        organization_id,
        project_id,
        WorkflowRunId::new(),
        requested_at,
        &authority,
        serde_json::json!([
            {"ticketId": "T-ITERATION-A"},
            {"ticketId": "T-ITERATION-B"}
        ]),
    )?;

    assert_eq!(loop_input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V3);
    assert_eq!(
        loop_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V3
    );
    assert_eq!(iteration_input.schema, WORKFLOW_RUN_INPUT_SCHEMA_V22);
    assert_eq!(
        iteration_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V22
    );
    assert_eq!(loop_input.plan.steps[1].id, LOOP_STEP_ID);
    assert_eq!(iteration_input.plan.steps[1].id, ITERATION_STEP_ID);
    assert_eq!(
        loop_input.plan.steps[1]
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.descriptor_id.as_str()),
        Some("workflow.loop")
    );
    assert_eq!(
        iteration_input.plan.steps[1]
            .descriptor
            .as_ref()
            .map(|descriptor| descriptor.descriptor_id.as_str()),
        Some("workflow.iteration")
    );
    Ok(())
}
