use crate::migrate_and_connect_for_test;
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::NodeCapabilities;
use a3s_cloud_control_plane::modules::plugins::domain::entities::{
    NewPluginAssignment, NewPluginPlanProjection, NewPluginRegistry, PluginAssignment,
    PluginPlanProjection, PluginRegistry,
};
use a3s_cloud_control_plane::modules::plugins::domain::events::PluginAssignmentChanged;
use a3s_cloud_control_plane::modules::plugins::domain::repositories::{
    CreatePluginAssignmentWrite, IPluginAssignmentRepository, IPluginPlanProjectionRepository,
};
use a3s_cloud_control_plane::modules::plugins::domain::value_objects::{
    PluginCatalogSelection, PluginRegistryEndpoint, PluginTrustRoot,
};
use a3s_cloud_control_plane::modules::plugins::{
    PostgresPluginAssignmentRepository, PostgresPluginPlanProjectionRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OperationId, OrganizationId, PluginAssignmentId, PluginPlanProjectionId,
    PluginRegistryId, PrincipalId, ProjectId, RepositoryError, ResourceName, Sha256Digest,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_use_core::{
    PlanScopeKind, PluginDesiredState, PluginManagedScope, PluginOperationConfirmation,
    PluginOperationPlan, PluginOperationPlanEnvelope, PluginPackageId, PluginSurfaceKind,
    PluginSurfaceRef, PLUGIN_MANAGED_SCOPE_SCHEMA_V2,
};
use chrono::{Duration, Utc};
use std::io;
use uuid::Uuid;

const INSTALL_PLAN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/plugins/operation-plan-install-v4.json"
));
const CONFIRMATION: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/plugins/operation-confirmation-v1.json"
));

