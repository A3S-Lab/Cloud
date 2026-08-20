use crate::migrate_and_connect_for_test;
use a3s_cloud_control_plane::infrastructure::FlowInfrastructure;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use a3s_cloud_control_plane::modules::workflow::domain::{
    CreateOntologyWrite, OntologyRecord, OntologyRevisionPublished, WorkflowCompositeRegionPolicy,
    WorkflowCompositeRegions, WorkflowCompositeRegionsSpec, WorkflowIterationFailureMode,
    WorkflowIterationRegionPolicy, WorkflowRevisionSemanticContracts,
    WorkflowStepDescriptorBinding, WorkflowStepDescriptorBindings,
    WorkflowStepDescriptorBindingsSpec,
};
use a3s_cloud_control_plane::modules::workflow::{
    CapabilityOwner, CapabilityReference, CapabilityType, CreateWorkflowDefinitionWrite,
    CreateWorkflowGoalWrite, CreateWorkflowRunWrite, IOntologyRepository,
    IWorkflowDefinitionRepository, IWorkflowGoalRepository, IWorkflowRunRepository,
    IWorkflowRunVariableReader, Ontology, OntologyName, PostgresOntologyRepository,
    PostgresWorkflowDefinitionRepository, PostgresWorkflowGoalRepository,
    PostgresWorkflowRunRepository, WorkflowContract, WorkflowDataSchema, WorkflowDataType,
    WorkflowDefinition, WorkflowDefinitionRecord, WorkflowEdgeSpec, WorkflowGoalCompiled,
    WorkflowGoalRecord, WorkflowPayload, WorkflowPayloadContent, WorkflowPlanCompiler,
    WorkflowRevision, WorkflowRevisionPublished, WorkflowRunCompiler, WorkflowRunFlowRuntime,
    WorkflowRunRecord, WorkflowRunRequested, WorkflowRunVariableReader, WorkflowSpec,
    WorkflowStepConfiguration, WorkflowStepKind, WorkflowVariableContract,
    WorkflowVariableContractSpec, WorkflowVariableDeclaration, WorkflowVariableDefault,
    WorkflowVariableDefaults, WorkflowVariableDefaultsSpec, WorkflowVariableMutationMode,
    WorkflowVariableRead, WorkflowVariableReadMode, WorkflowVariableScope,
    WorkflowVariableStorageClass, WORKFLOW_RUN_FLOW_VERSION_V2,
};
use a3s_flow::{WorkflowRunStatus as FlowWorkflowRunStatus, WorkflowSpec as FlowWorkflowSpec};
use a3s_orm::{
    sql_query, Database, DatabaseError, ExecuteResult, PostgresDialect, PostgresError,
    PostgresExecutor,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

#[path = "workflow_semantic_contract_descriptors.rs"]
mod workflow_semantic_contract_descriptors;

use workflow_semantic_contract_descriptors::{descriptor_registry, workflow_step};

pub(super) async fn exercise_workflow_semantic_contract_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let semantic_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("103"),
        )
        .await?;
    assert_eq!(
        semantic_migration_state,
        (1, "Workflow revision semantic contracts and plan v2".into())
    );
    let run_input_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("105"),
        )
        .await?;
    assert_eq!(
        run_input_migration_state,
        (1, "WorkflowRun input v2 capacity".into())
    );
    let defaults_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("107"),
        )
        .await?;
    assert_eq!(
        defaults_migration_state,
        (1, "immutable Workflow variable default material".into())
    );
    let composite_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("108"),
        )
        .await?;
    assert_eq!(
        composite_migration_state,
        (1, "immutable Workflow composite-region policies".into())
    );
    let variable_table_count = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from information_schema.tables where table_schema = 'public' and table_name in ('workflow_run_variables', 'workflow_variable_values', 'workflow_variable_events', 'workflow_composite_regions', 'workflow_iterations', 'workflow_loops', 'workflow_region_events')",
            ),
        )
        .await?;
    assert_eq!(variable_table_count, 0);

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Workflow semantic tenant', ")
            .bind(format!("workflow-semantic-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'Workflow semantic publisher', 1, ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", 'Workflow semantics', 'workflow-semantics', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let definition_id = WorkflowDefinitionId::new();
    let revision_id = WorkflowRevisionId::new();
    let revision = semantic_revision(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        actor,
        created_at,
        false,
    );
    let definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        definition_id,
        revision.contract.spec().name.clone(),
        revision.contract.spec().description.clone(),
        revision_id,
        revision.contract.digest().clone(),
        actor,
        created_at,
    )
    .expect("semantic Workflow definition");
    let record = WorkflowDefinitionRecord {
        definition: definition.clone(),
        revision: revision.clone(),
    };
    let request_id = Uuid::now_v7();
    let write = CreateWorkflowDefinitionWrite {
        event: WorkflowRevisionPublished::created(&definition, &revision, request_id)?,
        record: record.clone(),
        actor_principal_id: actor,
        request_id,
        idempotency: IdempotencyRequest::new(
            "postgres-workflow-semantics",
            "create",
            revision.contract.canonical_acl().as_bytes(),
        )
        .expect("idempotency request"),
    };
    let repository = PostgresWorkflowDefinitionRepository::new(executor.clone());
    let created = repository.create(write.clone()).await?;
    assert!(!created.replayed);
    assert_eq!(created.value, record);

    let replayed = repository.create(write).await?;
    assert!(replayed.replayed);
    assert_eq!(replayed.value, record);
    assert_eq!(
        repository
            .find_revision(organization_id, definition_id, revision_id)
            .await?,
        Some(revision.clone())
    );
    assert_eq!(
        repository
            .list_revisions(organization_id, definition_id)
            .await?,
        vec![revision.clone()]
    );

    let persisted = database
        .fetch_one_as(sql_query::<(i64, i64, i64, i64, i64)>(
            "select (select count(*) from workflow_revisions where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and workflow_definition_id = ")
        .bind(definition_id.as_uuid())
        .append("), (select count(*) from workflow_revision_semantic_contracts where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and workflow_definition_id = ")
        .bind(definition_id.as_uuid())
        .append("), (select count(*) from audit_records where aggregate_id = ")
        .bind(definition_id.as_uuid())
        .append("), (select count(*) from outbox_events where aggregate_id = ")
        .bind(definition_id.as_uuid())
        .append("), (select count(*) from idempotency_records where scope_key = 'postgres-workflow-semantics')"))
        .await?;
    assert_eq!(persisted, (1, 4, 1, 1, 1));

    let incomplete_id = WorkflowRevisionId::new();
    let incomplete = insert_unbound_successor(&database, &revision, incomplete_id, 2)
        .await
        .expect_err("compiler schema 2 revision without semantic children must roll back");
    assert!(
        database_error_message(&incomplete)
            == Some("Workflow compiler schema 2 requires three semantic contracts and optional default/composite material"),
        "unexpected incomplete-contract error: {incomplete}"
    );

    let downgrade_id = WorkflowRevisionId::new();
    let downgrade = insert_unbound_successor(&database, &revision, downgrade_id, 1)
        .await
        .expect_err("compiler authority downgrade must roll back");
    assert!(
        database_error_message(&downgrade)
            == Some("Workflow revisions cannot downgrade compiler schema authority"),
        "unexpected downgrade error: {downgrade}"
    );

    let revision_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from workflow_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and workflow_definition_id = ")
                .bind(definition_id.as_uuid()),
        )
        .await?;
    assert_eq!(revision_count, 1);

    for mutation in [
        sql_query::<()>("update workflow_revision_semantic_contracts set digest = digest where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and workflow_definition_id = ")
            .bind(definition_id.as_uuid())
            .append(" and workflow_revision_id = ")
            .bind(revision_id.as_uuid()),
        sql_query::<()>("delete from workflow_revision_semantic_contracts where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and workflow_definition_id = ")
            .bind(definition_id.as_uuid())
            .append(" and workflow_revision_id = ")
            .bind(revision_id.as_uuid()),
    ] {
        let error = database
            .execute(mutation)
            .await
            .expect_err("semantic contract rows must be immutable");
        assert!(
            database_error_message(&error)
                == Some("Workflow immutable records cannot be changed"),
            "unexpected immutable-record error: {error}"
        );
    }
    let semantic_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from workflow_revision_semantic_contracts where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and workflow_definition_id = ")
        .bind(definition_id.as_uuid()))
        .await?;
    assert_eq!(semantic_count, 4);

    let composite_definition_id = WorkflowDefinitionId::new();
    let composite_revision_id = WorkflowRevisionId::new();
    let composite_revision = semantic_revision(
        organization_id,
        project_id,
        composite_definition_id,
        composite_revision_id,
        actor,
        created_at,
        true,
    );
    let composite_definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        composite_definition_id,
        composite_revision.contract.spec().name.clone(),
        composite_revision.contract.spec().description.clone(),
        composite_revision_id,
        composite_revision.contract.digest().clone(),
        actor,
        created_at,
    )
    .expect("composite Workflow definition");
    let composite_record = WorkflowDefinitionRecord {
        definition: composite_definition.clone(),
        revision: composite_revision.clone(),
    };
    let composite_request_id = Uuid::now_v7();
    repository
        .create(CreateWorkflowDefinitionWrite {
            event: WorkflowRevisionPublished::created(
                &composite_definition,
                &composite_revision,
                composite_request_id,
            )?,
            record: composite_record.clone(),
            actor_principal_id: actor,
            request_id: composite_request_id,
            idempotency: IdempotencyRequest::new(
                "postgres-workflow-semantics",
                "create-composite",
                composite_revision.contract.canonical_acl().as_bytes(),
            )
            .expect("composite idempotency"),
        })
        .await?;
    assert_eq!(
        repository
            .find_revision(
                organization_id,
                composite_definition_id,
                composite_revision_id,
            )
            .await?,
        Some(composite_revision.clone())
    );
    assert_eq!(
        composite_revision
            .semantic_contracts
            .as_ref()
            .expect("composite semantic contracts")
            .persisted_contracts()
            .len(),
        5
    );
    let composite_rows = database
        .fetch_one_as(sql_query::<(i64, i64)>(
            "select count(*), count(*) filter (where kind = 'composite_regions') from workflow_revision_semantic_contracts where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and workflow_definition_id = ")
        .bind(composite_definition_id.as_uuid())
        .append(" and workflow_revision_id = ")
        .bind(composite_revision_id.as_uuid()))
        .await?;
    assert_eq!(composite_rows, (5, 1));

    let ontology = a3s_cloud_control_plane::modules::workflow::OntologyRevision::initial(
        organization_id,
        project_id,
        a3s_cloud_control_plane::modules::shared_kernel::domain::OntologyId::new(),
        a3s_cloud_control_plane::modules::shared_kernel::domain::OntologyRevisionId::new(),
        a3s_cloud_control_plane::modules::workflow::OntologyContract::from_spec(
            a3s_cloud_control_plane::modules::workflow::OntologySpec {
                name: "Semantic runtime".into(),
                description: String::new(),
                object_types: vec![
                    a3s_cloud_control_plane::modules::workflow::OntologyObjectType {
                        id: "ticket".into(),
                        label: "Ticket".into(),
                        schema_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                            .expect("ontology schema digest"),
                        key_fields: vec!["ticketId".into()],
                    },
                ],
                relation_types: Vec::new(),
                rules: Vec::new(),
            },
        )
        .expect("ontology contract"),
        actor,
        created_at,
    );
    let ontology_aggregate = Ontology::create(
        organization_id,
        project_id,
        ontology.ontology_id,
        OntologyName::parse(ontology.contract.spec().name.clone()).expect("ontology name"),
        ontology.contract.spec().description.clone(),
        ontology.id,
        ontology.contract.digest().clone(),
        actor,
        created_at,
    )
    .expect("ontology aggregate");
    let ontology_request_id = Uuid::now_v7();
    PostgresOntologyRepository::new(executor.clone())
        .create(CreateOntologyWrite {
            event: OntologyRevisionPublished::created(
                &ontology_aggregate,
                &ontology,
                ontology_request_id,
            )?,
            record: OntologyRecord {
                ontology: ontology_aggregate,
                revision: ontology.clone(),
            },
            actor_principal_id: actor,
            request_id: ontology_request_id,
            idempotency: IdempotencyRequest::new(
                "postgres-workflow-semantics",
                "ontology",
                b"semantic-ontology-v2",
            )
            .expect("ontology idempotency"),
        })
        .await?;
    let goal_contract =
        a3s_cloud_control_plane::modules::workflow::WorkflowGoalContract::from_spec(
            a3s_cloud_control_plane::modules::workflow::WorkflowGoalSpec {
                name: "Semantic runtime".into(),
                workflow_definition_id: definition_id,
                workflow_revision_id: revision_id,
                workflow_digest: revision.contract.digest().clone(),
                ontology_id: ontology.ontology_id,
                ontology_revision_id: ontology.id,
                ontology_digest: ontology.contract.digest().clone(),
                environment_id: None,
                input: serde_json::json!({"ticketId": "T-42"}),
            },
        )
        .expect("goal contract");
    let compiled_goal = WorkflowPlanCompiler::compile_goal(
        a3s_cloud_control_plane::modules::shared_kernel::domain::WorkflowGoalId::new(),
        a3s_cloud_control_plane::modules::shared_kernel::domain::PlanRevisionId::new(),
        goal_contract,
        &definition,
        &revision,
        &ontology,
        actor,
        created_at,
    )
    .expect("compiled semantic goal");
    let compiled_run = WorkflowRunCompiler::compile(
        a3s_cloud_control_plane::modules::shared_kernel::domain::WorkflowRunId::new(),
        &compiled_goal.goal,
        &compiled_goal.plan_revision,
        &revision,
        Some(60),
        actor,
        created_at,
    )
    .expect("compiled semantic run");
    let input_json = String::from_utf8(
        compiled_run
            .run
            .execution_input
            .canonical_bytes()
            .expect("canonical semantic run input"),
    )?;
    assert_eq!(
        compiled_run.run.execution_input.flow_workflow_version,
        WORKFLOW_RUN_FLOW_VERSION_V2
    );
    assert!(compiled_run.run.execution_input.variable_defaults.is_some());
    let goal_request_id = Uuid::now_v7();
    PostgresWorkflowGoalRepository::new(executor.clone())
        .create(CreateWorkflowGoalWrite {
            event: WorkflowGoalCompiled::envelope(
                &compiled_goal.goal,
                &compiled_goal.plan_revision,
                goal_request_id,
            )?,
            record: WorkflowGoalRecord {
                goal: compiled_goal.goal,
                plan_revision: compiled_goal.plan_revision,
            },
            actor_principal_id: actor,
            request_id: goal_request_id,
            idempotency: IdempotencyRequest::new(
                "postgres-workflow-semantics",
                "goal",
                b"semantic-goal-v2",
            )
            .expect("goal idempotency"),
        })
        .await?;
    let run_record = WorkflowRunRecord {
        run: compiled_run.run,
        steps: compiled_run.steps,
    };
    let run_request_id = Uuid::now_v7();
    let run_repository = PostgresWorkflowRunRepository::new(executor.clone());
    let created_run = run_repository
        .create(CreateWorkflowRunWrite {
            event: WorkflowRunRequested::envelope(&run_record.run, run_request_id)?,
            record: run_record.clone(),
            actor_principal_id: actor,
            request_id: run_request_id,
            idempotency: IdempotencyRequest::new(
                "postgres-workflow-semantics",
                "run",
                b"semantic-run-v2",
            )
            .expect("run idempotency"),
        })
        .await?;
    assert_eq!(created_run.value, run_record);
    assert_eq!(
        run_repository
            .find(organization_id, run_record.run.id)
            .await?,
        Some(run_record.clone())
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<String>(
                    "select execution_input from workflow_runs where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(run_record.run.id.as_uuid()),
            )
            .await?,
        input_json
    );
    let operation_identity = database
        .fetch_one_as(
            sql_query::<(String, String)>(
                "select workflow_name, workflow_version from operation_requests where operation_id = ",
            )
            .bind(run_record.run.operation_id.as_uuid()),
        )
        .await?;
    assert_eq!(
        operation_identity,
        ("cloud.workflow-run".into(), "2".into())
    );

    let flow_run_id = run_record.run.flow_run_id.clone();
    let flow_spec = FlowWorkflowSpec::rust_embedded(
        run_record.run.execution_input.flow_workflow_name.clone(),
        run_record.run.execution_input.flow_workflow_version.clone(),
        "a3s-cloud",
        "main",
    );
    let flow_input = serde_json::to_value(&run_record.run.execution_input)?;
    let flow =
        FlowInfrastructure::connect(&url, Arc::new(WorkflowRunFlowRuntime::default())).await?;
    flow.engine()
        .start_with_id(&flow_run_id, flow_spec.clone(), flow_input.clone())
        .await?;
    let completed = flow.engine().snapshot(&flow_run_id).await?;
    assert_eq!(completed.status, FlowWorkflowRunStatus::Completed);
    assert_eq!(
        completed.output,
        Some(serde_json::json!({
            "result": run_record.run.execution_input.goal_input,
        }))
    );
    let durable_history = flow.engine().history(&flow_run_id).await?;
    let inspection = WorkflowRunVariableReader::new(flow.engine())
        .inspect(&run_record)
        .await
        .expect("Flow-backed variable inspection");
    assert_eq!(inspection.variables.len(), 2);
    assert_eq!(
        inspection
            .variables
            .iter()
            .find(|variable| variable.name == "request")
            .and_then(|variable| variable.value.as_ref()),
        Some(&run_record.run.execution_input.goal_input)
    );
    assert_eq!(
        inspection
            .variables
            .iter()
            .find(|variable| variable.name == "fallback")
            .and_then(|variable| variable.value.as_ref()),
        Some(&serde_json::json!({"priority": "normal"}))
    );
    drop(flow);

    let restarted =
        FlowInfrastructure::connect(&url, Arc::new(WorkflowRunFlowRuntime::default())).await?;
    assert_eq!(
        restarted.engine().history(&flow_run_id).await?,
        durable_history
    );
    assert_eq!(restarted.engine().snapshot(&flow_run_id).await?, completed);
    assert_eq!(
        WorkflowRunVariableReader::new(restarted.engine())
            .inspect(&run_record)
            .await
            .expect("restarted Flow-backed variable inspection"),
        inspection
    );
    restarted
        .engine()
        .start_with_id(&flow_run_id, flow_spec, flow_input)
        .await?;
    assert_eq!(
        restarted.engine().history(&flow_run_id).await?,
        durable_history
    );
    Ok(())
}

