use a3s_cloud_control_plane::infrastructure::connect_and_migrate;
use a3s_cloud_control_plane::modules::executions::domain::events::ExecutionTemplatePublished;
use a3s_cloud_control_plane::modules::executions::domain::{
    CreateExecutionTemplateRevision, ExecutionTemplateDefinition, ExecutionTemplateRevision,
    IExecutionTemplateRepository,
};
use a3s_cloud_control_plane::modules::executions::PostgresExecutionTemplateRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, EnvironmentId, ExecutionTemplateId,
    ExecutionTemplateRevisionId, FormId, FormReleaseId, IdempotencyRequest, OntologyId,
    OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, Sha256Digest,
    WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use a3s_cloud_control_plane::modules::workflow::domain::ResolvedWorkflowPayload;
use a3s_cloud_control_plane::modules::workflow::{
    CapabilityOwner, CapabilityReference, CapabilityType, WorkflowDataSchema, WorkflowDataType,
    WorkflowEdgeSpec, WorkflowPayload, WorkflowPayloadContent, WorkflowPlan, WorkflowPlanStep,
    WorkflowRunInput, WorkflowStepConfiguration, WorkflowStepKind, WORKFLOW_PLAN_COMPILER_REVISION,
    WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_PLAN_SCHEMA, WORKFLOW_RUN_FLOW_NAME,
    WORKFLOW_RUN_FLOW_VERSION, WORKFLOW_RUN_INPUT_SCHEMA, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
};
use a3s_orm::{
    sql_query, Database, DatabaseError, Executor, PostgresDialect, PostgresError, PostgresExecutor,
    PostgresTransaction, Query,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const DOCUMENT_FILE: &str = "workflow-run-process-death.json";
pub(super) const EXECUTION_STEP_ID: &str = "execute_task";

pub(super) struct Fixture {
    pub(super) executor: PostgresExecutor,
    pub(super) postgres_url: String,
    pub(super) state_dir: PathBuf,
    pub(super) document: ProbeDocument,
}

struct SeedTransaction<'a> {
    transaction: &'a PostgresTransaction,
}

impl<'a> SeedTransaction<'a> {
    const fn new(transaction: &'a PostgresTransaction) -> Self {
        Self { transaction }
    }

