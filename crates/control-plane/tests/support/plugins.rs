use a3s_cloud_control_plane::infrastructure::connect_and_migrate;
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::plugins::domain::entities::{
    NewPluginRegistry, PluginRegistry,
};
use a3s_cloud_control_plane::modules::plugins::domain::events::PluginRegistryEnrolled;
use a3s_cloud_control_plane::modules::plugins::domain::repositories::{
    CreatePluginRegistryWrite, IPluginRegistryRepository,
};
use a3s_cloud_control_plane::modules::plugins::domain::services::{
    IPluginRegistryEnrollmentAuthorizer, PluginRegistryEnrollmentAuthorizationError,
};
use a3s_cloud_control_plane::modules::plugins::domain::value_objects::{
    PluginRegistryEndpoint, PluginTrustRoot,
};
use a3s_cloud_control_plane::modules::plugins::PostgresPluginRegistryRepository;
use a3s_cloud_control_plane::modules::search::{
    ISearchRepository, PostgresSearchRepository, SearchQuery, SearchResourceKind,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, RepositoryError, ResourceName, Sha256Digest,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use std::io;
use uuid::Uuid;

pub(super) async fn exercise_plugin_registry_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let foreign_organization_id = OrganizationId::new();
    let active_actor = PrincipalId::new();
    let revoked_after_preflight_actor = PrincipalId::new();
    let service_actor = PrincipalId::new();
    let disabled_actor = PrincipalId::new();
    let created_at = Utc::now();

    for (id, name, name_key) in [
        (
            organization_id,
            "Plugin persistence tenant",
            "plugin-persistence",
        ),
        (
            foreign_organization_id,
            "Foreign plugin tenant",
            "foreign-plugin",
        ),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(id.as_uuid())
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(name_key)
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }

    for (id, kind, name, disabled_at) in [
        (active_actor, "human", "Registry operator", None),
        (
            revoked_after_preflight_actor,
            "human",
            "Revoked registry operator",
            None,
        ),
        (service_actor, "service", "Registry automation", None),
        (
            disabled_actor,
            "human",
            "Disabled registry operator",
            Some(created_at + Duration::seconds(1)),
        ),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
                )
                .bind(id.as_uuid())
                .append(", ")
                .bind(kind)
                .append(", ")
                .bind(name)
                .append(", 1, ")
                .bind(created_at)
                .append(", ")
                .bind(disabled_at)
                .append(")"),
            )
            .await?;
        database
            .execute(
                sql_query::<()>(
                    "insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (",
                )
                .bind(Uuid::now_v7())
                .append(", ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(id.as_uuid())
                .append(", 'member', 1, ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", null)"),
            )
            .await?;
    }

    let repository = PostgresPluginRegistryRepository::new(executor.clone());
    repository
        .authorize_enrollment(organization_id, active_actor)
        .await?;
    for (tenant, actor) in [
        (foreign_organization_id, active_actor),
        (organization_id, service_actor),
        (organization_id, disabled_actor),
    ] {
        assert!(matches!(
            repository.authorize_enrollment(tenant, actor).await,
            Err(PluginRegistryEnrollmentAuthorizationError::Forbidden)
        ));
    }

    repository
        .authorize_enrollment(organization_id, revoked_after_preflight_actor)
        .await?;
    let revoked_at = created_at + Duration::seconds(2);
    database
        .execute(
            sql_query::<()>(
                "update organization_memberships set aggregate_version = 2, updated_at = ",
            )
            .bind(revoked_at)
            .append(", revoked_at = ")
            .bind(revoked_at)
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and principal_id = ")
            .bind(revoked_after_preflight_actor.as_uuid()),
        )
        .await?;
    let rejected_registry = registry(
        organization_id,
        revoked_after_preflight_actor,
        "Rejected registry",
        "https://rejected.registry.example/u0",
        'f',
        created_at + Duration::seconds(3),
    )?;
    assert!(matches!(
        repository
            .create(write(
                rejected_registry.clone(),
                "u0-postgres-revoked-after-preflight"
            )?)
            .await,
        Err(RepositoryError::Forbidden(_))
    ));
    assert_eq!(
        aggregate_write_counts(
            &database,
            rejected_registry.id.as_uuid(),
            "u0-postgres-revoked-after-preflight",
        )
        .await?,
        (0, 0, 0, 0)
    );

    let enrolled = registry(
        organization_id,
        active_actor,
        "Official registry",
        "https://registry.example/u0",
        'a',
        created_at + Duration::seconds(4),
    )?;
    let create = write(enrolled.clone(), "u0-postgres-official")?;
    let (left, right) = tokio::join!(
        repository.create(create.clone()),
        repository.create(create.clone())
    );
    let left = left?;
    let right = right?;
    assert_eq!(left.value, enrolled);
    assert_eq!(right.value, enrolled);
    assert_ne!(left.replayed, right.replayed);
    let reconstructed = PostgresPluginRegistryRepository::new(executor.clone());
    let replay = reconstructed.create(create.clone()).await?;
    assert!(replay.replayed);
    assert_eq!(replay.value, enrolled);

    let changed_root = registry(
        organization_id,
        active_actor,
        "Official registry",
        "https://registry.example/u0",
        'b',
        created_at + Duration::seconds(5),
    )?;
    assert_eq!(
        reconstructed
            .create(write(changed_root, "u0-postgres-official")?)
            .await
            .expect_err("changed enrollment input must not replay"),
        RepositoryError::IdempotencyConflict
    );

    let duplicate_name = registry(
        organization_id,
        active_actor,
        "Official registry",
        "https://duplicate-name.registry.example/u0",
        'c',
        created_at + Duration::seconds(6),
    )?;
    assert!(matches!(
        reconstructed
            .create(write(duplicate_name, "u0-postgres-duplicate-name")?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let duplicate_endpoint = registry(
        organization_id,
        active_actor,
        "Alternate registry",
        "https://registry.example/u0",
        'd',
        created_at + Duration::seconds(7),
    )?;
    assert!(matches!(
        reconstructed
            .create(write(duplicate_endpoint, "u0-postgres-duplicate-endpoint")?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(
        reconstructed.find(organization_id, enrolled.id).await?,
        Some(enrolled.clone())
    );
    assert_eq!(
        reconstructed
            .find(foreign_organization_id, enrolled.id)
            .await?,
        None
    );
    assert_eq!(
        reconstructed.list(organization_id).await?,
        vec![enrolled.clone()]
    );
    assert!(reconstructed
        .list(foreign_organization_id)
        .await?
        .is_empty());

    let search = PostgresSearchRepository::new(executor.clone());
    let query = SearchQuery::parse("official").map_err(test_error)?;
    let results = search
        .search(
            organization_id,
            &query,
            20,
            &ResourceAccessEvaluator::organization_wide(),
        )
        .await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].kind, SearchResourceKind::PluginRegistry);
    assert_eq!(results[0].id, enrolled.id.as_uuid());
    assert_eq!(results[0].title, enrolled.name.as_str());
    assert_eq!(
        results[0].description,
        "Plugin registry · https://registry.example/u0/"
    );
    assert_eq!(results[0].state.as_deref(), Some("active"));
    assert!(search
        .search(
            foreign_organization_id,
            &query,
            20,
            &ResourceAccessEvaluator::organization_wide(),
        )
        .await?
        .is_empty());

    assert_eq!(
        aggregate_write_counts(&database, enrolled.id.as_uuid(), "u0-postgres-official",).await?,
        (1, 1, 1, 1)
    );
    let migration_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from a3s_orm_migrations where version in ('084', '085')",
        ))
        .await?;
    assert_eq!(migration_count, 2);

    database
        .execute(
            sql_query::<()>("update plugin_registries set endpoint = ")
                .bind("https://REGISTRY.example/u0/")
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(enrolled.id.as_uuid()),
        )
        .await?;
    assert!(matches!(
        reconstructed.find(organization_id, enrolled.id).await,
        Err(RepositoryError::Storage(_))
    ));
    database
        .execute(
            sql_query::<()>("update plugin_registries set endpoint = ")
                .bind(enrolled.endpoint.as_str())
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(enrolled.id.as_uuid()),
        )
        .await?;
    assert_eq!(
        reconstructed.find(organization_id, enrolled.id).await?,
        Some(enrolled)
    );

    println!(
        "A3S_CLOUD_U0_POSTGRES_CERTIFIED store=postgresql schema=084 search=085 registries=1 outbox=1 audit=1 idempotency=1 checks=12/12"
    );
    Ok(())
}

fn registry(
    organization_id: OrganizationId,
    actor_id: PrincipalId,
    name: &str,
    endpoint: &str,
    digest_character: char,
    enrolled_at: chrono::DateTime<Utc>,
) -> Result<PluginRegistry, io::Error> {
    let digest = Sha256Digest::parse(format!(
        "sha256:{}",
        digest_character.to_string().repeat(64)
    ))
    .map_err(test_error)?;
    PluginRegistry::enroll(NewPluginRegistry {
        organization_id,
        id: a3s_cloud_control_plane::modules::shared_kernel::domain::PluginRegistryId::new(),
        name: ResourceName::parse(name).map_err(test_error)?,
        endpoint: PluginRegistryEndpoint::parse(endpoint).map_err(test_error)?,
        trust_root: PluginTrustRoot::from_digest(digest, 7).map_err(test_error)?,
        actor_id,
        request_id: Uuid::now_v7(),
        enrolled_at,
    })
    .map_err(test_error)
}

fn write(registry: PluginRegistry, key: &str) -> Result<CreatePluginRegistryWrite, io::Error> {
    Ok(CreatePluginRegistryWrite {
        event: PluginRegistryEnrolled::envelope(&registry).map_err(test_error)?,
        actor_id: registry.last_actor_id,
        request_id: registry.last_request_id,
        idempotency: CreatePluginRegistryWrite::idempotency_for(&registry, key)
            .map_err(test_error)?,
        registry,
    })
}

async fn aggregate_write_counts(
    database: &Database<PostgresDialect, PostgresExecutor>,
    aggregate_id: Uuid,
    idempotency_key: &str,
) -> Result<(i64, i64, i64, i64), Box<dyn std::error::Error>> {
    Ok(database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>(
                "select (select count(*) from plugin_registries where id = ",
            )
            .bind(aggregate_id)
            .append("), (select count(*) from outbox_events where aggregate_id = ")
            .bind(aggregate_id)
            .append(" and event_key = 'plugins.registry.enrolled'), (select count(*) from audit_records where aggregate_id = ")
            .bind(aggregate_id)
            .append(" and action = 'plugins.registry.enrolled'), (select count(*) from idempotency_records where idempotency_key = ")
            .bind(idempotency_key)
            .append(")"),
        )
        .await?)
}

fn test_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