/// Live PostgreSQL 17 gate for U0.3 assignment + plan-projection persistence.
///
/// Proves transactional create/replay, one live host/package uniqueness, tenant
/// isolation, FK fail-closed writes, plan projection digest idempotency,
/// confirmation CAS, and migrations 189-191 without exercising Fleet/Flow.
pub(super) async fn exercise_plugin_assignment_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {

    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let foreign_organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let foreign_project_id = ProjectId::new();
    let foreign_environment_id = EnvironmentId::new();
    let actor_id = PrincipalId::new();
    let host_id = NodeId::new();
    let foreign_host_id = NodeId::new();
    let registry_id = PluginRegistryId::new();
    let created_at = Utc::now();

    seed_tenants(
        &database,
        organization_id,
        foreign_organization_id,
        project_id,
        environment_id,
        foreign_project_id,
        foreign_environment_id,
        actor_id,
        host_id,
        foreign_host_id,
        registry_id,
        created_at,
    )
    .await?;

    let assignments = PostgresPluginAssignmentRepository::new(executor.clone());
    let created = assignment(
        organization_id,
        project_id,
        environment_id,
        registry_id,
        host_id,
        actor_id,
        "acme/research",
        created_at + Duration::seconds(1),
    )?;
    let create = write(created.clone(), "u03-postgres-assign-1")?;
    let (left, right) = tokio::join!(
        assignments.create(create.clone()),
        assignments.create(create.clone())
    );
    let left = left?;
    let right = right?;
    assert_eq!(left.value, created);
    assert_eq!(right.value, created);
    assert_ne!(left.replayed, right.replayed);

    let reconstructed = PostgresPluginAssignmentRepository::new(executor.clone());
    let replay = reconstructed.create(create.clone()).await?;
    assert!(replay.replayed);
    assert_eq!(replay.value, created);

    let mut changed = assignment(
        organization_id,
        project_id,
        environment_id,
        registry_id,
        host_id,
        actor_id,
        "acme/research",
        created_at + Duration::seconds(2),
    )?;
    changed.selection.catalog_record_digest = digest('1');
    assert_eq!(
        reconstructed
            .create(write(changed, "u03-postgres-assign-1")?)
            .await
            .expect_err("changed assignment input must not replay"),
        RepositoryError::IdempotencyConflict
    );

    let duplicate_host_package = assignment(
        organization_id,
        project_id,
        environment_id,
        registry_id,
        host_id,
        actor_id,
        "acme/research",
        created_at + Duration::seconds(3),
    )?;
    assert!(matches!(
        reconstructed
            .create(write(duplicate_host_package, "u03-postgres-assign-2")?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(
        reconstructed.find(organization_id, created.id).await?,
        Some(created.clone())
    );
    assert_eq!(
        reconstructed
            .find(foreign_organization_id, created.id)
            .await?,
        None
    );
    assert_eq!(
        reconstructed
            .list_for_environment(organization_id, environment_id)
            .await?,
        vec![created.clone()]
    );
    assert!(reconstructed
        .list_for_environment(foreign_organization_id, foreign_environment_id)
        .await?
        .is_empty());
    assert_eq!(
        reconstructed
            .find_live_for_host_package(
                organization_id,
                host_id,
                &PluginPackageId::parse("acme/research").map_err(test_error)?,
            )
            .await?
            .expect("live")
            .id,
        created.id
    );

    let missing_host = assignment(
        organization_id,
        project_id,
        environment_id,
        registry_id,
        NodeId::new(),
        actor_id,
        "acme/knowledge",
        created_at + Duration::seconds(4),
    )?;
    assert!(matches!(
        reconstructed
            .create(write(missing_host.clone(), "u03-postgres-missing-host")?)
            .await,
        Err(RepositoryError::NotFound)
    ));
    assert_eq!(
        assignment_write_counts(&database, created.id.as_uuid(), "u03-postgres-assign-1").await?,
        (1, 1, 1, 1)
    );
    assert_eq!(
        assignment_write_counts(
            &database,
            missing_host.id.as_uuid(),
            "u03-postgres-missing-host",
        )
        .await?,
        (0, 0, 0, 0)
    );

    let plan = PluginOperationPlan::from_json(INSTALL_PLAN).map_err(test_error)?;
    let envelope = PluginOperationPlanEnvelope::new(plan).map_err(test_error)?;
    let projections = PostgresPluginPlanProjectionRepository::new(executor.clone());
    let plan_created_at = chrono::DateTime::from_timestamp_millis(1_785_360_000_000)
        .ok_or_else(|| test_error("plan created_at"))?;
    let projection = PluginPlanProjection::from_validated_envelope(NewPluginPlanProjection {
        organization_id,
        id: PluginPlanProjectionId::new(),
        assignment_id: created.id,
        operation_id: created
            .current_operation_id
            .expect("assignment operation"),
        assignment_generation: created.assignment_generation,
        envelope: envelope.clone(),
        created_at: plan_created_at,
    })
    .map_err(test_error)?;
    let stored = projections.create(projection.clone()).await?;
    assert_eq!(stored, projection);
    assert_eq!(
        projections
            .find_by_plan_digest(organization_id, &projection.plan_digest)
            .await?,
        Some(projection.clone())
    );
    assert!(matches!(
        projections.create(projection.clone()).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(projections
        .find(foreign_organization_id, projection.id)
        .await?
        .is_none());

    let confirmation =
        PluginOperationConfirmation::from_json(CONFIRMATION).map_err(test_error)?;
    let confirmed_at = chrono::DateTime::from_timestamp_millis(1_785_360_200_000)
        .ok_or_else(|| test_error("confirmation time"))?;
    let confirmed = projection
        .confirm(&confirmation, confirmed_at)
        .map_err(test_error)?;
    let updated = projections.update(confirmed.clone(), None).await?;
    assert_eq!(updated.confirmation_digest, confirmed.confirmation_digest);
    assert!(updated.confirmation.is_some());
    assert!(matches!(
        projections.update(confirmed.clone(), None).await,
        Err(RepositoryError::Conflict(_))
    ));

    let migration_count = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from a3s_orm_migrations where version in ('189', '190', '191')",
        ))
        .await?;
    assert_eq!(migration_count, 3);

    database
        .execute(
            sql_query::<()>("update plugin_assignments set workspace_scope = ")
                .bind(serde_json::json!({"schema": "not-a-managed-scope"}))
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(created.id.as_uuid()),
        )
        .await?;
    assert!(matches!(
        reconstructed.find(organization_id, created.id).await,
        Err(RepositoryError::Storage(_))
    ));
    database
        .execute(
            sql_query::<()>("update plugin_assignments set workspace_scope = ")
                .bind(serde_json::to_value(&created.workspace_scope).map_err(test_error)?)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(created.id.as_uuid()),
        )
        .await?;
    assert_eq!(
        reconstructed.find(organization_id, created.id).await?,
        Some(created)
    );

    println!(
        "A3S_CLOUD_U0_3_POSTGRES_CERTIFIED store=postgresql schema=189,190,191 assignments=1 projections=1 confirmation=1 outbox=1 audit=1 idempotency=1 checks=12/12"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn seed_tenants(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    foreign_organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    foreign_project_id: ProjectId,
    foreign_environment_id: EnvironmentId,
    actor_id: PrincipalId,
    host_id: NodeId,
    foreign_host_id: NodeId,
    registry_id: PluginRegistryId,
    created_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    for (id, name, name_key) in [
        (
            organization_id,
            "Plugin assignment tenant",
            "plugin-assignment",
        ),
        (
            foreign_organization_id,
            "Foreign assignment tenant",
            "foreign-assignment",
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

    database
        .execute(
            sql_query::<()>(
                "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
            )
            .bind(actor_id.as_uuid())
            .append(", 'human', 'Assignment operator', 1, ")
            .bind(created_at)
            .append(", null)"),
        )
        .await?;

    for (org, project, environment, project_name, project_key, env_name, env_key) in [
        (
            organization_id,
            project_id,
            environment_id,
            "Assignment project",
            "assignment-project",
            "Assignment env",
            "assignment-env",
        ),
        (
            foreign_organization_id,
            foreign_project_id,
            foreign_environment_id,
            "Foreign project",
            "foreign-project",
            "Foreign env",
            "foreign-env",
        ),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(org.as_uuid())
                .append(", ")
                .bind(project.as_uuid())
                .append(", ")
                .bind(project_name)
                .append(", ")
                .bind(project_key)
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
        database
            .execute(
                sql_query::<()>(
                    "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(org.as_uuid())
                .append(", ")
                .bind(project.as_uuid())
                .append(", ")
                .bind(environment.as_uuid())
                .append(", ")
                .bind(env_name)
                .append(", ")
                .bind(env_key)
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
    }

    let capabilities =
        NodeCapabilities::new("test-runtime", "test-runtime-1", serde_json::json!({}))
            .map_err(test_error)?;
    for (org, node, name, key) in [
        (
            organization_id,
            host_id,
            "assignment host",
            "assignment-host",
        ),
        (
            foreign_organization_id,
            foreign_host_id,
            "foreign host",
            "foreign-host",
        ),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, last_sequence, aggregate_version) values (",
                )
                .bind(org.as_uuid())
                .append(", ")
                .bind(node.as_uuid())
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(key)
                .append(", 'ready', ")
                .bind(Uuid::now_v7())
                .append(", 'test', 'test-runtime', 'test-runtime-1', ")
                .bind(capabilities.digest())
                .append(", ")
                .bind(capabilities.document().clone())
                .append(", ")
                .bind(created_at)
                .append(", ")
                .bind(created_at)
                .append(", 0, 1)"),
            )
            .await?;
    }

    let registry = PluginRegistry::enroll(NewPluginRegistry {
        organization_id,
        id: registry_id,
        name: ResourceName::parse("Assignment registry").map_err(test_error)?,
        endpoint: PluginRegistryEndpoint::parse("https://registry.example/u03")
            .map_err(test_error)?,
        trust_root: PluginTrustRoot::from_digest(digest('a'), 7).map_err(test_error)?,
        actor_id,
        request_id: Uuid::now_v7(),
        enrolled_at: created_at,
    })
    .map_err(test_error)?;
    database
        .execute(
            sql_query::<()>(
                "insert into plugin_registries (organization_id, id, name, name_key, endpoint, root_object_ref, root_sha256, root_version, state, aggregate_version, last_actor_id, last_request_id, created_at, updated_at) values (",
            )
            .bind(registry.organization_id.as_uuid())
            .append(", ")
            .bind(registry.id.as_uuid())
            .append(", ")
            .bind(registry.name.as_str())
            .append(", ")
            .bind(registry.name.key())
            .append(", ")
            .bind(registry.endpoint.as_str())
            .append(", ")
            .bind(registry.trust_root.object_ref().as_str())
            .append(", ")
            .bind(registry.trust_root.digest().as_str())
            .append(", ")
            .bind(registry.trust_root.version() as i64)
            .append(", ")
            .bind(registry.state.as_str())
            .append(", ")
            .bind(registry.aggregate_version as i64)
            .append(", ")
            .bind(registry.last_actor_id.as_uuid())
            .append(", ")
            .bind(registry.last_request_id)
            .append(", ")
            .bind(registry.created_at)
            .append(", ")
            .bind(registry.updated_at)
            .append(")"),
        )
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn assignment(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    registry_id: PluginRegistryId,
    target_host_id: NodeId,
    actor_id: PrincipalId,
    package_id: &str,
    created_at: chrono::DateTime<Utc>,
) -> Result<PluginAssignment, io::Error> {
    PluginAssignment::create(NewPluginAssignment {
        organization_id,
        project_id,
        environment_id,
        id: PluginAssignmentId::new(),
        registry_id,
        target_host_id,
        workspace_scope: PluginManagedScope {
            schema: PLUGIN_MANAGED_SCOPE_SCHEMA_V2.into(),
            host_id: "host:node-01".into(),
            scope_kind: PlanScopeKind::Workspace,
            scope_id: "workspace:research".into(),
            authority_id: "cloud:organization-01".into(),
            fence_generation: 7,
            fence_digest: digest('d').as_str().into(),
        },
        selection: PluginCatalogSelection {
            package_id: PluginPackageId::parse(package_id).map_err(test_error)?,
            catalog_record_digest: digest('e'),
            version: "2.0.0".into(),
            package_digest: digest('b'),
            manifest_digest: digest('c'),
            selected_surfaces: vec![
                PluginSurfaceRef {
                    kind: PluginSurfaceKind::Skill,
                    id: "review".into(),
                },
                PluginSurfaceRef {
                    kind: PluginSurfaceKind::Ui,
                    id: "review".into(),
                },
            ],
        },
        policy_digest: digest('f'),
        desired_state: PluginDesiredState::Enabled,
        actor_id,
        request_id: Uuid::now_v7(),
        operation_id: OperationId::new(),
        created_at,
    })
    .map_err(test_error)
}

fn write(assignment: PluginAssignment, key: &str) -> Result<CreatePluginAssignmentWrite, io::Error> {
    Ok(CreatePluginAssignmentWrite {
        event: PluginAssignmentChanged::envelope(&assignment).map_err(test_error)?,
        idempotency: CreatePluginAssignmentWrite::idempotency_for(&assignment, key)
            .map_err(test_error)?,
        assignment,
    })
}

async fn assignment_write_counts(
    database: &Database<PostgresDialect, PostgresExecutor>,
    aggregate_id: Uuid,
    idempotency_key: &str,
) -> Result<(i64, i64, i64, i64), Box<dyn std::error::Error>> {
    Ok(database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>(
                "select (select count(*) from plugin_assignments where id = ",
            )
            .bind(aggregate_id)
            .append("), (select count(*) from outbox_events where aggregate_id = ")
            .bind(aggregate_id)
            .append(" and event_key = 'plugin.assignment.changed'), (select count(*) from audit_records where aggregate_id = ")
            .bind(aggregate_id)
            .append(" and action = 'plugin.assignment.changed'), (select count(*) from idempotency_records where idempotency_key = ")
            .bind(idempotency_key)
            .append(")"),
        )
        .await?)
}

fn digest(byte: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
}

fn test_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}