    async fn execute<Q>(&self, query: Q) -> Result<(), DatabaseError<PostgresError>>
    where
        Q: Query,
    {
        let query = query
            .compile(&PostgresDialect)
            .map_err(DatabaseError::Build)?;
        self.transaction
            .execute(&query)
            .await
            .map_err(DatabaseError::Execute)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ProbeDocument {
    pub(super) actor: PrincipalId,
    pub(super) terminal_input: WorkflowRunInput,
    pub(super) cancellation_input: WorkflowRunInput,
    pub(super) execution_input: WorkflowRunInput,
}

pub(super) async fn setup_fixture(postgres_url: String, state_dir: &Path) -> TestResult<Fixture> {
    let executor = connect_and_migrate(&postgres_url, 8).await?;
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let requested_at = canonical_timestamp(Utc::now());
    seed_identity_authority(
        &executor,
        organization_id,
        project_id,
        environment_id,
        actor,
        requested_at,
    )
    .await?;
    let execution_template =
        publish_execution_template(&executor, organization_id, project_id, actor, requested_at)
            .await?;
    let terminal_input = workflow_run_input(
        organization_id,
        project_id,
        WorkflowRunId::new(),
        requested_at,
        false,
    )?;
    let cancellation_input = workflow_run_input(
        organization_id,
        project_id,
        WorkflowRunId::new(),
        requested_at + Duration::milliseconds(1),
        true,
    )?;
    let execution_input = execution_workflow_run_input(
        organization_id,
        project_id,
        environment_id,
        WorkflowRunId::new(),
        requested_at + Duration::milliseconds(2),
        &execution_template,
    )?;
    seed_workflow_authority(&executor, &terminal_input, actor, "terminal").await?;
    seed_workflow_authority(&executor, &cancellation_input, actor, "cancellation").await?;
    seed_workflow_authority(&executor, &execution_input, actor, "execution").await?;
    let document = ProbeDocument {
        actor,
        terminal_input,
        cancellation_input,
        execution_input,
    };
    let document_path = state_dir.join(DOCUMENT_FILE);
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&document_path)?;
    serde_json::to_writer(&file, &document)?;
    file.sync_all()?;
    Ok(Fixture {
        executor,
        postgres_url,
        state_dir: state_dir.to_path_buf(),
        document,
    })
}

pub(super) fn load_document(state_dir: &Path) -> TestResult<ProbeDocument> {
    Ok(serde_json::from_slice(&std::fs::read(
        state_dir.join(DOCUMENT_FILE),
    )?)?)
}

fn workflow_run_input(
    organization_id: OrganizationId,
    project_id: ProjectId,
    workflow_run_id: WorkflowRunId,
    requested_at: DateTime<Utc>,
    human_decision: bool,
) -> Result<WorkflowRunInput, String> {
    let goal_input = if human_decision {
        serde_json::json!({"requestId": "REQ-CANCEL", "approved": false})
    } else {
        serde_json::json!({"ticketId": "T-PROCESS-DEATH", "priority": "high"})
    };
    let input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &goal_input,
        1024 * 1024,
        "WorkflowRun process-death input",
    )?))?;
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Input))?;
    let output_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Output))?;
    let mut payloads = vec![
        data_schema.clone(),
        input_configuration.clone(),
        output_configuration.clone(),
    ];
    let mut steps = vec![plan_step(
        "input",
        WorkflowStepKind::Input,
        &input_configuration,
        data_schema.digest(),
    )];
    let mut edges = Vec::new();
    if human_decision {
        let mut configuration = WorkflowStepConfiguration::empty(WorkflowStepKind::HumanDecision);
        configuration.message = Some("Approve the cancellation probe?".into());
        configuration.details =
            Some("The process-death gate intentionally leaves this open.".into());
        configuration.expires_after_seconds = Some(3_600);
        let decision_configuration = payload(configuration)?;
        let mut decision_step = plan_step(
            "human_review",
            WorkflowStepKind::HumanDecision,
            &decision_configuration,
            data_schema.digest(),
        );
        decision_step.capability = Some(CapabilityReference {
            owner: CapabilityOwner::Forms,
            capability_type: CapabilityType::FormRelease,
            resource_id: FormId::new().as_uuid(),
            revision: FormReleaseId::new().to_string(),
            digest: Sha256Digest::parse(digest('a'))?,
            capability: "form.interact".into(),
        });
        payloads.push(decision_configuration);
        steps.push(decision_step);
        edges.push(edge("input-review", "input", "human_review"));
        edges.push(edge("review-output", "human_review", "output"));
    } else {
        edges.push(edge("input-output", "input", "output"));
    }
    steps.push(plan_step(
        "output",
        WorkflowStepKind::Output,
        &output_configuration,
        data_schema.digest(),
    ));
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let plan = WorkflowPlan {
        schema: WORKFLOW_PLAN_SCHEMA.into(),
        compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION.into(),
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_digest: Sha256Digest::parse(digest('1'))?,
        workflow_payload_set_digest: digest_payload_set(&payloads)?,
        semantic_contract_set_digest: None,
        variable_contract_digest: None,
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: Sha256Digest::parse(digest('2'))?,
        environment_id: None,
        input_digest,
        steps,
        edges,
    };
    plan.validate()?;
    let plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun process-death plan",
    )?))?;
    let input = WorkflowRunInput {
        schema: WORKFLOW_RUN_INPUT_SCHEMA.into(),
        runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
        flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
        flow_workflow_version: WORKFLOW_RUN_FLOW_VERSION.into(),
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
        variable_contract: None,
        variable_defaults: None,
        requested_at,
        deadline_at: requested_at + Duration::hours(1),
    };
    input.validate()?;
    Ok(input)
}

