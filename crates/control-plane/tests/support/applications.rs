use super::*;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_control_plane::modules::applications::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationResponseMode, CreateApplication, CreateApplicationHandler, IApplicationRepository,
    IApplicationWorkflowRevisionPort, PostgresApplicationRepository, PublishApplicationRelease,
    PublishApplicationReleaseHandler, WorkflowApplicationReleaseEvidenceReader,
};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use a3s_cloud_control_plane::modules::workflow::{
    CreateWorkflowDefinitionWrite, IWorkflowDefinitionRepository,
    PostgresWorkflowDefinitionRepository, WorkflowDefinition, WorkflowDefinitionRecord,
    WorkflowRevisionPublished,
};

pub(super) async fn exercise_application_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("124"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (1, "immutable Application releases".into())
    );

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
            .append(", 'Application tenant', ")
            .bind(format!("application-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'Application publisher', 1, ")
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
                .append(", 'Application project', 'application-project', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let workflow_definition_id = WorkflowDefinitionId::new();
    let workflow_revision_id = WorkflowRevisionId::new();
    let workflow_revision = super::workflow_semantic_contracts_support::semantic_revision(
        organization_id,
        project_id,
        workflow_definition_id,
        workflow_revision_id,
        actor,
        created_at,
        false,
    );
    let workflow_definition = WorkflowDefinition::create(
        organization_id,
        project_id,
        workflow_definition_id,
        workflow_revision.contract.spec().name.clone(),
        workflow_revision.contract.spec().description.clone(),
        workflow_revision_id,
        workflow_revision.contract.digest().clone(),
        actor,
        created_at,
    )?;
    let workflow_record = WorkflowDefinitionRecord {
        definition: workflow_definition.clone(),
        revision: workflow_revision.clone(),
    };
    let workflow_request_id = Uuid::now_v7();
    let workflow_repository = Arc::new(PostgresWorkflowDefinitionRepository::new(executor.clone()));
    workflow_repository
        .create(CreateWorkflowDefinitionWrite {
            event: WorkflowRevisionPublished::created(
                &workflow_definition,
                &workflow_revision,
                workflow_request_id,
            )?,
            record: workflow_record,
            actor_principal_id: actor,
            request_id: workflow_request_id,
            idempotency: IdempotencyRequest::new(
                "postgres-application-workflow",
                "create",
                workflow_revision.contract.digest().as_str().as_bytes(),
            )?,
        })
        .await?;

    let evidence_reader = Arc::new(WorkflowApplicationReleaseEvidenceReader::new(
        workflow_repository,
    ));
    let evidence = evidence_reader
        .resolve_revision(
            organization_id,
            project_id,
            workflow_definition_id,
            workflow_revision_id,
        )
        .await?;
    assert_eq!(
        evidence.binding.workflow_contract_digest,
        *workflow_revision.contract.digest()
    );
    assert_eq!(
        evidence.binding.workflow_payload_set_digest,
        workflow_revision.payload_set_digest
    );
    assert_eq!(
        &evidence.binding.workflow_semantic_contract_set_digest,
        workflow_revision
            .semantic_contract_set_digest()
            .expect("semantic contract set")
    );

    let applications = Arc::new(PostgresApplicationRepository::new(executor.clone()));
    let create_handler =
        CreateApplicationHandler::new(applications.clone(), evidence_reader.clone());
    let initial_contract = contract(&evidence, '1');
    let create = CreateApplication {
        organization_id,
        project_id,
        name: "Support copilot".into(),
        description: "Exact Workflow-backed application".into(),
        release_acl: initial_contract.canonical_acl().into(),
        actor_principal_id: actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "postgres-application-create".into(),
        request_id: Uuid::now_v7(),
    };
    let created = create_handler
        .execute(create.clone(), context())
        .await?
        .map_err(|error| format!("create Application: {error}"))?;
    assert!(!created.replayed);
    let create_replay = create_handler
        .execute(create.clone(), context())
        .await?
        .map_err(|error| format!("replay Application create: {error}"))?;
    assert!(create_replay.replayed);
    assert_eq!(create_replay.record, created.record);

    let conflicting = create_handler
        .execute(
            CreateApplication {
                release_acl: contract(&evidence, '2').canonical_acl().into(),
                ..create
            },
            context(),
        )
        .await?;
    assert!(matches!(
        conflicting,
        Err(a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError::Conflict(_))
    ));

    let publish_handler =
        PublishApplicationReleaseHandler::new(applications.clone(), evidence_reader);
    let second_contract = contract(&evidence, '2');
    let publish = PublishApplicationRelease {
        organization_id,
        project_id,
        application_id: created.record.application.id,
        expected_version: 1,
        release_acl: second_contract.canonical_acl().into(),
        actor_principal_id: actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "postgres-application-publish-2".into(),
        request_id: Uuid::now_v7(),
    };
    let published = publish_handler
        .execute(publish.clone(), context())
        .await?
        .map_err(|error| format!("publish Application release: {error}"))?;
    assert!(!published.replayed);
    let publish_replay = publish_handler
        .execute(publish, context())
        .await?
        .map_err(|error| format!("replay Application publication: {error}"))?;
    assert!(publish_replay.replayed);
    assert_eq!(publish_replay.record, published.record);

    assert_rejected(
        database
            .execute(
                sql_query::<()>("update application_releases set canonical_acl = canonical_acl where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(created.record.application.id.as_uuid()),
            )
            .await,
        "mutating immutable Application releases",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("delete from application_releases where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(created.record.application.id.as_uuid())
                    .append(" and id = ")
                    .bind(created.record.release.id.as_uuid()),
            )
            .await,
        "deleting an immutable Application release",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("update applications set aggregate_version = aggregate_version + 2, current_release_number = current_release_number + 2 where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(created.record.application.id.as_uuid()),
            )
            .await,
        "skipping an Application release generation",
    );

    let exact_binding_count = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from application_releases release join workflow_revisions revision on revision.organization_id = release.organization_id and revision.project_id = release.project_id and revision.workflow_definition_id = release.workflow_definition_id and revision.id = release.workflow_revision_id where release.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and release.application_id = ")
                .bind(created.record.application.id.as_uuid())
                .append(" and revision.content_digest = release.workflow_contract_digest and revision.payload_set_digest = release.workflow_payload_set_digest"),
        )
        .await?;
    assert_eq!(exact_binding_count, 2);

    let evidence_counts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>("select (select count(*) from applications where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from application_releases where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key in ('application.release.created', 'application.release.published')), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and action in ('application.release.created', 'application.release.published')), (select count(*) from idempotency_records where scope_key like ")
                .bind(format!("organizations/{organization_id}/projects/{project_id}/applications%"))
                .append(")"),
        )
        .await?;
    assert_eq!(evidence_counts, (1, 2, 2, 2, 2));

    let restarted_executor = connect_postgres(&url, 4).await?;
    let restarted = PostgresApplicationRepository::new(restarted_executor);
    assert_eq!(
        restarted
            .find(organization_id, project_id, created.record.application.id,)
            .await?,
        Some(published.record.application.clone())
    );
    assert!(restarted
        .find(
            organization_id,
            ProjectId::new(),
            created.record.application.id,
        )
        .await?
        .is_none());
    assert!(restarted
        .find(
            OrganizationId::new(),
            project_id,
            created.record.application.id,
        )
        .await?
        .is_none());
    assert_eq!(
        restarted
            .list_releases(
                organization_id,
                project_id,
                created.record.application.id,
                50,
            )
            .await?,
        vec![published.record.release, created.record.release]
    );
    Ok(())
}

fn contract(
    evidence: &a3s_cloud_control_plane::modules::applications::ApplicationWorkflowRevisionEvidence,
    presentation: char,
) -> ApplicationReleaseContract {
    ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![
                ApplicationResponseMode::Blocking,
                ApplicationResponseMode::Streaming,
            ],
        },
        workflow: evidence.binding.clone(),
        presentation_digest: digest(presentation),
    })
    .expect("Application contract")
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database accepted {label}");
}