fn database_error_message(error: &DatabaseError<PostgresError>) -> Option<&str> {
    let DatabaseError::Execute(PostgresError::Database(error)) = error else {
        return None;
    };
    error.as_db_error().map(|error| error.message())
}

async fn insert_unbound_successor(
    database: &Database<PostgresDialect, PostgresExecutor>,
    parent: &WorkflowRevision,
    revision_id: WorkflowRevisionId,
    compiler_schema_version: u32,
) -> Result<ExecuteResult, DatabaseError<PostgresError>> {
    database
        .execute(
            sql_query::<()>("insert into workflow_revisions (organization_id, project_id, workflow_definition_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, payload_set_digest, created_by, created_at) values (")
                .bind(parent.organization_id.as_uuid())
                .append(", ")
                .bind(parent.project_id.as_uuid())
                .append(", ")
                .bind(parent.workflow_definition_id.as_uuid())
                .append(", ")
                .bind(revision_id.as_uuid())
                .append(", 2, ")
                .bind(parent.id.as_uuid())
                .append(", ")
                .bind(parent.contract.digest().as_str())
                .append(", ")
                .bind(parent.contract_schema())
                .append(", ")
                .bind(compiler_schema_version)
                .append(", ")
                .bind(parent.contract.canonical_acl())
                .append(", ")
                .bind(parent.contract.digest().as_str())
                .append(", ")
                .bind(parent.payload_set_digest.as_str())
                .append(", ")
                .bind(parent.created_by.as_uuid())
                .append(", ")
                .bind(parent.created_at)
                .append(")"),
        )
        .await
}