fn execution_workflow_run_input(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workflow_run_id: WorkflowRunId,
    requested_at: DateTime<Utc>,
    template: &ExecutionTemplateRevision,
) -> Result<WorkflowRunInput, String> {
    let goal_input = serde_json::json!({
        "ticketId": "T-EXECUTION-PROCESS-DEATH",
        "release": "2026.08"
    });
    let input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &goal_input,
        1024 * 1024,
        "WorkflowRun Execution process-death input",
    )?))?;
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Input))?;
    let execution_configuration = payload(WorkflowStepConfiguration::empty(
        WorkflowStepKind::Execution,
    ))?;
    let output_configuration = payload(WorkflowStepConfiguration::empty(WorkflowStepKind::Output))?;
    let mut payloads = vec![
        data_schema.clone(),
        input_configuration.clone(),
        execution_configuration.clone(),
        output_configuration.clone(),
    ];
    let mut execution_step = plan_step(
        EXECUTION_STEP_ID,
        WorkflowStepKind::Execution,
        &execution_configuration,
        data_schema.digest(),
    );
    execution_step.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Executions,
        capability_type: CapabilityType::ExecutionTemplate,
        resource_id: template.template_id.as_uuid(),
        revision: template.revision_id.to_string(),
        digest: template.definition.digest().clone(),
        capability: "execution.run".into(),
    });
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let plan = WorkflowPlan {
        schema: WORKFLOW_PLAN_SCHEMA.into(),
        compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION.into(),
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_digest: Sha256Digest::parse(digest('4'))?,
        workflow_payload_set_digest: digest_payload_set(&payloads)?,
        semantic_contract_set_digest: None,
        variable_contract_digest: None,
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: Sha256Digest::parse(digest('5'))?,
        environment_id: Some(environment_id),
        input_digest,
        steps: vec![
            plan_step(
                "input",
                WorkflowStepKind::Input,
                &input_configuration,
                data_schema.digest(),
            ),
            execution_step,
            plan_step(
                "output",
                WorkflowStepKind::Output,
                &output_configuration,
                data_schema.digest(),
            ),
        ],
        edges: vec![
            edge("input-execution", "input", EXECUTION_STEP_ID),
            edge("execution-output", EXECUTION_STEP_ID, "output"),
        ],
    };
    plan.validate()?;
    let plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun Execution process-death plan",
    )?))?;
    let input = WorkflowRunInput {
        schema: WORKFLOW_RUN_INPUT_SCHEMA.into(),
        runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
        flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
        flow_workflow_version: WORKFLOW_RUN_FLOW_VERSION.into(),
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
        variable_contract: None,
        variable_defaults: None,
        requested_at,
        deadline_at: requested_at + Duration::hours(1),
    };
    input.validate()?;
    Ok(input)
}

fn payload(configuration: WorkflowStepConfiguration) -> Result<WorkflowPayload, String> {
    WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(configuration))
}

fn plan_step(
    id: &str,
    kind: WorkflowStepKind,
    configuration: &WorkflowPayload,
    schema_digest: &Sha256Digest,
) -> WorkflowPlanStep {
    WorkflowPlanStep {
        id: id.into(),
        kind,
        configuration_digest: configuration.digest().clone(),
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest.clone(),
        policy_digest: None,
        capability: None,
        descriptor: None,
    }
}

