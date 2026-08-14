use super::*;
use a3s_cloud_control_plane::modules::connectors::{
    ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDefinition,
    ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
    ConnectorHttpStatusPolicy, ConnectorProfile, ConnectorRecord, ConnectorRevision,
    ConnectorRevisionPublished, ConnectorSecretReference, CreateConnectorProfileWrite,
    IConnectorProfileRepository, PostgresConnectorProfileRepository, ReviseConnectorProfileWrite,
};
use a3s_cloud_control_plane::modules::secrets::{
    CreateSecretWrite, EncryptedSecretValue, ISecretRepository, PostgresSecretRepository, Secret,
    SecretChanged,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotencyRequest, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, ResourceName, SecretId,
};
use chrono::Duration;

pub(super) async fn exercise_connector_profile_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
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
            .list_revisions(organization_id, project_id, environment_id, profile_id)
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