fn semantic_revision(
    organization_id: OrganizationId,
    project_id: ProjectId,
    definition_id: WorkflowDefinitionId,
    revision_id: WorkflowRevisionId,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
    include_composite: bool,
) -> WorkflowRevision {
    let schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Object,
            fields: Vec::new(),
        }))
        .expect("Workflow data schema");
    let input_configuration = WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
        WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
    ))
    .expect("input configuration");
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))
        .expect("output configuration");
    let composite_configuration = include_composite.then(|| {
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Subworkflow),
        ))
        .expect("composite configuration")
    });
    let output_step = workflow_step(
        "output",
        WorkflowStepKind::Output,
        output_configuration.digest().clone(),
        schema.digest().clone(),
    );
    let mut steps = vec![workflow_step(
        "input",
        WorkflowStepKind::Input,
        input_configuration.digest().clone(),
        schema.digest().clone(),
    )];
    if let Some(configuration) = composite_configuration.as_ref() {
        let mut iteration = workflow_step(
            "iteration",
            WorkflowStepKind::Subworkflow,
            configuration.digest().clone(),
            schema.digest().clone(),
        );
        iteration.capability = Some(CapabilityReference {
            owner: CapabilityOwner::Workflow,
            capability_type: CapabilityType::WorkflowRevision,
            resource_id: WorkflowDefinitionId::new().as_uuid(),
            revision: WorkflowRevisionId::new().to_string(),
            digest: Sha256Digest::parse(format!("sha256:{}", "9".repeat(64)))
                .expect("child Workflow digest"),
            capability: "workflow.run".into(),
        });
        steps.push(iteration);
    }
    steps.push(output_step);
    let edges = if include_composite {
        vec![
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
        ]
    } else {
        vec![WorkflowEdgeSpec {
            id: "input-output".into(),
            source: "input".into(),
            target: "output".into(),
            source_handle: None,
        }]
    };
    let workflow = WorkflowSpec {
        name: if include_composite {
            "Composite semantic persistence".into()
        } else {
            "Semantic persistence".into()
        },
        description: "Revision-owned compiler contracts".into(),
        steps,
        edges,
    };
    let registry = descriptor_registry(
        input_configuration.digest().clone(),
        composite_configuration
            .as_ref()
            .map(|configuration| configuration.digest().clone()),
    );
    let binding_pairs = if include_composite {
        vec![
            ("input", "workflow.input"),
            ("iteration", "workflow.iteration"),
            ("output", "workflow.output"),
        ]
    } else {
        vec![("input", "workflow.input"), ("output", "workflow.output")]
    };
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "integration.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: binding_pairs
            .into_iter()
            .map(|(step_id, descriptor_id)| WorkflowStepDescriptorBinding {
                step_id: step_id.into(),
                descriptor_id: descriptor_id.into(),
                descriptor_revision: "1.0.0".into(),
                semantic_digest: registry
                    .resolve(descriptor_id, "1.0.0")
                    .expect("descriptor")
                    .semantic_digest()
                    .clone(),
            })
            .collect(),
    })
    .expect("descriptor bindings");
    let default =
        WorkflowVariableDefault::new("fallback", serde_json::json!({"priority": "normal"}))
            .expect("variable default");
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "integration.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![
            WorkflowVariableDeclaration {
                name: "request".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::Object,
                value_schema_digest: schema.digest().clone(),
                source_schema_digest: Some(schema.digest().clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "fallback".into(),
                scope: WorkflowVariableScope::Run,
                value_type: WorkflowDataType::Object,
                value_schema_digest: schema.digest().clone(),
                source_schema_digest: None,
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Deterministic,
                required: false,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: Some(default.digest.clone()),
            },
        ],
        reads: vec![WorkflowVariableRead {
            id: "output-request".into(),
            variable: "request".into(),
            consumer_step_id: "output".into(),
            consumer_region_id: None,
            target_port: "result".into(),
            path: Vec::new(),
            expected_type: WorkflowDataType::Object,
            expected_schema_digest: schema.digest().clone(),
            required: true,
            mode: WorkflowVariableReadMode::DirectValue,
        }],
        assignments: Vec::new(),
        exports: Vec::new(),
    })
    .expect("variable contract");
    let defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: variables.id().into(),
        revision: variables.revision().into(),
        values: vec![default],
    })
    .expect("variable defaults");
    let composite_regions = include_composite.then(|| {
        WorkflowCompositeRegions::from_spec(WorkflowCompositeRegionsSpec {
            id: "integration.workflow".into(),
            revision: "1.0.0".into(),
            compiler_schema_version: 2,
            regions: vec![WorkflowCompositeRegionPolicy::Iteration(
                WorkflowIterationRegionPolicy {
                    step_id: "iteration".into(),
                    maximum_items: 1_000,
                    maximum_concurrency: 10,
                    failure_mode: WorkflowIterationFailureMode::ContinueNull,
                },
            )],
        })
        .expect("composite regions")
    });
    let semantics = WorkflowRevisionSemanticContracts::create_with_optional_contracts(
        &workflow,
        bindings,
        registry,
        variables,
        Some(defaults),
        composite_regions,
    )
    .expect("semantic contracts");
    WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        WorkflowContract::from_spec(workflow).expect("Workflow contract"),
        {
            let mut payloads = vec![schema, input_configuration, output_configuration];
            if let Some(configuration) = composite_configuration {
                payloads.push(configuration);
            }
            payloads
        },
        semantics,
        actor,
        created_at,
    )
    .expect("semantic Workflow revision")
}
