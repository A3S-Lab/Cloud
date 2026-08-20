use super::*;
use a3s_cloud_control_plane::modules::applications::{
    Application, ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationRecord, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationReleasePublished, ApplicationResponseMode,
    ApplicationWorkflowBinding, CreateApplicationWrite, IApplicationRepository,
    PostgresApplicationRepository, PublishApplicationReleaseWrite,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, PrincipalId, RepositoryError, Sha256Digest,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_orm::{DatabaseError, Executor, PostgresError, PostgresTransaction, Query};
use chrono::Duration;

pub(super) async fn exercise_application_release_persistence(
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
    let workflow_definition_id = WorkflowDefinitionId::new();
    let workflow_revision_id = WorkflowRevisionId::new();
    let workflow_contract_digest = digest('a');
    let workflow_payload_set_digest = digest('b');
    let created_at = Utc::now();
    seed_scope(&database, organization_id, project_id, actor, created_at).await?;
    seed_workflow_revision(
        &executor,
        organization_id,
        project_id,
        actor,
        workflow_definition_id,
        workflow_revision_id,
        &workflow_contract_digest,
        &workflow_payload_set_digest,
        created_at,
    )
    .await?;

    let repository = PostgresApplicationRepository::new(executor.clone());
    let application_id = ApplicationId::new();
    let initial = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        release_contract(
            workflow_definition_id,
            workflow_revision_id,
            workflow_contract_digest.clone(),
            workflow_payload_set_digest.clone(),
            'f',
        )?,
        actor,
        created_at + Duration::seconds(1),
    )?;
    let initial_application = Application::create(
        application_id,
        ResourceName::parse("Support assistant")?,
        "One immutable release bound to one exact Workflow revision".into(),
        &initial,
    )?;
    let initial_record = ApplicationRecord::new(initial_application.clone(), initial.clone())?;
    let create_request_id = Uuid::now_v7();
    let create_idempotency = IdempotencyRequest::new(
        format!("organizations/{organization_id}/projects/{project_id}/applications"),
        "application-create",
        initial.contract.canonical_acl().as_bytes(),
    )?;
    let create = CreateApplicationWrite {
        event: ApplicationReleasePublished::published(
            &initial_application,
            &initial,
            create_request_id,
        )?,
        actor_principal_id: actor,
        request_id: create_request_id,
        idempotency: create_idempotency.clone(),
        record: initial_record.clone(),
    };
    assert!(!repository.create(create.clone()).await?.replayed);
    let replay = repository.create(create).await?;
    assert!(replay.replayed);
    assert_eq!(replay.value, initial_record);
    assert_eq!(
        repository.replay_write(&create_idempotency).await?,
        Some(initial_record.clone())
    );

    let conflicting_idempotency = IdempotencyRequest::new(
        create_idempotency.scope.clone(),
        create_idempotency.key.clone(),
        b"different Application request",
    )?;
    let conflicting_request_id = Uuid::now_v7();
    assert_eq!(
        repository
            .create(CreateApplicationWrite {
                event: ApplicationReleasePublished::published(
                    &initial_application,
                    &initial,
                    conflicting_request_id,
                )?,
                actor_principal_id: actor,
                request_id: conflicting_request_id,
                idempotency: conflicting_idempotency,
                record: initial_record.clone(),
            })
            .await,
        Err(RepositoryError::IdempotencyConflict)
    );

    let successor = ApplicationRelease::successor(
        &initial,
        ApplicationReleaseId::new(),
        release_contract(
            workflow_definition_id,
            workflow_revision_id,
            workflow_contract_digest.clone(),
            workflow_payload_set_digest.clone(),
            '9',
        )?,
        actor,
        created_at + Duration::seconds(2),
    )?;
    let current_application = initial_application.advance(1, &successor)?;
    let current_record = ApplicationRecord::new(current_application.clone(), successor.clone())?;
    let publish_request_id = Uuid::now_v7();
    let publish_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/projects/{project_id}/applications/{application_id}/releases"
        ),
        "application-publish",
        successor.contract.canonical_acl().as_bytes(),
    )?;
    let publish = PublishApplicationReleaseWrite {
        event: ApplicationReleasePublished::published(
            &current_application,
            &successor,
            publish_request_id,
        )?,
        actor_principal_id: actor,
        request_id: publish_request_id,
        expected_version: 1,
        idempotency: publish_idempotency,
        record: current_record.clone(),
    };
    assert!(!repository.publish_release(publish.clone()).await?.replayed);
    let published_replay = repository.publish_release(publish).await?;
    assert!(published_replay.replayed);
    assert_eq!(published_replay.value, current_record);
    assert_eq!(
        repository.replay_write(&create_idempotency).await?,
        Some(initial_record.clone())
    );

    let stale = ApplicationRelease::successor(
        &initial,
        ApplicationReleaseId::new(),
        release_contract(
            workflow_definition_id,
            workflow_revision_id,
            workflow_contract_digest.clone(),
            workflow_payload_set_digest.clone(),
            '8',
        )?,
        actor,
        created_at + Duration::seconds(3),
    )?;
    let stale_application = initial_application.advance(1, &stale)?;
    let stale_record = ApplicationRecord::new(stale_application.clone(), stale.clone())?;
    let stale_request_id = Uuid::now_v7();
    assert!(matches!(
        repository
            .publish_release(PublishApplicationReleaseWrite {
                event: ApplicationReleasePublished::published(
                    &stale_application,
                    &stale,
                    stale_request_id,
                )?,
                actor_principal_id: actor,
                request_id: stale_request_id,
                expected_version: 1,
                idempotency: IdempotencyRequest::new(
                    "application-stale",
                    "application-stale",
                    stale.contract.canonical_acl().as_bytes(),
                )?,
                record: stale_record,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(
        repository
            .find(organization_id, project_id, application_id)
            .await?,
        Some(current_application.clone())
    );
    assert_eq!(
        repository.list(organization_id, project_id, 50).await?,
        vec![current_application.clone()]
    );
    assert!(repository
        .list(organization_id, ProjectId::new(), 50)
        .await?
        .is_empty());
    assert!(repository
        .list(organization_id, project_id, 0)
        .await?
        .is_empty());
    assert!(repository
        .find(organization_id, ProjectId::new(), application_id)
        .await?
        .is_none());
    assert_eq!(
        repository
            .list_releases(organization_id, project_id, application_id, 50)
            .await?,
        vec![successor.clone(), initial.clone()]
    );
    assert_eq!(
        PostgresApplicationRepository::new(executor.clone())
            .find_release(organization_id, project_id, application_id, successor.id,)
            .await?,
        Some(successor.clone())
    );
    assert!(repository
        .find_release(
            organization_id,
            ProjectId::new(),
            application_id,
            successor.id,
        )
        .await?
        .is_none());
    assert!(repository
        .list_releases(organization_id, ProjectId::new(), application_id, 50)
        .await?
        .is_empty());

    let mismatched_application_id = ApplicationId::new();
    let mismatched_release = ApplicationRelease::initial(
        organization_id,
        project_id,
        mismatched_application_id,
        ApplicationReleaseId::new(),
        release_contract(
            workflow_definition_id,
            workflow_revision_id,
            digest('7'),
            workflow_payload_set_digest.clone(),
            '7',
        )?,
        actor,
        created_at + Duration::seconds(4),
    )?;
    let mismatched_application = Application::create(
        mismatched_application_id,
        ResourceName::parse("Mismatched Workflow")?,
        String::new(),
        &mismatched_release,
    )?;
    let mismatched_record =
        ApplicationRecord::new(mismatched_application.clone(), mismatched_release.clone())?;
    let mismatched_request_id = Uuid::now_v7();
    assert!(repository
        .create(CreateApplicationWrite {
            event: ApplicationReleasePublished::published(
                &mismatched_application,
                &mismatched_release,
                mismatched_request_id,
            )?,
            actor_principal_id: actor,
            request_id: mismatched_request_id,
            idempotency: IdempotencyRequest::new(
                "application-mismatched-workflow",
                "application-mismatched-workflow",
                mismatched_release.contract.canonical_acl().as_bytes(),
            )?,
            record: mismatched_record,
        })
        .await
        .is_err());
    assert!(repository
        .find(organization_id, project_id, mismatched_application_id)
        .await?
        .is_none());

    let persisted_facts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>(
                "select (select count(*) from applications where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from application_releases where organization_id = ")
            .bind(organization_id.as_uuid())
            .append("), (select count(*) from outbox_events where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and event_key = 'application.release.published'), (select count(*) from audit_records where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and action = 'application.release.published')"),
        )
        .await?;
    assert_eq!(persisted_facts, (1, 2, 2, 2));

    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update application_releases set canonical_acl = 'tampered' where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and application_id = ")
                .bind(application_id.as_uuid())
                .append(" and id = ")
                .bind(initial.id.as_uuid()),
            )
            .await,
        "mutating an immutable Application release",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("delete from application_releases where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and id = ")
                    .bind(initial.id.as_uuid()),
            )
            .await,
        "deleting an immutable Application release",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "update applications set aggregate_version = aggregate_version + 2, current_release_number = current_release_number + 2 where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(application_id.as_uuid()),
            )
            .await,
        "skipping an Application release",
    );
    Ok(())
}

