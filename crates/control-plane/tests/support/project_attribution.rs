use super::*;
use a3s_cloud_control_plane::modules::projects::domain::entities::{
    Project, ProjectAttributionProfile,
};
use a3s_cloud_control_plane::modules::projects::domain::events::{
    ProjectAttributionProfileUpdated, ProjectCreated,
};
use a3s_cloud_control_plane::modules::projects::domain::repositories::{
    IProjectRepository, ProjectAttributionRecord, UpdateProjectAttributionWrite,
};
use a3s_cloud_control_plane::modules::projects::domain::value_objects::{
    BusinessOwnerReference, CostAttributionCode, ProjectAttributionLabels, ProjectName,
};
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
};
use std::collections::BTreeMap;

pub(super) async fn exercise_project_attribution_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("104"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (1, "immutable project attribution profiles".into())
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor_id = Uuid::now_v7();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Attribution tenant', ")
            .bind(format!("attribution-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                .bind(actor_id)
                .append(", 'human', 'Attribution owner', 1, ")
                .bind(created_at)
                .append(", null)"),
        )
        .await?;
    let repository = PostgresProjectsRepository::new(executor.clone());
    let project = Project::create(
        organization_id,
        project_id,
        ProjectName::parse("Platform")?,
        created_at,
    );
    repository
        .create(
            project.clone(),
            ProjectCreated::envelope(&project, Uuid::now_v7())?,
            IdempotencyRequest::new(
                format!("organizations/{organization_id}/projects"),
                "postgres:project-attribution:create",
                b"platform",
            )?,
        )
        .await?;

    let first_id = ProjectAttributionProfileId::new();
    let first = ProjectAttributionProfile::create(
        organization_id,
        project_id,
        first_id,
        None,
        BusinessOwnerReference::parse("finance/platform")?,
        Some(CostAttributionCode::parse("CC-1042")?),
        ProjectAttributionLabels::parse(BTreeMap::from([(
            "service.tier".into(),
            "critical".into(),
        )]))?,
        PrincipalId::from_uuid(actor_id),
        created_at + chrono::Duration::seconds(1),
    )?;
    let first_project = project.point_to_attribution_profile(1, first_id)?;
    let first_idempotency = IdempotencyRequest::new(
        format!("organizations/{organization_id}/projects/{project_id}/attribution-profiles"),
        "postgres:project-attribution:first",
        b"first",
    )?;
    let first_request_id = Uuid::now_v7();
    let first_write = UpdateProjectAttributionWrite {
        record: ProjectAttributionRecord {
            project: first_project.clone(),
            attribution_profile: first.clone(),
        },
        expected_project_version: 1,
        event: ProjectAttributionProfileUpdated::envelope(
            &first,
            first_project.aggregate_version,
            first_request_id,
        )?,
        idempotency: first_idempotency.clone(),
        request_id: first_request_id,
    };
    let first_result = repository.update_attribution(first_write).await?;
    assert!(!first_result.replayed);
    let replay = repository
        .replay_attribution_update(&first_idempotency)
        .await?
        .ok_or_else(|| std::io::Error::other("first attribution replay is missing"))?;
    assert!(replay.replayed);
    assert_eq!(replay.value.attribution_profile.id, first_id);
    let conflicting_replay = IdempotencyRequest::new(
        format!("organizations/{organization_id}/projects/{project_id}/attribution-profiles"),
        "postgres:project-attribution:first",
        b"different",
    )?;
    assert!(repository
        .replay_attribution_update(&conflicting_replay)
        .await
        .is_err());

    let second_id = ProjectAttributionProfileId::new();
    let second = ProjectAttributionProfile::create(
        organization_id,
        project_id,
        second_id,
        Some(first_id),
        BusinessOwnerReference::parse("engineering/platform")?,
        None,
        ProjectAttributionLabels::parse(BTreeMap::from([("team".into(), "platform".into())]))?,
        PrincipalId::from_uuid(actor_id),
        created_at + chrono::Duration::seconds(2),
    )?;
    let second_project = first_project.point_to_attribution_profile(2, second_id)?;
    let second_request_id = Uuid::now_v7();
    repository
        .update_attribution(UpdateProjectAttributionWrite {
            record: ProjectAttributionRecord {
                project: second_project.clone(),
                attribution_profile: second.clone(),
            },
            expected_project_version: 2,
            event: ProjectAttributionProfileUpdated::envelope(
                &second,
                second_project.aggregate_version,
                second_request_id,
            )?,
            idempotency: IdempotencyRequest::new(
                format!(
                    "organizations/{organization_id}/projects/{project_id}/attribution-profiles"
                ),
                "postgres:project-attribution:second",
                b"second",
            )?,
            request_id: second_request_id,
        })
        .await?;

    let first_audit = database
        .fetch_one_as(
            sql_query::<(Option<Uuid>, Option<Uuid>, Option<Uuid>, String)>(
                "select project_id, environment_id, attribution_profile_id, attribution_status from audit_records where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and request_id = ")
            .bind(first_request_id)
            .append(" and action = 'project.attribution-profile.updated'"),
        )
        .await?;
    let second_audit = database
        .fetch_one_as(
            sql_query::<(Option<Uuid>, Option<Uuid>, Option<Uuid>, String)>(
                "select project_id, environment_id, attribution_profile_id, attribution_status from audit_records where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and request_id = ")
            .bind(second_request_id)
            .append(" and action = 'project.attribution-profile.updated'"),
        )
        .await?;
    assert_eq!(
        first_audit,
        (
            Some(project_id.as_uuid()),
            None,
            Some(first_id.as_uuid()),
            "profile_bound".into(),
        )
    );
    assert_eq!(
        second_audit,
        (
            Some(project_id.as_uuid()),
            None,
            Some(second_id.as_uuid()),
            "profile_bound".into(),
        )
    );

    let stale_id = ProjectAttributionProfileId::new();
    let stale = ProjectAttributionProfile::create(
        organization_id,
        project_id,
        stale_id,
        Some(first_id),
        BusinessOwnerReference::parse("stale")?,
        None,
        ProjectAttributionLabels::default(),
        PrincipalId::from_uuid(actor_id),
        created_at + chrono::Duration::seconds(3),
    )?;
    let stale_project = first_project.point_to_attribution_profile(2, stale_id)?;
    let stale_request_id = Uuid::now_v7();
    assert!(repository
        .update_attribution(UpdateProjectAttributionWrite {
            record: ProjectAttributionRecord {
                project: stale_project.clone(),
                attribution_profile: stale.clone(),
            },
            expected_project_version: 2,
            event: ProjectAttributionProfileUpdated::envelope(
                &stale,
                stale_project.aggregate_version,
                stale_request_id,
            )?,
            idempotency: IdempotencyRequest::new(
                format!(
                    "organizations/{organization_id}/projects/{project_id}/attribution-profiles"
                ),
                "postgres:project-attribution:stale",
                b"stale",
            )?,
            request_id: stale_request_id,
        })
        .await
        .is_err());

    let project = repository
        .find(organization_id, project_id)
        .await?
        .ok_or_else(|| std::io::Error::other("project attribution pointer is missing"))?;
    assert_eq!(project.aggregate_version, 3);
    assert_eq!(project.current_attribution_profile_id, Some(second_id));
    let first = repository
        .find_attribution_profile(organization_id, project_id, first_id)
        .await?
        .ok_or_else(|| std::io::Error::other("first attribution profile is missing"))?;
    let second = repository
        .find_attribution_profile(organization_id, project_id, second_id)
        .await?
        .ok_or_else(|| std::io::Error::other("second attribution profile is missing"))?;
    assert_eq!(first.business_owner_reference.as_str(), "finance/platform");
    assert_eq!(second.previous_profile_id, Some(first_id));
    assert_eq!(
        second.business_owner_reference.as_str(),
        "engineering/platform"
    );

    assert_rejected(
        database
            .execute(
                sql_query::<()>("update project_attribution_profiles set business_owner_reference = 'tampered' where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and id = ")
                    .bind(first_id.as_uuid()),
            )
            .await,
        "update immutable profile",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>(
                    "delete from project_attribution_profiles where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and id = ")
                .bind(first_id.as_uuid()),
            )
            .await,
        "delete immutable profile",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into project_attribution_profiles (organization_id, project_id, id, previous_profile_id, business_owner_reference, cost_attribution_code, labels, created_by, created_at) values (")
                    .bind(organization_id.as_uuid())
                    .append(", ")
                    .bind(ProjectId::new().as_uuid())
                    .append(", ")
                    .bind(Uuid::now_v7())
                    .append(", null, 'foreign', null, '{}'::jsonb, ")
                    .bind(actor_id)
                    .append(", ")
                    .bind(created_at)
                    .append(")"),
            )
            .await,
        "insert profile outside its project",
    );
    assert_rejected(
        database
            .execute(
                sql_query::<()>("insert into project_attribution_profiles (organization_id, project_id, id, previous_profile_id, business_owner_reference, cost_attribution_code, labels, created_by, created_at) values (")
                    .bind(organization_id.as_uuid())
                    .append(", ")
                    .bind(project_id.as_uuid())
                    .append(", ")
                    .bind(Uuid::now_v7())
                    .append(", ")
                    .bind(first_id.as_uuid())
                    .append(", 'fork', null, '{\"Invalid.Key\":42}'::jsonb, ")
                    .bind(actor_id)
                    .append(", ")
                    .bind(created_at)
                    .append(")"),
            )
            .await,
        "invalid labels and forked lineage",
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64)>("select (select count(*) from project_attribution_profiles where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append("), (select count(*) from projects where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(project_id.as_uuid())
                .append(" and current_attribution_profile_id = ")
                .bind(second_id.as_uuid())
                .append("), (select count(*) from project_attribution_profiles where previous_profile_id = ")
                .bind(first_id.as_uuid())
                .append("), (select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key = 'project.attribution-profile.updated'), (select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and action = 'project.attribution-profile.updated'), (select count(*) from idempotency_records where scope_key = ")
                .bind(format!("organizations/{organization_id}/projects/{project_id}/attribution-profiles"))
                .append(")"),
        )
        .await?;
    assert_eq!(evidence, (2, 1, 1, 2, 2, 2));
    Ok(())
}

fn assert_rejected<T, E: std::fmt::Debug>(result: Result<T, E>, label: &str) {
    assert!(result.is_err(), "database must reject {label}");
}
