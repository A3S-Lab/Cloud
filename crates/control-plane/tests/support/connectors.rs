use super::*;
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_control_plane::modules::connectors::{
    BeginConnectorExecutionDispatch, ConnectorDefinition, ConnectorExecutionAttemptBinding,
    ConnectorExecutionAttemptCursor, ConnectorExecutionEvidence, ConnectorExecutionEvidenceCursor,
    ConnectorExecutionReceipt, ConnectorExecutionRecoveryState, ConnectorExecutionRequest,
    ConnectorExecutionReservation, ConnectorHttpAuthentication, ConnectorHttpDefinition,
    ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
    ConnectorHttpRevisionMaterializer, ConnectorHttpStatusPolicy, ConnectorProfile,
    ConnectorRecord, ConnectorRevision, ConnectorRevisionPublished, ConnectorSecretReference,
    CreateConnectorProfile, CreateConnectorProfileHandler, CreateConnectorProfileWrite,
    GetConnectorExecutionAttempt, GetConnectorExecutionAttemptHandler, GetConnectorProfile,
    GetConnectorProfileHandler, IConnectorExecutionAttemptRepository,
    IConnectorExecutionEvidenceRepository, IConnectorProfileRepository,
    ListConnectorExecutionEvidence, ListConnectorExecutionEvidenceHandler, ListConnectorRevisions,
    ListConnectorRevisionsHandler, ListUnresolvedConnectorExecutionAttempts,
    ListUnresolvedConnectorExecutionAttemptsHandler, PostgresConnectorExecutionAttemptRepository,
    PostgresConnectorExecutionEvidenceRepository, PostgresConnectorProfileRepository,
    ReserveConnectorExecutionAttempt, ReviseConnectorProfile, ReviseConnectorProfileHandler,
    ReviseConnectorProfileWrite, SettleConnectorExecutionAttempt,
};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::identity::domain::value_objects::ResourceGrantScope;
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::secrets::{
    CreateSecretWrite, EncryptedSecretValue, ISecretEncryptionService, ISecretRepository,
    PostgresSecretRepository, RevokeSecretVersion, RevokeSecretVersionHandler, Secret,
    SecretChanged, SecretEncryptionError,
};
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, ResourceName, SecretId,
};
use chrono::Duration;
use std::sync::Mutex;

