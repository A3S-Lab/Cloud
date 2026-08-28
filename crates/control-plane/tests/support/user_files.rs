use super::*;
use a3s_cloud_control_plane::modules::files::{
    IUserFileRepository, PostgresUserFileRepository, RecordUserFileScan, RecordUserFileUpload,
    ReserveUserFileWrite, SharedUserFileObjectStore, UserFile, UserFileAdmissionContract,
    UserFileAdmissionContractSpec, UserFileApplicationService, UserFileContentReference,
    UserFileLifecycleChanged, UserFileScanDecision, UserFileScanPolicy, UserFileState,
    UserFileTransition,
};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
    UserFileId, UserFileUploadId,
};
use a3s_orm::DatabaseError;
use std::{io::Cursor, sync::Arc};

const TEST_QUOTA_BYTES: i64 = 8_192;
const RESERVATION_BYTES: u64 = 6_000;

pub(super) async fn exercise_user_file_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let migration_state = database
        .fetch_one_as(
            sql_query::<(i64, String)>(
                "select count(*), max(name) from a3s_orm_migrations where version = ",
            )
            .bind("170"),
        )
        .await?;
    assert_eq!(
        migration_state,
        (
            1,
            "UserFile lifecycle metadata and organization quota".into()
        )
    );

    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let other_project_id = ProjectId::new();
    let actor_principal_id = PrincipalId::new();
    let created_at = Utc::now() - chrono::Duration::minutes(1);
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'UserFile tenant', ")
            .bind(format!("user-files-{organization_id}"))
            .append(", 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(other_project_id.as_uuid())
            .append(", 'Other knowledge files', 'other-knowledge-files', 1, ")
            .bind(created_at)
            .append(")"),
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
            .append(", 'Knowledge files', 'knowledge-files', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into user_file_organization_quotas (organization_id, limit_bytes, allocated_bytes, revision, updated_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(TEST_QUOTA_BYTES)
            .append(", 0, 0, null)"),
        )
        .await?;

    let repository = PostgresUserFileRepository::new(executor.clone());
    let scope = format!("organizations/{organization_id}/projects/{project_id}/user-files");

    // Force the final audit insert to fail. Every earlier mutation in the
    // repository transaction, including quota, aggregate, and Outbox, must
    // roll back with it.
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "alter table audit_records add constraint user_file_audit_failure_probe check (action <> 'file.user-file.reserved')",
        )
        .await?;
    let rollback_probe = reserve_write(
        user_file(
            organization_id,
            project_id,
            actor_principal_id,
            "rollback-probe.bin",
            512,
            b"rollback probe",
            created_at,
        )?,
        &scope,
        "postgres:user-file:rollback-probe",
        b"rollback-probe",
    )?;
    let failed = repository.reserve(rollback_probe).await;
    executor
        .pool()
        .get()
        .await?
        .batch_execute("alter table audit_records drop constraint user_file_audit_failure_probe")
        .await?;
    assert!(matches!(failed, Err(RepositoryError::Storage(_))));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from user_files where organization_id = ")
                    .bind(organization_id.as_uuid()),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(i64, i64)>(
                    "select allocated_bytes, revision from user_file_organization_quotas where organization_id = ",
                )
                .bind(organization_id.as_uuid()),
            )
            .await?,
        (0, 0)
    );
    assert_eq!(
        user_file_side_effect_counts(&database, organization_id, &scope).await?,
        (0, 0, 0)
    );

    // Each file fits independently, but both cannot fit in the organization
    // quota. Row locking must serialize the allocation decision.
    let left_bytes = vec![b'l'; RESERVATION_BYTES as usize];
    let right_bytes = vec![b'r'; RESERVATION_BYTES as usize];
    let left_write = reserve_write(
        user_file(
            organization_id,
            project_id,
            actor_principal_id,
            "left.bin",
            RESERVATION_BYTES,
            &left_bytes,
            created_at + chrono::Duration::seconds(1),
        )?,
        &scope,
        "postgres:user-file:left",
        b"left",
    )?;
    let right_write = reserve_write(
        user_file(
            organization_id,
            project_id,
            actor_principal_id,
            "right.bin",
            RESERVATION_BYTES,
            &right_bytes,
            created_at + chrono::Duration::seconds(2),
        )?,
        &scope,
        "postgres:user-file:right",
        b"right",
    )?;
    let (left_result, right_result) = tokio::join!(
        repository.reserve(left_write.clone()),
        repository.reserve(right_write.clone()),
    );
    let (winner, winner_write, winner_bytes, quota_error) = match (left_result, right_result) {
        (Ok(winner), Err(RepositoryError::Conflict(message))) => {
            (winner, left_write, left_bytes, message)
        }
        (Err(RepositoryError::Conflict(message)), Ok(winner)) => {
            (winner, right_write, right_bytes, message)
        }
        (left, right) => {
            return Err(std::io::Error::other(format!(
                "concurrent UserFile reservations did not produce one winner and one quota conflict: left={left:?}, right={right:?}"
            ))
            .into())
        }
    };
    assert!(!winner.replayed);
    assert!(quota_error.contains("quota exceeded"));

    let stored = repository
        .find(organization_id, project_id, winner.value.id)
        .await?
        .ok_or_else(|| std::io::Error::other("winning UserFile reservation is missing"))?;
    assert_eq!(stored, winner.value);
    assert_eq!(
        repository.list(organization_id, project_id, 10).await?,
        vec![stored.clone()]
    );
    let quota = repository.quota(organization_id).await?;
    assert_eq!(quota.limit_bytes, TEST_QUOTA_BYTES as u64);
    assert_eq!(quota.allocated_bytes, RESERVATION_BYTES);
    assert_eq!(quota.revision, 1);
    assert_eq!(
        user_file_side_effect_counts(&database, organization_id, &scope).await?,
        (1, 1, 1)
    );

    let duplicate_content = UserFileContentReference::new(
        organization_id,
        other_project_id,
        stored.id,
        UserFileUploadId::new(),
        Sha256Digest::from_bytes(b"duplicate cross-project identity"),
        512,
        "application/octet-stream",
    )?;
    let duplicate = UserFile::reserve(
        UserFileAdmissionContract::from_spec(UserFileAdmissionContractSpec {
            original_name: "duplicate.bin".into(),
            upload_expires_at: created_at + chrono::Duration::minutes(15),
            retention_until: created_at + chrono::Duration::days(1),
            scan_policy: UserFileScanPolicy::Required,
            content: duplicate_content,
        })?,
        actor_principal_id,
        created_at + chrono::Duration::seconds(3),
    )?;
    let duplicate_scope =
        format!("organizations/{organization_id}/projects/{other_project_id}/user-files");
    assert!(matches!(
        repository
            .reserve(reserve_write(
                duplicate,
                &duplicate_scope,
                "postgres:user-file:duplicate-identity",
                b"duplicate-identity",
            )?)
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .find(organization_id, other_project_id, stored.id)
            .await?,
        None
    );
    assert_eq!(
        repository.quota(organization_id).await?.allocated_bytes,
        RESERVATION_BYTES
    );
    assert_eq!(
        user_file_side_effect_counts(&database, organization_id, &scope).await?,
        (1, 1, 1)
    );

    let replay = repository.reserve(winner_write.clone()).await?;
    assert!(replay.replayed);
    assert_eq!(replay.value, stored);
    let conflicting_replay = IdempotencyRequest::new(
        winner_write.idempotency.scope.clone(),
        winner_write.idempotency.key.clone(),
        b"different reservation",
    )?;
    assert!(matches!(
        repository.replay_write(&conflicting_replay).await,
        Err(RepositoryError::IdempotencyConflict)
    ));
    assert_eq!(
        user_file_side_effect_counts(&database, organization_id, &scope).await?,
        (1, 1, 1)
    );

    let object_directory = tempfile::tempdir()?;
    let object_store = SharedUserFileObjectStore::local(object_directory.path())?;
    let service =
        UserFileApplicationService::new(Arc::new(repository.clone()), Arc::new(object_store));

    let upload_request_id = Uuid::now_v7();
    let upload_key = "postgres:user-file:upload";
    let uploaded = service
        .record_upload(RecordUserFileUpload {
            transition: UserFileTransition {
                organization_id,
                project_id,
                user_file_id: stored.id,
                expected_version: stored.aggregate_version,
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: upload_key.into(),
                request_id: upload_request_id,
            },
            reader: Box::pin(Cursor::new(winner_bytes)),
        })
        .await?;
    assert!(!uploaded.replayed);
    assert_eq!(uploaded.file.state, UserFileState::AwaitingScan);
    assert_eq!(uploaded.file.aggregate_version, 2);
    assert_eq!(
        repository.quota(organization_id).await?.allocated_bytes,
        RESERVATION_BYTES
    );
    let upload_replay = service
        .record_upload(RecordUserFileUpload {
            transition: UserFileTransition {
                organization_id,
                project_id,
                user_file_id: stored.id,
                expected_version: stored.aggregate_version,
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: upload_key.into(),
                request_id: upload_request_id,
            },
            // Replay must resolve from metadata before consuming another byte.
            reader: Box::pin(Cursor::new(Vec::<u8>::new())),
        })
        .await?;
    assert!(upload_replay.replayed);
    assert_eq!(upload_replay.file, uploaded.file);
    assert_eq!(
        user_file_side_effect_counts(&database, organization_id, &scope).await?,
        (2, 2, 2)
    );

    let upload_payload = database
        .fetch_one_as(
            sql_query::<serde_json::Value>(
                "select payload from outbox_events where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(stored.id.as_uuid())
            .append(" and aggregate_version = 2"),
        )
        .await?;
    assert_eq!(
        upload_payload["cleanupDueAt"],
        serde_json::json!(uploaded.file.cleanup_due_at())
    );

    let scan_request_id = Uuid::now_v7();
    let scan_key = "postgres:user-file:scan";
    let evidence_digest = Sha256Digest::from_bytes(b"postgres scanner evidence");
    let admitted = service
        .record_scan(RecordUserFileScan {
            transition: UserFileTransition {
                organization_id,
                project_id,
                user_file_id: stored.id,
                expected_version: uploaded.file.aggregate_version,
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: scan_key.into(),
                request_id: scan_request_id,
            },
            evidence_digest: evidence_digest.as_str().into(),
            decision: UserFileScanDecision::Admitted,
        })
        .await?;
    assert!(!admitted.replayed);
    assert_eq!(admitted.file.state, UserFileState::Admitted);
    assert_eq!(admitted.file.aggregate_version, 3);
    let scan_replay = service
        .record_scan(RecordUserFileScan {
            transition: UserFileTransition {
                organization_id,
                project_id,
                user_file_id: stored.id,
                expected_version: uploaded.file.aggregate_version,
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: scan_key.into(),
                request_id: scan_request_id,
            },
            evidence_digest: evidence_digest.as_str().into(),
            decision: UserFileScanDecision::Admitted,
        })
        .await?;
    assert!(scan_replay.replayed);
    assert_eq!(scan_replay.file, admitted.file);

    let tombstone_scope = format!(
        "organizations/{organization_id}/projects/{project_id}/user-files/{}/tombstone",
        stored.id
    );
    let tombstone_request_id = Uuid::now_v7();
    let tombstone_key = "postgres:user-file:tombstone";
    let transitioned = service
        .tombstone(UserFileTransition {
            organization_id,
            project_id,
            user_file_id: stored.id,
            expected_version: admitted.file.aggregate_version,
            actor_principal_id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            idempotency_key: tombstone_key.into(),
            request_id: tombstone_request_id,
        })
        .await?;
    assert!(!transitioned.replayed);
    assert_eq!(transitioned.file.state, UserFileState::Tombstoned);
    assert_eq!(transitioned.file.aggregate_version, 4);
    assert_eq!(
        transitioned.file.tombstoned_from,
        Some(UserFileState::Admitted)
    );
    let tombstoned = transitioned.file.clone();
    let transition_replay = service
        .tombstone(UserFileTransition {
            organization_id,
            project_id,
            user_file_id: stored.id,
            expected_version: admitted.file.aggregate_version,
            actor_principal_id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            idempotency_key: tombstone_key.into(),
            request_id: tombstone_request_id,
        })
        .await?;
    assert!(transition_replay.replayed);
    assert_eq!(transition_replay.file, tombstoned);

    // A different key cannot reuse the stale version even when its requested
    // successor is otherwise structurally valid.
    assert!(matches!(
        service
            .tombstone(UserFileTransition {
                organization_id,
                project_id,
                user_file_id: stored.id,
                expected_version: admitted.file.aggregate_version,
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "postgres:user-file:stale-tombstone".into(),
                request_id: Uuid::now_v7(),
            })
            .await,
        Err(ApplicationError::Conflict(_))
    ));

    let quota = repository.quota(organization_id).await?;
    assert_eq!(quota.allocated_bytes, 0);
    assert_eq!(quota.revision, 2);
    assert_eq!(
        repository
            .find(organization_id, project_id, stored.id)
            .await?,
        Some(tombstoned.clone())
    );
    assert_eq!(
        user_file_side_effect_counts(&database, organization_id, &scope).await?,
        (4, 4, 4)
    );
    let tombstone_payload = database
        .fetch_one_as(
            sql_query::<serde_json::Value>(
                "select payload from outbox_events where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(stored.id.as_uuid())
            .append(" and aggregate_version = 4"),
        )
        .await?;
    assert_eq!(
        tombstone_payload["cleanupDueAt"],
        serde_json::json!(tombstoned.cleanup_due_at())
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from idempotency_records where scope_key = ",)
                    .bind(tombstone_scope.as_str()),
            )
            .await?,
        1
    );

    Ok(())
}

fn user_file(
    organization_id: OrganizationId,
    project_id: ProjectId,
    actor_principal_id: PrincipalId,
    original_name: &str,
    size_bytes: u64,
    digest_input: &[u8],
    created_at: chrono::DateTime<Utc>,
) -> Result<UserFile, String> {
    let content = UserFileContentReference::new(
        organization_id,
        project_id,
        UserFileId::new(),
        UserFileUploadId::new(),
        Sha256Digest::from_bytes(digest_input),
        size_bytes,
        "application/octet-stream",
    )?;
    let contract = UserFileAdmissionContract::from_spec(UserFileAdmissionContractSpec {
        original_name: original_name.into(),
        upload_expires_at: created_at + chrono::Duration::minutes(15),
        retention_until: created_at + chrono::Duration::days(1),
        scan_policy: UserFileScanPolicy::Required,
        content,
    })?;
    UserFile::reserve(contract, actor_principal_id, created_at)
}

fn reserve_write(
    file: UserFile,
    scope: &str,
    key: &str,
    canonical_request: &[u8],
) -> Result<ReserveUserFileWrite, String> {
    let request_id = Uuid::now_v7();
    Ok(ReserveUserFileWrite {
        event: UserFileLifecycleChanged::changed(&file, request_id, None)?,
        actor_principal_id: file.created_by,
        request_id,
        idempotency: IdempotencyRequest::new(scope, key, canonical_request)?,
        file,
    })
}

async fn user_file_side_effect_counts(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    reservation_scope: &str,
) -> Result<(i64, i64, i64), DatabaseError<PostgresError>> {
    database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>(
                "select (select count(*) from outbox_events where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and event_key = 'user-file.lifecycle.changed'), (select count(*) from audit_records where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and action like 'file.user-file.%'), (select count(*) from idempotency_records where scope_key = ")
            .bind(reservation_scope)
            .append(" or scope_key like ")
            .bind(format!("{reservation_scope}/%"))
            .append(")"),
        )
        .await
}
