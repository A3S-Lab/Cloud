use a3s_cloud_control_plane::infrastructure::connect_and_migrate;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use a3s_cloud_control_plane::modules::workflow::domain::{
    WorkflowRevisionSemanticContracts, WorkflowStepDescriptorBinding,
    WorkflowStepDescriptorBindings, WorkflowStepDescriptorBindingsSpec,
};
use a3s_cloud_control_plane::modules::workflow::{
    CreateWorkflowDefinitionWrite, IWorkflowDefinitionRepository,
    PostgresWorkflowDefinitionRepository, WorkflowContract, WorkflowDataSchema, WorkflowDataType,
    WorkflowDefinition, WorkflowDefinitionRecord, WorkflowEdgeSpec, WorkflowPayload,
    WorkflowPayloadContent, WorkflowRevision, WorkflowRevisionPublished, WorkflowSpec,
    WorkflowStepConfiguration, WorkflowStepDescriptorAdmission, WorkflowStepDescriptorRegistry,
    WorkflowStepDescriptorRegistrySpec, WorkflowStepDescriptorSpec, WorkflowStepExecutionClass,
    WorkflowStepFailureContract, WorkflowStepFallbackMode, WorkflowStepKind, WorkflowStepOwner,
    WorkflowStepPort, WorkflowStepPortCardinality, WorkflowStepPresentationSpec,
    WorkflowStepRetryClassification, WorkflowStepSpec, WorkflowVariableContract,
    WorkflowVariableContractSpec, WorkflowVariableDeclaration, WorkflowVariableMutationMode,
    WorkflowVariableRead, WorkflowVariableReadMode, WorkflowVariableScope,
    WorkflowVariableStorageClass,
};
use a3s_orm::{
    sql_query, Database, DatabaseError, ExecuteResult, PostgresDialect, PostgresError,
    PostgresExecutor,
};
use chrono::Utc;
use uuid::Uuid;

pub(super) async fn exercise_workflow_semantic_contract_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(sql_query::<(i64, String)>(
            "select count(*), max(version) from a3s_orm_migrations",
        ))
        .await?;
    assert_eq!(migration_state, (103, "103".into()));

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
    let repository = PostgresWorkflowDefinitionRepository::new(executor);
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
    assert_eq!(persisted, (1, 3, 1, 1, 1));

    let incomplete_id = WorkflowRevisionId::new();
    let incomplete = insert_unbound_successor(&database, &revision, incomplete_id, 2)
        .await
        .expect_err("compiler schema 2 revision without semantic children must roll back");
    assert!(
        database_error_message(&incomplete)
            == Some("Workflow compiler schema 2 requires three semantic contracts"),
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
    assert_eq!(semantic_count, 3);
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
    let workflow = WorkflowSpec {
        name: "Semantic persistence".into(),
        description: "Revision-owned compiler contracts".into(),
        steps: vec![
            workflow_step(
                "input",
                WorkflowStepKind::Input,
                input_configuration.digest().clone(),
                schema.digest().clone(),
            ),
            workflow_step(
                "output",
                WorkflowStepKind::Output,
                output_configuration.digest().clone(),
                schema.digest().clone(),
            ),
        ],
        edges: vec![WorkflowEdgeSpec {
            id: "input-output".into(),
            source: "input".into(),
            target: "output".into(),
            source_handle: None,
        }],
    };
    let registry = descriptor_registry(input_configuration.digest().clone());
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "integration.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: [("input", "workflow.input"), ("output", "workflow.output")]
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
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "integration.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![WorkflowVariableDeclaration {
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
        }],
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
    let semantics =
        WorkflowRevisionSemanticContracts::create(&workflow, bindings, registry, variables)
            .expect("semantic contracts");
    WorkflowRevision::initial_with_semantic_contracts(
        organization_id,
        project_id,
        definition_id,
        revision_id,
        WorkflowContract::from_spec(workflow).expect("Workflow contract"),
        vec![schema, input_configuration, output_configuration],
        semantics,
        actor,
        created_at,
    )
    .expect("semantic Workflow revision")
}

fn workflow_step(
    id: &str,
    kind: WorkflowStepKind,
    configuration_digest: Sha256Digest,
    schema_digest: Sha256Digest,
) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest,
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest,
        policy_digest: None,
        capability: None,
    }
}

fn descriptor_registry(
    configuration_schema_digest: Sha256Digest,
) -> WorkflowStepDescriptorRegistry {
    WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "integration.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: vec![
            descriptor(
                "workflow.input",
                WorkflowStepKind::Input,
                "invocation",
                "value",
                configuration_schema_digest.clone(),
            ),
            descriptor(
                "workflow.output",
                WorkflowStepKind::Output,
                "result",
                "value",
                configuration_schema_digest,
            ),
        ],
    })
    .expect("descriptor registry")
}

fn descriptor(
    id: &str,
    kind: WorkflowStepKind,
    input_port: &str,
    output_port: &str,
    configuration_schema_digest: Sha256Digest,
) -> WorkflowStepDescriptorSpec {
    WorkflowStepDescriptorSpec {
        id: id.into(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(kind),
        semantic_profile: id.into(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports: vec![port(input_port)],
        output_ports: vec![port(output_port)],
        configuration_schema_digest,
        default_policy_digest: None,
        required_bindings: Vec::new(),
        allowed_capability_types: Vec::new(),
        failure: WorkflowStepFailureContract {
            error_output: None,
            retry_classification: WorkflowStepRetryClassification::NotRetryable,
            fallback: WorkflowStepFallbackMode::Unsupported,
            failure_branch: false,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 2,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: id.into(),
            summary: format!("{id} descriptor"),
            icon_key: id.into(),
        },
    }
}

fn port(name: &str) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type: WorkflowDataType::Object,
        cardinality: WorkflowStepPortCardinality::Single,
        required: true,
        dynamic: false,
    }
}