pub(super) async fn exercise_connector_profile_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("109"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (1, "immutable Connector profiles and Secret bindings".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Connector tenant', ")
            .bind(format!("connector-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'Connector owner', 1, ")
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
            .append(", 'Connector project', 'connector-project', 1, ")
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
                .append(", 'Production', 'production', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let secret_repository = PostgresSecretRepository::new(executor.clone());
    let destination_secret_id = create_secret(
        &secret_repository,
        organization_id,
        project_id,
        environment_id,
        "Connector destination",
        "ciphertext-destination",
        created_at,
    )
    .await?;
    let hmac_secret_id = create_secret(
        &secret_repository,
        organization_id,
        project_id,
        environment_id,
        "Connector HMAC",
        "ciphertext-hmac",
        created_at,
    )
    .await?;

    let repository = PostgresConnectorProfileRepository::new(executor.clone());
    let profile_id = ConnectorProfileId::new();
    let initial = ConnectorRevision::initial(
        organization_id,
        project_id,
        environment_id,
        profile_id,
        ConnectorRevisionId::new(),
        definition(destination_secret_id, hmac_secret_id, 5_000)?,
        actor,
        created_at + Duration::seconds(1),
    )?;
    let initial_profile = ConnectorProfile::create(
        profile_id,
        ResourceName::parse("Incident delivery")?,
        &initial,
    )?;
    let initial_record = ConnectorRecord::new(initial_profile.clone(), initial.clone())?;
    let create_request_id = Uuid::now_v7();
    let create_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/connector-profiles"
        ),
        "connector-create",
        initial.definition.canonical_acl().as_bytes(),
    )?;
    let create = CreateConnectorProfileWrite {
        event: ConnectorRevisionPublished::created(&initial_profile, &initial, create_request_id)?,
        actor_principal_id: actor,
        request_id: create_request_id,
        idempotency: create_idempotency.clone(),
        record: initial_record.clone(),
    };
    assert!(!repository.create(create.clone()).await?.replayed);
    let replay = repository.create(create).await?;
    assert!(replay.replayed);
    assert_eq!(replay.value, initial_record);
    let conflicting_idempotency = IdempotencyRequest::new(
        create_idempotency.scope.clone(),
        create_idempotency.key.clone(),
        b"different Connector request",
    )?;
    let conflicting_request_id = Uuid::now_v7();
    assert_eq!(
        repository
            .create(CreateConnectorProfileWrite {
                event: ConnectorRevisionPublished::created(
                    &initial_profile,
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

    let successor = ConnectorRevision::successor(
        &initial,
        ConnectorRevisionId::new(),
        definition(destination_secret_id, hmac_secret_id, 7_500)?,
        actor,
        created_at + Duration::seconds(2),
    )?;
    let current_profile = initial_profile.advance(1, &successor)?;
    let current_record = ConnectorRecord::new(current_profile.clone(), successor.clone())?;
    let revise_request_id = Uuid::now_v7();
    let revise_idempotency = IdempotencyRequest::new(
        format!("organizations/{organization_id}/connector-profiles/{profile_id}/revisions"),
        "connector-revise",
        successor.definition.canonical_acl().as_bytes(),
    )?;
    let revise = ReviseConnectorProfileWrite {
        event: ConnectorRevisionPublished::revised(
            &current_profile,
            &successor,
            revise_request_id,
        )?,
        actor_principal_id: actor,
        request_id: revise_request_id,
        expected_version: 1,
        idempotency: revise_idempotency,
        record: current_record.clone(),
    };
    assert!(!repository.revise(revise.clone()).await?.replayed);
    let revised_replay = repository.revise(revise).await?;
    assert!(revised_replay.replayed);
    assert_eq!(revised_replay.value, current_record);

    let stale_revision = ConnectorRevision::successor(
        &initial,
        ConnectorRevisionId::new(),
        definition(destination_secret_id, hmac_secret_id, 9_000)?,
        actor,
        created_at + Duration::seconds(3),
    )?;
    let stale_profile = initial_profile.advance(1, &stale_revision)?;
    let stale_record = ConnectorRecord::new(stale_profile.clone(), stale_revision.clone())?;
    let stale_request_id = Uuid::now_v7();
    assert!(matches!(
        repository
            .revise(ReviseConnectorProfileWrite {
                event: ConnectorRevisionPublished::revised(
                    &stale_profile,
                    &stale_revision,
                    stale_request_id,
                )?,
                actor_principal_id: actor,
                request_id: stale_request_id,
                expected_version: 1,
                idempotency: IdempotencyRequest::new(
                    "connector-stale",
                    "connector-stale",
                    stale_revision.definition.canonical_acl().as_bytes(),
                )?,
                record: stale_record,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(
        repository
            .find(organization_id, project_id, environment_id, profile_id)
            .await?,
        Some(current_profile.clone())
    );
    assert!(repository
        .find(
            organization_id,
            project_id,
            EnvironmentId::new(),
            profile_id,
        )
        .await?
        .is_none());
    assert!(repository
        .find(
            OrganizationId::new(),
            project_id,
            environment_id,
            profile_id,
        )
        .await?
        .is_none());
    assert_eq!(
        repository
            .list_revisions(organization_id, project_id, environment_id, profile_id, 50)
            .await?,
        vec![successor.clone(), initial.clone()]
    );

    let missing_secret_profile_id = ConnectorProfileId::new();
    let missing_secret_revision = ConnectorRevision::initial(
        organization_id,
        project_id,
        environment_id,
        missing_secret_profile_id,
        ConnectorRevisionId::new(),
        destination_only_definition(SecretId::new())?,
        actor,
        created_at + Duration::seconds(4),
    )?;
    let missing_secret_profile = ConnectorProfile::create(
        missing_secret_profile_id,
        ResourceName::parse("Missing Secret")?,
        &missing_secret_revision,
    )?;
    let missing_secret_record = ConnectorRecord::new(
        missing_secret_profile.clone(),
        missing_secret_revision.clone(),
    )?;
    let missing_request_id = Uuid::now_v7();
    assert_eq!(
        repository
            .create(CreateConnectorProfileWrite {
                event: ConnectorRevisionPublished::created(
                    &missing_secret_profile,
                    &missing_secret_revision,
                    missing_request_id,
                )?,
                actor_principal_id: actor,
                request_id: missing_request_id,
                idempotency: IdempotencyRequest::new(
                    "connector-missing-secret",
                    "connector-missing-secret",
                    missing_secret_revision
                        .definition
                        .canonical_acl()
                        .as_bytes(),
                )?,
                record: missing_secret_record,
            })
            .await,
        Err(RepositoryError::NotFound)
    );

    let exact_bindings = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from connector_revision_secret_bindings where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and profile_id = ")
            .bind(profile_id.as_uuid())
            .append(" and revision_id = ")
            .bind(initial.id.as_uuid())
            .append(" and ((purpose = 'destination' and secret_id = ")
            .bind(destination_secret_id.as_uuid())
            .append(" and secret_version = 1) or (purpose = 'hmac_sha256' and secret_id = ")
            .bind(hmac_secret_id.as_uuid())
            .append(" and secret_version = 1))"),
        )
        .await?;
    assert_eq!(exact_bindings, 2);
    let plaintext_leaks = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from connector_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and (canonical_acl like '%ciphertext-destination%' or canonical_acl like '%ciphertext-hmac%')"),
        )
        .await?;
    assert_eq!(plaintext_leaks, 0);

    assert_rejected(
        database
            .execute(
                sql_query::<()>("update connector_revisions set canonical_acl = 'tampered' where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and profile_id = ")
                    .bind(profile_id.as_uuid())
                    .append(" and id = ")
                    .bind(initial.id.as_uuid()),
            )
            .await,
        "mutating an immutable Connector revision",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "delete from connector_revision_secret_bindings where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and profile_id = ")
                .bind(profile_id.as_uuid())
                .append(" and revision_id = ")
                .bind(initial.id.as_uuid())
                .append(" and purpose = 'destination'"),
            )
            .await,
        "deleting an immutable exact Secret binding",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("update connector_profiles set aggregate_version = aggregate_version + 2, current_revision_number = current_revision_number + 2 where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(profile_id.as_uuid()),
            )
            .await,
        "skipping a Connector profile revision",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into connector_revisions (organization_id, project_id, environment_id, profile_id, id, revision_number, parent_revision_id, parent_digest, definition_kind, definition_schema, canonical_acl, definition_digest, secret_binding_count, created_by, created_at) values (")
                    .bind(organization_id.as_uuid())
                    .append(", ")
                    .bind(project_id.as_uuid())
                    .append(", ")
                    .bind(environment_id.as_uuid())
                    .append(", ")
                    .bind(profile_id.as_uuid())
                    .append(", ")
                    .bind(ConnectorRevisionId::new().as_uuid())
                    .append(", 3, ")
                    .bind(successor.id.as_uuid())
                    .append(", ")
                    .bind(successor.definition.digest().as_str())
                    .append(", 'http', 'cloud.connector.http.v1', ")
                    .bind(successor.definition.canonical_acl())
                    .append(", ")
                    .bind(successor.definition.digest().as_str())
                    .append(", 2, ")
                    .bind(actor.as_uuid())
                    .append(", ")
                    .bind(created_at + Duration::seconds(5))
                    .append(")"),
            )
            .await,
        "inserting a no-op Connector revision",
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64)>("select (select count(*) from connector_profiles where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from connector_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from connector_revision_secret_bindings where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key in ('connector.profile.created', 'connector.profile.revised')), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and action in ('connector.profile.created', 'connector.profile.revised')), (select count(*) from idempotency_records where scope_key like ")
                .bind(format!("organizations/{organization_id}/%connector%"))
                .append(")"),
        )
        .await?;
    assert_eq!(evidence, (1, 2, 4, 2, 2, 2));
    Ok(())
}

pub(super) async fn exercise_connector_application_and_materialization(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("110"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (1, "race-safe active Connector Secret admission".into())
    );
    let error_migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("111"),
        )
        .await?;
    assert_eq!(
        error_migration_state,
        (1, "typed Connector Secret admission failures".into())
    );
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Connector application tenant', ")
            .bind(format!("connector-application-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'human', 'Connector application owner', 1, ")
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
            .append(", 'Connector application project', 'connector-application-project', 1, ")
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
                .append(", 'Production', 'production', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let secrets = Arc::new(PostgresSecretRepository::new(executor.clone()));
    let destination_secret_id = create_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "Connector application destination",
        "connector-application-destination",
        created_at,
    )
    .await?;
    let hmac_secret_id = create_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "Connector application HMAC",
        "connector-application-hmac",
        created_at,
    )
    .await?;
    let connectors = Arc::new(PostgresConnectorProfileRepository::new(executor.clone()));
    let projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let create_handler =
        CreateConnectorProfileHandler::new(projects, connectors.clone(), secrets.clone());
    let create = CreateConnectorProfile {
        organization_id,
        project_id,
        environment_id,
        name: "Incident delivery".into(),
        definition_acl: definition(destination_secret_id, hmac_secret_id, 5_000)?
            .canonical_acl()
            .to_owned(),
        actor_principal_id: actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "connector-application-create".into(),
        request_id: Uuid::now_v7(),
    };
    let created = create_handler
        .execute(create.clone(), connector_context())
        .await??;
    assert!(!created.replayed);
    assert!(
        create_handler
            .execute(create.clone(), connector_context())
            .await??
            .replayed
    );

    let denied_replay = create_handler
        .execute(
            CreateConnectorProfile {
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
                ..create.clone()
            },
            connector_context(),
        )
        .await?;
    assert!(matches!(denied_replay, Err(ApplicationError::NotFound(_))));

    let encryption = Arc::new(ConnectorFixtureEncryption::default());
    let materializer = ConnectorHttpRevisionMaterializer::new(secrets.clone(), encryption.clone());
    let materialized = materializer.materialize(&created.record.revision).await?;
    let debug = format!("{materialized:?}");
    assert!(!debug.contains("hooks.example.test"));
    assert!(!debug.contains("connector-token"));
    assert_eq!(
        encryption
            .ciphertexts
            .lock()
            .expect("Connector fixture encryption lock")
            .as_slice(),
        [
            "connector-application-destination",
            "connector-application-hmac"
        ]
    );

    RevokeSecretVersionHandler::new(secrets.clone())
        .execute(
            RevokeSecretVersion {
                organization_id,
                secret_id: hmac_secret_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                version: 1,
                idempotency_key: "revoke-connector-hmac".into(),
                request_id: Uuid::now_v7(),
            },
            connector_context(),
        )
        .await??;
    assert!(matches!(
        materializer.materialize(&created.record.revision).await,
        Err(ApplicationError::Forbidden(_))
    ));
    assert!(
        create_handler
            .execute(create.clone(), connector_context())
            .await??
            .replayed
    );
    let rejected_new_write = create_handler
        .execute(
            CreateConnectorProfile {
                name: "Revoked HMAC".into(),
                idempotency_key: "connector-application-revoked".into(),
                request_id: Uuid::now_v7(),
                ..create
            },
            connector_context(),
        )
        .await?;
    assert!(matches!(
        rejected_new_write,
        Err(ApplicationError::Invalid(_))
    ));

    let revise_handler = ReviseConnectorProfileHandler::new(connectors.clone(), secrets.clone());
    let revise = ReviseConnectorProfile {
        organization_id,
        project_id,
        environment_id,
        profile_id: created.record.profile.id,
        expected_version: 1,
        definition_acl: literal_definition_acl(7_500)?,
        actor_principal_id: actor,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "connector-application-revise".into(),
        request_id: Uuid::now_v7(),
    };
    let revised = revise_handler
        .execute(revise.clone(), connector_context())
        .await??;
    assert!(!revised.replayed);
    assert!(
        revise_handler
            .execute(revise, connector_context())
            .await??
            .replayed
    );
    materializer.materialize(&revised.record.revision).await?;

    let admission_error = database
        .execute(
            sql_query::<()>("insert into connector_revision_secret_bindings (organization_id, project_id, environment_id, profile_id, revision_id, purpose, secret_id, secret_version) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", ")
                .bind(revised.record.profile.id.as_uuid())
                .append(", ")
                .bind(revised.record.revision.id.as_uuid())
                .append(", 'hmac_sha256', ")
                .bind(hmac_secret_id.as_uuid())
                .append(", 1)"),
        )
        .await
        .expect_err("migration 110 must reject a revoked exact Secret binding");
    let admission_error = format!("{admission_error:?}");
    assert!(
        admission_error.contains("Connector Secret binding is not active in its exact environment"),
        "migration 110 returned an unexpected admission failure: {admission_error}"
    );

    let loaded = GetConnectorProfileHandler::new(connectors.clone())
        .execute(
            GetConnectorProfile {
                organization_id,
                project_id,
                environment_id,
                profile_id: created.record.profile.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            connector_context(),
        )
        .await??;
    assert_eq!(loaded, revised.record);
    let history = ListConnectorRevisionsHandler::new(connectors)
        .execute(
            ListConnectorRevisions {
                organization_id,
                project_id,
                environment_id,
                profile_id: created.record.profile.id,
                limit: 50,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            connector_context(),
        )
        .await??;
    assert_eq!(history.len(), 2);

    assert_eq!(
        secrets
            .find_materializable_version(
                organization_id,
                project_id,
                environment_id,
                hmac_secret_id,
                1,
            )
            .await,
        Err(RepositoryError::NotFound)
    );
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>("select (select count(*) from connector_profiles where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from connector_revisions where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key in ('connector.profile.created', 'connector.profile.revised')), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and action in ('connector.profile.created', 'connector.profile.revised'))"),
        )
        .await?;
    assert_eq!(evidence, (1, 2, 2, 2));
    Ok(())
}

pub(super) async fn exercise_connector_execution_evidence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("113"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (1, "fenced Connector execution attempts".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor = PrincipalId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Connector evidence tenant', ")
            .bind(format!("connector-evidence-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor.as_uuid())
                .append(", 'service', 'Connector evidence recorder', 1, ")
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
            .append(", 'Connector evidence project', 'connector-evidence-project', 1, ")
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
                .append(", 'Production', 'production', 1, ")
                .bind(created_at)
                .append(")"),
        )
        .await?;

    let profiles = Arc::new(PostgresConnectorProfileRepository::new(executor.clone()));
    let profile_id = ConnectorProfileId::new();
    let revision = ConnectorRevision::initial(
        organization_id,
        project_id,
        environment_id,
        profile_id,
        ConnectorRevisionId::new(),
        ConnectorDefinition::parse_acl(&literal_definition_acl(5_000)?)?,
        actor,
        created_at + Duration::seconds(1),
    )?;
    let profile = ConnectorProfile::create(
        profile_id,
        ResourceName::parse("Execution evidence")?,
        &revision,
    )?;
    let record = ConnectorRecord::new(profile, revision.clone())?;
    let request_id = Uuid::now_v7();
    profiles
        .create(CreateConnectorProfileWrite {
            event: ConnectorRevisionPublished::created(
                &record.profile,
                &record.revision,
                request_id,
            )?,
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "connector-execution-evidence-profile",
                "create",
                revision.definition.digest().as_str().as_bytes(),
            )?,
            record,
        })
        .await?;

    let evidence = Arc::new(PostgresConnectorExecutionEvidenceRepository::new(
        executor.clone(),
    ));
    let attempts = Arc::new(PostgresConnectorExecutionAttemptRepository::new(
        executor.clone(),
    ));
    let accepted_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"sensitive accepted request".to_vec(),
    )?
    .with_header("x-a3s-source", "workflow")?;
    let accepted_receipt = ConnectorExecutionReceipt::accepted(
        revision.id,
        accepted_request.attempt_id(),
        created_at + Duration::seconds(3),
        202,
        Some("application/json".into()),
        b"sensitive accepted response".to_vec(),
    )
    .map_err(|error| error.to_string())?;
    let accepted = ConnectorExecutionEvidence::accepted(
        &revision,
        &accepted_request,
        &accepted_receipt,
        created_at + Duration::seconds(2),
    )?;
    let accepted_settlement = prepare_connector_settlement(
        attempts.as_ref(),
        &revision,
        &accepted_request,
        accepted.clone(),
    )
    .await?;
    let (left, right) = tokio::join!(
        attempts.settle(accepted_settlement.clone()),
        attempts.settle(accepted_settlement.clone())
    );
    let mut replayed = [left?.replayed, right?.replayed];
    replayed.sort_unstable();
    assert_eq!(replayed, [false, true]);

    let changed = ConnectorExecutionEvidence::rejected(
        &revision,
        &accepted_request,
        Some(400),
        accepted.started_at(),
        accepted.completed_at(),
    )?;
    assert!(matches!(
        attempts
            .settle(SettleConnectorExecutionAttempt::new(
                accepted_settlement.fence,
                changed,
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let retryable_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"sensitive retryable request".to_vec(),
    )?;
    let retryable = ConnectorExecutionEvidence::retryable(
        &revision,
        &retryable_request,
        Some(503),
        Some(std::time::Duration::from_secs(7)),
        created_at + Duration::seconds(4),
        created_at + Duration::seconds(5),
    )?;
    attempts
        .settle(
            prepare_connector_settlement(
                attempts.as_ref(),
                &revision,
                &retryable_request,
                retryable.clone(),
            )
            .await?,
        )
        .await?;
    let rejected_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"sensitive rejected request".to_vec(),
    )?;
    let rejected = ConnectorExecutionEvidence::rejected(
        &revision,
        &rejected_request,
        Some(400),
        created_at + Duration::seconds(6),
        created_at + Duration::seconds(7),
    )?;
    attempts
        .settle(
            prepare_connector_settlement(
                attempts.as_ref(),
                &revision,
                &rejected_request,
                rejected.clone(),
            )
            .await?,
        )
        .await?;

    let list_handler =
        ListConnectorExecutionEvidenceHandler::new(profiles.clone(), evidence.clone());
    let list = ListConnectorExecutionEvidence {
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id: revision.id,
        after: None,
        limit: 2,
        resource_access: ResourceAccessEvaluator::organization_wide(),
    };
    let first = list_handler
        .execute(list.clone(), connector_context())
        .await??;
    assert_eq!(first.evidence, vec![rejected.clone(), retryable.clone()]);
    assert_eq!(
        first.next_cursor,
        Some(ConnectorExecutionEvidenceCursor::after(&retryable))
    );
    let second = list_handler
        .execute(
            ListConnectorExecutionEvidence {
                after: first.next_cursor,
                ..list.clone()
            },
            connector_context(),
        )
        .await??;
    assert_eq!(second.evidence, vec![accepted.clone()]);
    assert!(second.next_cursor.is_none());
    let denied = list_handler
        .execute(
            ListConnectorExecutionEvidence {
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
                ..list
            },
            connector_context(),
        )
        .await?;
    assert!(matches!(denied, Err(ApplicationError::NotFound(_))));

    let recovered = PostgresConnectorExecutionEvidenceRepository::new(executor.clone())
        .find(
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision.id,
            accepted.attempt_id(),
        )
        .await?;
    assert_eq!(recovered, Some(accepted.clone()));
    assert!(evidence
        .find(
            OrganizationId::new(),
            project_id,
            environment_id,
            profile_id,
            revision.id,
            accepted.attempt_id(),
        )
        .await?
        .is_none());

    let concurrent_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"one concurrent reservation winner".to_vec(),
    )?;
    let concurrent_reserved_at = created_at + Duration::seconds(6);
    let concurrent_binding =
        ConnectorExecutionAttemptBinding::from_exact(&revision, &concurrent_request)?;
    let concurrent_left = ReserveConnectorExecutionAttempt::new(
        concurrent_binding.clone(),
        Uuid::now_v7(),
        concurrent_reserved_at,
        concurrent_reserved_at + Duration::seconds(30),
    )?;
    let concurrent_right = ReserveConnectorExecutionAttempt::new(
        concurrent_binding,
        Uuid::now_v7(),
        concurrent_reserved_at,
        concurrent_reserved_at + Duration::seconds(30),
    )?;
    let (concurrent_left, concurrent_right) = tokio::join!(
        attempts.reserve(concurrent_left),
        attempts.reserve(concurrent_right)
    );
    let concurrent_outcomes = [concurrent_left?, concurrent_right?];
    assert_eq!(
        concurrent_outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ConnectorExecutionReservation::Acquired { .. }))
            .count(),
        1
    );
    assert_eq!(
        concurrent_outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ConnectorExecutionReservation::Busy(_)))
            .count(),
        1
    );

    let takeover_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"expired pre-dispatch takeover".to_vec(),
    )?;
    let takeover_first_at = created_at + Duration::seconds(7);
    let takeover_first_fence = match attempts
        .reserve(ReserveConnectorExecutionAttempt::new(
            ConnectorExecutionAttemptBinding::from_exact(&revision, &takeover_request)?,
            Uuid::now_v7(),
            takeover_first_at,
            takeover_first_at + Duration::seconds(1),
        )?)
        .await?
    {
        ConnectorExecutionReservation::Acquired { fence, .. } => fence,
        other => return Err(format!("unexpected initial takeover reservation: {other:?}").into()),
    };
    let takeover_second_at = takeover_first_fence.lease_expires_at();
    let takeover_second_fence = match attempts
        .reserve(ReserveConnectorExecutionAttempt::new(
            ConnectorExecutionAttemptBinding::from_exact(&revision, &takeover_request)?,
            Uuid::now_v7(),
            takeover_second_at,
            takeover_second_at + Duration::seconds(30),
        )?)
        .await?
    {
        ConnectorExecutionReservation::Acquired { fence, replayed } => {
            assert!(!replayed);
            fence
        }
        other => return Err(format!("unexpected expired reservation takeover: {other:?}").into()),
    };
    assert_eq!(takeover_second_fence.generation(), 2);
    assert!(matches!(
        attempts
            .begin_dispatch(BeginConnectorExecutionDispatch::new(
                takeover_first_fence,
                takeover_first_at + Duration::milliseconds(500),
                takeover_first_at + Duration::milliseconds(900),
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let foreign_reserved_at = created_at + Duration::seconds(8);
    assert_eq!(
        attempts
            .reserve(ReserveConnectorExecutionAttempt::new(
                ConnectorExecutionAttemptBinding::restore(
                    organization_id,
                    project_id,
                    EnvironmentId::new(),
                    profile_id,
                    revision.id,
                    Uuid::now_v7(),
                    accepted.request_digest().clone(),
                    accepted.request_body_bytes(),
                )?,
                Uuid::now_v7(),
                foreign_reserved_at,
                foreign_reserved_at + Duration::seconds(30),
            )?)
            .await,
        Err(RepositoryError::NotFound)
    );

    let reserved_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"safe pre-dispatch reservation".to_vec(),
    )?;
    let reserved_at = created_at + Duration::seconds(8);
    let reserved = ReserveConnectorExecutionAttempt::new(
        ConnectorExecutionAttemptBinding::from_exact(&revision, &reserved_request)?,
        Uuid::now_v7(),
        reserved_at,
        reserved_at + Duration::seconds(30),
    )?;
    let reserved_fence = match attempts.reserve(reserved.clone()).await? {
        ConnectorExecutionReservation::Acquired { fence, replayed } => {
            assert!(!replayed);
            fence
        }
        other => return Err(format!("unexpected reservation: {other:?}").into()),
    };
    assert!(matches!(
        attempts.reserve(reserved).await?,
        ConnectorExecutionReservation::Acquired { replayed: true, .. }
    ));

    let uncertain_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"provider outcome unknown".to_vec(),
    )?;
    let uncertain_reserved_at = created_at + Duration::seconds(9);
    let uncertain_fence = match attempts
        .reserve(ReserveConnectorExecutionAttempt::new(
            ConnectorExecutionAttemptBinding::from_exact(&revision, &uncertain_request)?,
            Uuid::now_v7(),
            uncertain_reserved_at,
            uncertain_reserved_at + Duration::seconds(30),
        )?)
        .await?
    {
        ConnectorExecutionReservation::Acquired { fence, .. } => fence,
        other => return Err(format!("unexpected uncertain reservation: {other:?}").into()),
    };
    let uncertain_started_at = created_at + Duration::seconds(10);
    let uncertain_deadline_at = created_at + Duration::seconds(11);
    attempts
        .begin_dispatch(BeginConnectorExecutionDispatch::new(
            uncertain_fence,
            uncertain_started_at,
            uncertain_deadline_at,
        )?)
        .await?;
    assert!(matches!(
        attempts
            .reserve(ReserveConnectorExecutionAttempt::new(
                ConnectorExecutionAttemptBinding::from_exact(&revision, &uncertain_request)?,
                Uuid::now_v7(),
                created_at + Duration::seconds(12),
                created_at + Duration::seconds(30),
            )?)
            .await?,
        ConnectorExecutionReservation::Indeterminate(_)
    ));

    let attempt_get = GetConnectorExecutionAttemptHandler::new(attempts.clone())
        .execute(
            GetConnectorExecutionAttempt {
                organization_id,
                project_id,
                environment_id,
                profile_id,
                revision_id: revision.id,
                attempt_id: reserved_request.attempt_id(),
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            connector_context(),
        )
        .await??;
    assert_eq!(attempt_get.attempt.fence(), reserved_fence);
    assert_eq!(
        attempt_get
            .attempt
            .recovery_state(reserved_at + Duration::seconds(1)),
        ConnectorExecutionRecoveryState::Reserved
    );
    let attempt_list_handler =
        ListUnresolvedConnectorExecutionAttemptsHandler::new(profiles.clone(), attempts.clone());
    let unresolved = ListUnresolvedConnectorExecutionAttempts {
        organization_id,
        project_id,
        environment_id,
        profile_id,
        revision_id: revision.id,
        after: None,
        limit: 1,
        resource_access: ResourceAccessEvaluator::organization_wide(),
    };
    let unresolved_first = attempt_list_handler
        .execute(unresolved.clone(), connector_context())
        .await??;
    assert_eq!(unresolved_first.attempts.len(), 1);
    assert_eq!(
        unresolved_first.attempts[0].attempt.binding().attempt_id(),
        uncertain_request.attempt_id()
    );
    assert_eq!(
        unresolved_first.attempts[0]
            .attempt
            .recovery_state(created_at + Duration::seconds(12)),
        ConnectorExecutionRecoveryState::Indeterminate
    );
    assert_eq!(
        unresolved_first.next_cursor,
        Some(ConnectorExecutionAttemptCursor::after(
            &unresolved_first.attempts[0].attempt,
        ))
    );
    let unresolved_second = attempt_list_handler
        .execute(
            ListUnresolvedConnectorExecutionAttempts {
                after: unresolved_first.next_cursor,
                ..unresolved.clone()
            },
            connector_context(),
        )
        .await??;
    assert_eq!(unresolved_second.attempts.len(), 1);
    assert_eq!(
        unresolved_second.attempts[0].attempt.binding().attempt_id(),
        reserved_request.attempt_id()
    );
    let denied_attempts = attempt_list_handler
        .execute(
            ListUnresolvedConnectorExecutionAttempts {
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
                ..unresolved
            },
            connector_context(),
        )
        .await?;
    assert!(matches!(
        denied_attempts,
        Err(ApplicationError::NotFound(_))
    ));

    let recovered_attempt = PostgresConnectorExecutionAttemptRepository::new(executor.clone())
        .find(
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision.id,
            accepted.attempt_id(),
        )
        .await?
        .ok_or("recovered terminal Connector attempt is missing")?;
    assert_eq!(recovered_attempt.evidence, Some(accepted.clone()));

    assert_rejected(
        database
            .execute(
                sql_query::<()>("update connector_execution_evidence set outcome = 'rejected' where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and attempt_id = ")
                    .bind(accepted.attempt_id()),
            )
            .await,
        "mutating immutable Connector execution evidence",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "delete from connector_execution_evidence where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and attempt_id = ")
                .bind(accepted.attempt_id()),
            )
            .await,
        "deleting immutable Connector execution evidence",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("update connector_execution_attempts set request_digest = ")
                    .bind(rejected.request_digest().as_str())
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and attempt_id = ")
                    .bind(accepted.attempt_id()),
            )
            .await,
        "mutating immutable Connector attempt binding",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "delete from connector_execution_attempts where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and attempt_id = ")
                .bind(accepted.attempt_id()),
            )
            .await,
        "deleting terminal Connector execution attempt",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into connector_execution_evidence (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, outcome, response_status, response_digest, response_body_bytes, retry_after_seconds, started_at, completed_at) select organization_id, project_id, environment_id, profile_id, revision_id, ")
                    .bind(Uuid::now_v7())
                    .append(", request_digest, request_body_bytes, outcome, response_status, response_digest, response_body_bytes, retry_after_seconds, started_at, completed_at from connector_execution_evidence where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and attempt_id = ")
                    .bind(accepted.attempt_id()),
            )
            .await,
        "recording Connector evidence without an exact attempt",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into connector_execution_attempts (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, state, fence_generation, fence_token, reserved_at, lease_expires_at, dispatch_started_at, outcome_deadline_at, terminal_at, created_at) select organization_id, project_id, environment_id, profile_id, revision_id, ")
                    .bind(Uuid::now_v7())
                    .append(", request_digest, request_body_bytes, 'terminal', fence_generation, ")
                    .bind(Uuid::now_v7())
                    .append(", reserved_at, lease_expires_at, dispatch_started_at, outcome_deadline_at, terminal_at, created_at from connector_execution_attempts where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and attempt_id = ")
                    .bind(accepted.attempt_id()),
            )
            .await,
        "committing a terminal Connector attempt without evidence",
    );
    let forged_preflight_attempt_id = Uuid::now_v7();
    assert_rejected(
        database
            .execute(
                sql_query::<()>("with inserted_attempt as (insert into connector_execution_attempts (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, state, fence_generation, fence_token, reserved_at, lease_expires_at, dispatch_started_at, outcome_deadline_at, terminal_at, created_at) select organization_id, project_id, environment_id, profile_id, revision_id, ")
                    .bind(forged_preflight_attempt_id)
                    .append(", request_digest, request_body_bytes, 'terminal', 1, ")
                    .bind(Uuid::now_v7())
                    .append(", started_at, started_at + interval '30 seconds', null, null, completed_at, started_at from connector_execution_evidence where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and attempt_id = ")
                    .bind(accepted.attempt_id())
                    .append(" returning organization_id, project_id, environment_id, profile_id, revision_id, attempt_id) insert into connector_execution_evidence (organization_id, project_id, environment_id, profile_id, revision_id, attempt_id, request_digest, request_body_bytes, outcome, response_status, response_digest, response_body_bytes, retry_after_seconds, started_at, completed_at) select inserted_attempt.organization_id, inserted_attempt.project_id, inserted_attempt.environment_id, inserted_attempt.profile_id, inserted_attempt.revision_id, inserted_attempt.attempt_id, evidence.request_digest, evidence.request_body_bytes, evidence.outcome, evidence.response_status, evidence.response_digest, evidence.response_body_bytes, evidence.retry_after_seconds, evidence.started_at, evidence.completed_at from inserted_attempt cross join connector_execution_evidence evidence where evidence.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and evidence.attempt_id = ")
                    .bind(accepted.attempt_id()),
            )
            .await,
        "claiming an accepted provider response before dispatch",
    );
    let stored = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>("select count(*), count(*) filter (where request_digest like 'sha256:%' and (response_digest is null or response_digest like 'sha256:%')), (select count(*) from connector_execution_attempts where organization_id = ")
                .bind(organization_id.as_uuid())
                .append("), (select count(*) from connector_execution_attempts where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and state = 'terminal') from connector_execution_evidence where organization_id = ")
                .bind(organization_id.as_uuid()),
        )
        .await?;
    assert_eq!(stored, (3, 3, 7, 3));
    Ok(())
}