fn edge(id: &str, source: &str, target: &str) -> WorkflowEdgeSpec {
    WorkflowEdgeSpec {
        id: id.into(),
        source: source.into(),
        target: target.into(),
        source_handle: None,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PayloadDigestEntry<'a> {
    kind: &'a str,
    schema: &'a str,
    digest: &'a str,
}

fn digest_payload_set(payloads: &[WorkflowPayload]) -> Result<Sha256Digest, String> {
    let entries = payloads
        .iter()
        .map(|payload| PayloadDigestEntry {
            kind: payload.kind().as_str(),
            schema: payload.schema(),
            digest: payload.digest().as_str(),
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_vec(&entries).map_err(|error| error.to_string())?;
    Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(encoded)))
}

async fn publish_execution_template(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    created_at: DateTime<Utc>,
) -> TestResult<ExecutionTemplateRevision> {
    let definition = ExecutionTemplateDefinition::parse_acl(include_str!(
        "../../../../contracts/w0.3/execution-template.acl"
    ))?;
    let revision = ExecutionTemplateRevision::create(
        organization_id,
        project_id,
        ExecutionTemplateId::new(),
        ExecutionTemplateRevisionId::new(),
        definition,
        actor,
        created_at,
    )?;
    let request_id = Uuid::new_v5(
        &revision.revision_id.as_uuid(),
        b"workflow-run-process-death-template-publication",
    );
    let idempotency = IdempotencyRequest::new(
        format!("organizations/{organization_id}/projects/{project_id}/execution-templates"),
        "workflow-run-process-death-template",
        revision.definition.canonical_acl().as_bytes(),
    )?;
    let write = PostgresExecutionTemplateRepository::new(executor.clone())
        .create(CreateExecutionTemplateRevision {
            event: ExecutionTemplatePublished::envelope(&revision, request_id)?,
            revision,
            actor_principal_id: actor,
            request_id,
            idempotency,
        })
        .await?;
    if write.replayed {
        return Err("fresh process-death fixture replayed ExecutionTemplate publication".into());
    }
    Ok(write.value)
}

async fn seed_identity_authority(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    actor: PrincipalId,
    created_at: DateTime<Utc>,
) -> TestResult {
    let database = Database::new(PostgresDialect, executor.clone());
    database
        .execute(
            sql_query::<()>("insert into organizations (id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", 'WorkflowRun recovery organization', 'workflow-run-recovery-organization', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", 'WorkflowRun recovery project', 'workflow-run-recovery-project', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", 'WorkflowRun recovery executions', 'workflow-run-recovery-executions', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'WorkflowRun recovery actor', 1, ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (")
                .bind(Uuid::now_v7())
                .append(", ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(actor.as_uuid())
                .append(", 'member', 1, ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    Ok(())
}

async fn seed_workflow_authority(
    executor: &PostgresExecutor,
    input: &WorkflowRunInput,
    actor: PrincipalId,
    label: &str,
) -> TestResult {
    let input = input.clone();
    let label = label.to_owned();
    let canonical_plan = String::from_utf8(canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun recovery seeded plan",
    )?)?;
    let ontology_name_key = format!("workflow-run-recovery-{label}-ontology");
    let workflow_name_key = format!("workflow-run-recovery-{label}");
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let database = SeedTransaction::new(transaction);
                database
                    .execute(
                        sql_query::<()>("insert into ontologies (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(input.organization_id.as_uuid()).append(", ")
                            .bind(input.project_id.as_uuid()).append(", ")
                            .bind(input.plan.ontology_id.as_uuid()).append(", ")
                            .bind(format!("WorkflowRun {label} ontology")).append(", ")
                            .bind(ontology_name_key).append(", '', ")
                            .bind(input.plan.ontology_revision_id.as_uuid()).append(", 1, ")
                            .bind(input.plan.ontology_digest.as_str()).append(", 1, ")
                            .bind(actor.as_uuid()).append(", ")
                            .bind(input.requested_at).append(", ")
                            .bind(input.requested_at).append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into ontology_revisions (organization_id, project_id, ontology_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, migration_policy, migration_rule_id, migration_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid()).append(", ")
                            .bind(input.project_id.as_uuid()).append(", ")
                            .bind(input.plan.ontology_id.as_uuid()).append(", ")
                            .bind(input.plan.ontology_revision_id.as_uuid()).append(", 1, null, null, 'cloud.workflow.ontology.v1', 1, 'ontology \"workflow_run_recovery\" {}', ")
                            .bind(input.plan.ontology_digest.as_str()).append(", 'initial', null, null, ")
                            .bind(actor.as_uuid()).append(", ")
                            .bind(input.requested_at).append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_definitions (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(input.organization_id.as_uuid()).append(", ")
                            .bind(input.project_id.as_uuid()).append(", ")
                            .bind(input.plan.workflow_definition_id.as_uuid()).append(", ")
                            .bind(format!("WorkflowRun recovery {label}")).append(", ")
                            .bind(workflow_name_key).append(", '', ")
                            .bind(input.plan.workflow_revision_id.as_uuid()).append(", 1, ")
                            .bind(input.plan.workflow_digest.as_str()).append(", 1, ")
                            .bind(actor.as_uuid()).append(", ")
                            .bind(input.requested_at).append(", ")
                            .bind(input.requested_at).append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_revisions (organization_id, project_id, workflow_definition_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, payload_set_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid()).append(", ")
                            .bind(input.project_id.as_uuid()).append(", ")
                            .bind(input.plan.workflow_definition_id.as_uuid()).append(", ")
                            .bind(input.plan.workflow_revision_id.as_uuid()).append(", 1, null, null, 'cloud.workflow.definition.v1', 1, 'workflow \"workflow_run_recovery\" {}', ")
                            .bind(input.plan.workflow_digest.as_str()).append(", ")
                            .bind(input.plan.workflow_payload_set_digest.as_str()).append(", ")
                            .bind(actor.as_uuid()).append(", ")
                            .bind(input.requested_at).append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_goals (organization_id, project_id, id, name, contract_schema, canonical_acl, contract_digest, input_digest, workflow_definition_id, workflow_revision_id, workflow_digest, ontology_id, ontology_revision_id, ontology_digest, environment_id, plan_revision_id, plan_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid()).append(", ")
                            .bind(input.project_id.as_uuid()).append(", ")
                            .bind(input.workflow_goal_id.as_uuid()).append(", ")
                            .bind(format!("WorkflowRun recovery {label} goal")).append(", 'cloud.workflow.goal.v1', 'goal \"workflow_run_recovery\" {}', ")
                            .bind(digest('3')).append(", ")
                            .bind(input.plan.input_digest.as_str()).append(", ")
                            .bind(input.plan.workflow_definition_id.as_uuid()).append(", ")
                            .bind(input.plan.workflow_revision_id.as_uuid()).append(", ")
                            .bind(input.plan.workflow_digest.as_str()).append(", ")
                            .bind(input.plan.ontology_id.as_uuid()).append(", ")
                            .bind(input.plan.ontology_revision_id.as_uuid()).append(", ")
                            .bind(input.plan.ontology_digest.as_str()).append(", ")
                            .bind(input.plan.environment_id.map(|id| id.as_uuid())).append(", ")
                            .bind(input.plan_revision_id.as_uuid()).append(", ")
                            .bind(input.plan_digest.as_str()).append(", ")
                            .bind(actor.as_uuid()).append(", ")
                            .bind(input.requested_at).append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_plan_revisions (organization_id, project_id, workflow_goal_id, id, plan_schema, compiler_revision, canonical_plan, plan_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid()).append(", ")
                            .bind(input.project_id.as_uuid()).append(", ")
                            .bind(input.workflow_goal_id.as_uuid()).append(", ")
                            .bind(input.plan_revision_id.as_uuid()).append(", 'cloud.workflow.plan.v1', 'cloud.workflow.plan-compiler.v1', ")
                            .bind(canonical_plan).append(", ")
                            .bind(input.plan_digest.as_str()).append(", ")
                            .bind(actor.as_uuid()).append(", ")
                            .bind(input.requested_at).append(")"),
                    )
                    .await?;
                Ok::<(), DatabaseError<PostgresError>>(())
            })
        })
        .await?;
    Ok(())
}

fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    value - Duration::nanoseconds(i64::from(value.nanosecond() % 1_000))
}

pub(super) fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
