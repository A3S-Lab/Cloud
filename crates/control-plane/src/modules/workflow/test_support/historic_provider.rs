use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    Sha256Digest, WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{
    CapabilityReference, CapabilityType, Ontology, OntologyContract, OntologyName,
    OntologyObjectType, OntologyRecord, OntologyRevision, OntologySpec, PlanRevision,
    WorkflowContract, WorkflowDataSchema, WorkflowDataType, WorkflowDefinition,
    WorkflowDefinitionRecord, WorkflowEdgeSpec, WorkflowGoal, WorkflowGoalContract,
    WorkflowGoalRecord, WorkflowGoalSpec, WorkflowPayload, WorkflowPayloadContent, WorkflowPlan,
    WorkflowPlanStep, WorkflowRevision, WorkflowRun, WorkflowRunRecord, WorkflowSpec,
    WorkflowStepConfiguration, WorkflowStepKind, WorkflowStepSpec, WORKFLOW_PLAN_COMPILER_REVISION,
    WORKFLOW_PLAN_SCHEMA,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct HistoricProviderWorkflowFixture {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub principal_id: PrincipalId,
    pub definition: WorkflowDefinition,
    pub revision: WorkflowRevision,
    pub ontology: Ontology,
    pub ontology_revision: OntologyRevision,
    pub goal: WorkflowGoal,
    pub plan_revision: PlanRevision,
    pub run_record: WorkflowRunRecord,
}

impl HistoricProviderWorkflowFixture {
    pub fn definition_record(&self) -> WorkflowDefinitionRecord {
        WorkflowDefinitionRecord {
            definition: self.definition.clone(),
            revision: self.revision.clone(),
        }
    }

    pub fn ontology_record(&self) -> OntologyRecord {
        OntologyRecord {
            ontology: self.ontology.clone(),
            revision: self.ontology_revision.clone(),
        }
    }

    pub fn goal_record(&self) -> WorkflowGoalRecord {
        WorkflowGoalRecord {
            goal: self.goal.clone(),
            plan_revision: self.plan_revision.clone(),
        }
    }
}

pub(crate) fn historic_provider_workflow_fixture() -> HistoricProviderWorkflowFixture {
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
    let configuration = |kind| {
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(kind),
        ))
        .expect("step configuration")
    };
    let input_configuration = configuration(WorkflowStepKind::Input);
    let agent_configuration = configuration(WorkflowStepKind::Agent);
    let output_configuration = configuration(WorkflowStepKind::Output);
    let step = |id: &str, kind: WorkflowStepKind, configuration: &WorkflowPayload, capability| {
        WorkflowStepSpec {
            id: id.into(),
            label: id.into(),
            kind,
            configuration_digest: configuration.digest().clone(),
            input_schema_digest: data_schema.digest().clone(),
            output_schema_digest: data_schema.digest().clone(),
            policy_digest: None,
            capability,
        }
    };
    let workflow = WorkflowSpec {
        name: "Historic provider workflow".into(),
        description: String::new(),
        steps: vec![
            step("input", WorkflowStepKind::Input, &input_configuration, None),
            step(
                "agent",
                WorkflowStepKind::Agent,
                &agent_configuration,
                Some(CapabilityReference {
                    owner: CapabilityType::AgentRelease.owner(),
                    capability_type: CapabilityType::AgentRelease,
                    resource_id: Uuid::now_v7(),
                    revision: "release-1".into(),
                    digest: digest('d'),
                    capability: "agent.invoke".into(),
                }),
            ),
            step(
                "output",
                WorkflowStepKind::Output,
                &output_configuration,
                None,
            ),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-agent".into(),
                source: "input".into(),
                target: "agent".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "agent-output".into(),
                source: "agent".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    };
    let contract = WorkflowContract::from_spec(workflow.clone()).expect("workflow contract");
    let revision = WorkflowRevision::initial(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        contract.clone(),
        vec![
            data_schema,
            input_configuration,
            agent_configuration,
            output_configuration,
        ],
        principal_id,
        now,
    )
    .expect("historic revision remains structurally readable");
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        workflow.name.clone(),
        workflow.description.clone(),
        revision_id,
        contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("definition");

    let ontology_id = OntologyId::new();
    let ontology_revision_id = OntologyRevisionId::new();
    let ontology_contract = OntologyContract::from_spec(OntologySpec {
        name: "Historic provider ontology".into(),
        description: String::new(),
        object_types: vec![OntologyObjectType {
            id: "request".into(),
            label: "Request".into(),
            schema_digest: digest('e'),
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
    let ontology = Ontology::create(
        organization_id,
        project_id,
        ontology_id,
        OntologyName::parse(ontology_contract.spec().name.clone()).expect("ontology name"),
        ontology_contract.spec().description.clone(),
        ontology_revision_id,
        ontology_contract.digest().clone(),
        principal_id,
        now,
    )
    .expect("ontology");

    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "Historic provider goal".into(),
        workflow_definition_id: definition_id,
        workflow_revision_id: revision_id,
        workflow_digest: contract.digest().clone(),
        ontology_id,
        ontology_revision_id,
        ontology_digest: ontology_contract.digest().clone(),
        environment_id: None,
        input: json!({}),
    })
    .expect("goal contract");
    let goal_id = WorkflowGoalId::new();
    let plan_revision = PlanRevision::create(
        organization_id,
        project_id,
        goal_id,
        PlanRevisionId::new(),
        WorkflowPlan {
            schema: WORKFLOW_PLAN_SCHEMA.into(),
            compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION.into(),
            workflow_definition_id: definition_id,
            workflow_revision_id: revision_id,
            workflow_digest: contract.digest().clone(),
            workflow_payload_set_digest: revision.payload_set_digest.clone(),
            semantic_contract_set_digest: None,
            variable_contract_digest: None,
            composite_regions_digest: None,
            ontology_id,
            ontology_revision_id,
            ontology_digest: ontology_contract.digest().clone(),
            environment_id: None,
            input_digest: goal_contract.input_digest().clone(),
            steps: workflow
                .steps
                .iter()
                .map(|step| WorkflowPlanStep {
                    id: step.id.clone(),
                    kind: step.kind,
                    configuration_digest: step.configuration_digest.clone(),
                    input_schema_digest: step.input_schema_digest.clone(),
                    output_schema_digest: step.output_schema_digest.clone(),
                    policy_digest: step.policy_digest.clone(),
                    capability: step.capability.clone(),
                    descriptor: None,
                    failure: None,
                    default_output: None,
                })
                .collect(),
            edges: workflow.edges.clone(),
        },
        principal_id,
        now,
    )
    .expect("historic Plan remains readable");
    let goal = WorkflowGoal::create(
        organization_id,
        project_id,
        goal_id,
        goal_contract,
        &plan_revision,
        principal_id,
        now,
    )
    .expect("historic Goal remains readable");

    // The Run replay sentinel is independently valid. The handler test binds
    // it to a pre-existing idempotency record so compilation ordering is
    // observable even though this provider revision cannot create a new Run.
    let (run, steps) = WorkflowRun::create(
        super::workflow_run_input().expect("Run replay input"),
        principal_id,
    )
    .expect("Run replay record");

    HistoricProviderWorkflowFixture {
        organization_id,
        project_id,
        principal_id,
        definition,
        revision,
        ontology,
        ontology_revision,
        goal,
        plan_revision,
        run_record: WorkflowRunRecord { run, steps },
    }
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}