async fn prepare_connector_settlement(
    attempts: &PostgresConnectorExecutionAttemptRepository,
    revision: &ConnectorRevision,
    request: &ConnectorExecutionRequest,
    evidence: ConnectorExecutionEvidence,
) -> Result<SettleConnectorExecutionAttempt, Box<dyn std::error::Error>> {
    let reserved_at = evidence.started_at() - Duration::milliseconds(1);
    let fence = match attempts
        .reserve(ReserveConnectorExecutionAttempt::new(
            ConnectorExecutionAttemptBinding::from_exact(revision, request)?,
            Uuid::now_v7(),
            reserved_at,
            reserved_at + Duration::seconds(30),
        )?)
        .await?
    {
        ConnectorExecutionReservation::Acquired { fence, .. } => fence,
        other => return Err(format!("unexpected Connector reservation: {other:?}").into()),
    };
    attempts
        .begin_dispatch(BeginConnectorExecutionDispatch::new(
            fence.clone(),
            evidence.started_at(),
            evidence.started_at() + Duration::seconds(60),
        )?)
        .await?;
    Ok(SettleConnectorExecutionAttempt::new(fence, evidence)?)
}

fn connector_context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

#[derive(Default)]
struct ConnectorFixtureEncryption {
    ciphertexts: Mutex<Vec<String>>,
}