async fn seed_scope(
    database: &Database<PostgresDialect, a3s_orm::PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
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
                .append(", 'human', 'Application owner', 1, ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", 'Application project', 'application-project', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn seed_workflow_revision(
    executor: &a3s_orm::PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    workflow_definition_id: WorkflowDefinitionId,
    workflow_revision_id: WorkflowRevisionId,
    contract_digest: &Sha256Digest,
    payload_set_digest: &Sha256Digest,
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let contract_digest = contract_digest.clone();
    let payload_set_digest = payload_set_digest.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let database = SeedTransaction::new(transaction);
                database
                    .execute(
                        sql_query::<()>("insert into workflow_definitions (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(project_id.as_uuid())
                            .append(", ")
                            .bind(workflow_definition_id.as_uuid())
                            .append(", 'Application Workflow', 'application-workflow', '', ")
                            .bind(workflow_revision_id.as_uuid())
                            .append(", 1, ")
                            .bind(contract_digest.as_str())
                            .append(", 1, ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_revisions (organization_id, project_id, workflow_definition_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, payload_set_digest, created_by, created_at) values (")
                            .bind(organization_id.as_uuid())
                            .append(", ")
                            .bind(project_id.as_uuid())
                            .append(", ")
                            .bind(workflow_definition_id.as_uuid())
                            .append(", ")
                            .bind(workflow_revision_id.as_uuid())
                            .append(", 1, null, null, 'cloud.workflow.definition.v1', 1, 'workflow \"application_fixture\" {}', ")
                            .bind(contract_digest.as_str())
                            .append(", ")
                            .bind(payload_set_digest.as_str())
                            .append(", ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                Ok::<(), DatabaseError<PostgresError>>(())
            })
        })
        .await?;
    Ok(())
}

fn release_contract(
    workflow_definition_id: WorkflowDefinitionId,
    workflow_revision_id: WorkflowRevisionId,
    workflow_contract_digest: Sha256Digest,
    workflow_payload_set_digest: Sha256Digest,
    presentation_marker: char,
) -> Result<ApplicationReleaseContract, String> {
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
        workflow: ApplicationWorkflowBinding {
            workflow_definition_id,
            workflow_revision_id,
            workflow_contract_digest,
            workflow_payload_set_digest,
            workflow_semantic_contract_set_digest: digest('c'),
            input_schema_digest: digest('d'),
            output_schema_digest: digest('e'),
        },
        presentation_digest: digest(presentation_marker),
    })
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
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

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database must reject {label}");
}
