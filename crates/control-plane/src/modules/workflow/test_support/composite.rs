use super::*;

pub(crate) fn composite_workflow_run_input(
    policy: WorkflowCompositeRegionPolicy,
    goal_input: serde_json::Value,
) -> Result<WorkflowRunInput, String> {
    let step_id = policy.step_id().to_owned();
    let input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &goal_input,
        1024 * 1024,
        "WorkflowRun composite test input",
    )?))?;
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Input))?;
    let composite_configuration = configuration(WorkflowStepConfiguration::empty(
        WorkflowStepKind::Subworkflow,
    ))?;
    let output_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Output))?;
    let schema_digest = data_schema.digest().clone();
    let mut payloads = vec![
        data_schema,
        input_configuration.clone(),
        composite_configuration.clone(),
        output_configuration.clone(),
    ];
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let workflow_payload_set_digest = digest_payload_set(&payloads)?;
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.composite".into(),
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
        id: "support.composite".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        regions: vec![policy],
    })?;
    let semantic_digest = Sha256Digest::parse(digest('8'))?;
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
    composite_step.descriptor = Some(descriptor(&step_id, WorkflowStepKind::Subworkflow));
    composite_step.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Workflow,
        capability_type: CapabilityType::WorkflowRevision,
        resource_id: WorkflowDefinitionId::new().as_uuid(),
        revision: WorkflowRevisionId::new().to_string(),
        digest: Sha256Digest::parse(digest('c'))?,
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
        workflow_digest: Sha256Digest::parse(digest('1'))?,
        workflow_payload_set_digest,
        semantic_contract_set_digest: Some(Sha256Digest::parse(digest('9'))?),
        variable_contract_digest: Some(variables.digest().clone()),
        composite_regions_digest: Some(regions.digest().clone()),
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: Sha256Digest::parse(digest('2'))?,
        environment_id: None,
        input_digest,
        steps: vec![input_step, composite_step, output_step],
        edges: vec![
            edge("input-composite", "input", &step_id, None),
            edge("composite-output", &step_id, "output", None),
        ],
    };
    plan.validate()?;
    variables.validate_graph_bindings(&plan.workflow_spec()?)?;
    regions.validate_plan(&plan)?;
    let plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun composite test plan",
    )?))?;
    let input = WorkflowRunInput {
        schema: WORKFLOW_RUN_INPUT_SCHEMA_V3.into(),
        runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3.into(),
        flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
        flow_workflow_version: WORKFLOW_RUN_FLOW_VERSION_V3.into(),
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        workflow_run_id: WorkflowRunId::new(),
        workflow_goal_id: WorkflowGoalId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest,
        plan,
        goal_input,
        payloads: payloads
            .iter()
            .map(ResolvedWorkflowPayload::from_payload)
            .collect(),
        variable_contract: Some(
            crate::modules::workflow::domain::ResolvedWorkflowVariableContract::from_contract(
                &variables,
            ),
        ),
        variable_defaults: None,
        composite_regions: Some(ResolvedWorkflowCompositeRegions::from_regions(&regions)),
        application_projection: None,
        requested_at: timestamp(8, 0),
        deadline_at: timestamp(9, 0),
    };
    input.validate()?;
    Ok(input)
}