#[async_trait]
impl ISecretEncryptionService for ConnectorFixtureEncryption {
    async fn encrypt(
        &self,
        _plaintext: &[u8],
        _context: &[u8],
    ) -> Result<EncryptedSecretValue, SecretEncryptionError> {
        Err(SecretEncryptionError::Rejected(
            "Connector PostgreSQL fixture decrypts only".into(),
        ))
    }

    async fn decrypt(
        &self,
        value: &EncryptedSecretValue,
        _context: &[u8],
    ) -> Result<Vec<u8>, SecretEncryptionError> {
        self.ciphertexts
            .lock()
            .expect("Connector fixture encryption lock")
            .push(value.ciphertext().to_owned());
        match value.ciphertext() {
            "connector-application-destination" => {
                Ok(b"https://hooks.example.test/delivery?token=connector-token".to_vec())
            }
            "connector-application-hmac" => Ok(vec![b'h'; 32]),
            _ => Err(SecretEncryptionError::Rejected(
                "unexpected Connector fixture ciphertext".into(),
            )),
        }
    }

    async fn health(&self) -> Result<bool, SecretEncryptionError> {
        Ok(true)
    }
}

fn literal_definition_acl(timeout_milliseconds: u64) -> Result<String, String> {
    ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
        destination: ConnectorHttpDestination::LiteralHttps {
            endpoint: "https://hooks.example.test/revised".into(),
        },
        method: ConnectorHttpMethod::Post,
        request_content_type: "application/json".into(),
        maximum_request_bytes: 1024,
        maximum_response_bytes: 1024,
        timeout_milliseconds,
        status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
        authentication: ConnectorHttpAuthentication::None,
    })
    .map(|definition| definition.canonical_acl().to_owned())
}

async fn create_secret(
    repository: &PostgresSecretRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    name: &str,
    ciphertext: &str,
    created_at: chrono::DateTime<Utc>,
) -> Result<SecretId, Box<dyn std::error::Error>> {
    let secret_id = SecretId::new();
    let (secret, version) = Secret::create(
        secret_id,
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse(name)?,
        EncryptedSecretValue::new("integration-key", ciphertext)?,
        created_at,
    )?;
    let request_id = Uuid::now_v7();
    repository
        .create(CreateSecretWrite {
            event: SecretChanged::created(&secret, &version, request_id)?,
            idempotency: IdempotencyRequest::new(
                format!("connector-secret-{organization_id}"),
                secret_id.to_string(),
                ciphertext.as_bytes(),
            )?,
            secret,
            version,
        })
        .await?;
    Ok(secret_id)
}

fn definition(
    destination_secret_id: SecretId,
    hmac_secret_id: SecretId,
    timeout_milliseconds: u64,
) -> Result<ConnectorDefinition, String> {
    Ok(ConnectorDefinition::Http(
        ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
            destination: ConnectorHttpDestination::SecretHttpsUrl {
                reference: ConnectorSecretReference::new(destination_secret_id, 1)?,
            },
            method: ConnectorHttpMethod::Post,
            request_content_type: "application/json; charset=utf-8".into(),
            maximum_request_bytes: 16 * 1024,
            maximum_response_bytes: 32 * 1024,
            timeout_milliseconds,
            status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
            authentication: ConnectorHttpAuthentication::HmacSha256 {
                secret: ConnectorSecretReference::new(hmac_secret_id, 1)?,
                signature_header: "x-a3s-signature".into(),
                value_prefix: "v1=".into(),
            },
        })?,
    ))
}

fn destination_only_definition(secret_id: SecretId) -> Result<ConnectorDefinition, String> {
    Ok(ConnectorDefinition::Http(
        ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
            destination: ConnectorHttpDestination::SecretHttpsUrl {
                reference: ConnectorSecretReference::new(secret_id, 1)?,
            },
            method: ConnectorHttpMethod::Post,
            request_content_type: "application/json".into(),
            maximum_request_bytes: 1024,
            maximum_response_bytes: 1024,
            timeout_milliseconds: 1_000,
            status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
            authentication: ConnectorHttpAuthentication::None,
        })?,
    ))
}

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database must reject {label}");
}
