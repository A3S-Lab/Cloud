use super::*;
use a3s_cloud_control_plane::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRoleBinding,
};
use a3s_cloud_control_plane::modules::identity::domain::repositories::{
    AcceptPlatformRolePolicyRevisionWrite, BootstrapPlatformRbacWrite,
    ChangePlatformRoleBindingWrite, CreatePlatformRoleBindingWrite, IPlatformRbacRepository,
    PlatformRbacBootstrap, RevokePlatformRoleBindingWrite,
};
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    PlatformPermission, PlatformRole, PlatformRolePolicyContract, PlatformRolePolicySpec,
};
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    PlatformRoleBindingId, PlatformRolePolicyId, PrincipalId,
};
use chrono::Duration as ChronoDuration;

pub async fn exercise_platform_rbac_authority(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&postgres_url, 12).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let installation_id = InstallationId::from_uuid(
        database
            .fetch_one_as(sql_query::<Uuid>(
                "select id from cloud_installations where singleton_key",
            ))
            .await?,
    );
    let owner_a = PrincipalId::new();
    let owner_b = PrincipalId::new();
    let admin = PrincipalId::new();
    let operator = PrincipalId::new();
    let spare = PrincipalId::new();
    for (principal, suffix) in [
        (owner_a, "owner-a"),
        (owner_b, "owner-b"),
        (admin, "admin"),
        (operator, "operator"),
        (spare, "spare"),
    ] {
        database
            .execute(
                sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                    .bind(principal.as_uuid())
                    .append(", 'human', ")
                    .bind(format!("MT2 {suffix}"))
                    .append(", 1, ")
                    .bind(Utc::now())
                    .append(", null)"),
            )
            .await?;
    }

    let repository_a = PostgresIdentityRepository::new(executor.clone());
    let repository_b = PostgresIdentityRepository::new(executor.clone());
    let candidate_a = bootstrap_write(installation_id, owner_a, "mt2:bootstrap-a")?;
    let candidate_b = bootstrap_write(installation_id, owner_b, "mt2:bootstrap-b")?;
    let (bootstrap_a, bootstrap_b) = tokio::join!(
        repository_a.bootstrap_platform_rbac(candidate_a.clone()),
        repository_b.bootstrap_platform_rbac(candidate_b.clone())
    );
    let (winning_repository, winning_write, initial_owner, second_owner, initial_policy) =
        match (bootstrap_a, bootstrap_b) {
            (Ok(winner), Err(RepositoryError::Forbidden(_))) => (
                &repository_a,
                candidate_a,
                owner_a,
                owner_b,
                winner.value.policy,
            ),
            (Err(RepositoryError::Forbidden(_)), Ok(winner)) => (
                &repository_b,
                candidate_b,
                owner_b,
                owner_a,
                winner.value.policy,
            ),
            results => {
                return Err(format!(
                "concurrent platform RBAC bootstrap did not elect exactly one owner: {results:?}"
            )
                .into())
            }
        };
    let bootstrap_replay = winning_repository
        .bootstrap_platform_rbac(winning_write)
        .await?;
    assert!(bootstrap_replay.replayed);
    assert_eq!(bootstrap_replay.value.policy, initial_policy);

    let current_policy = winning_repository
        .current_platform_role_policy(installation_id)
        .await?
        .ok_or("platform RBAC bootstrap has no current policy")?;
    assert_eq!(current_policy, initial_policy);
    assert_eq!(
        winning_repository
            .find_platform_role_policy_revision(installation_id, initial_policy.id)
            .await?,
        Some(initial_policy.clone())
    );

    let second_owner_binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        second_owner,
        PlatformRole::PlatformOwner,
        &current_policy,
        initial_owner,
        Utc::now(),
    )?;
    winning_repository
        .create_platform_role_binding(CreatePlatformRoleBindingWrite {
            binding: second_owner_binding.clone(),
            actor_principal_id: initial_owner,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("platform-role-bindings", "mt2:create-second-owner")?,
        })
        .await?;

    let admin_binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        admin,
        PlatformRole::PlatformAdmin,
        &current_policy,
        initial_owner,
        Utc::now(),
    )?;
    winning_repository
        .create_platform_role_binding(CreatePlatformRoleBindingWrite {
            binding: admin_binding.clone(),
            actor_principal_id: initial_owner,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("platform-role-bindings", "mt2:create-admin")?,
        })
        .await?;

    let forged_owner = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        spare,
        PlatformRole::PlatformOwner,
        &current_policy,
        admin,
        Utc::now(),
    )?;
    assert!(matches!(
        repository_b
            .create_platform_role_binding(CreatePlatformRoleBindingWrite {
                binding: forged_owner,
                actor_principal_id: admin,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("platform-role-bindings", "mt2:admin-create-owner")?,
            })
            .await,
        Err(RepositoryError::Forbidden(_))
    ));
    assert!(matches!(
        repository_b
            .change_platform_role_binding(ChangePlatformRoleBindingWrite {
                installation_id,
                binding_id: admin_binding.id,
                expected_version: 1,
                role: PlatformRole::PlatformOwner,
                actor_principal_id: admin,
                changed_at: Utc::now(),
                request_id: Uuid::now_v7(),
                idempotency: idempotency("platform-role-bindings", "mt2:self-escalation")?,
            })
            .await,
        Err(RepositoryError::Forbidden(_))
    ));

    let operator_binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        operator,
        PlatformRole::PlatformOperator,
        &current_policy,
        admin,
        Utc::now(),
    )?;
    repository_b
        .create_platform_role_binding(CreatePlatformRoleBindingWrite {
            binding: operator_binding.clone(),
            actor_principal_id: admin,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("platform-role-bindings", "mt2:create-operator")?,
        })
        .await?;
    assert!(matches!(
        repository_a
            .change_platform_role_binding(ChangePlatformRoleBindingWrite {
                installation_id,
                binding_id: operator_binding.id,
                expected_version: 1,
                role: PlatformRole::PlatformAdmin,
                actor_principal_id: operator,
                changed_at: Utc::now(),
                request_id: Uuid::now_v7(),
                idempotency: idempotency("platform-role-bindings", "mt2:operator-escalation")?,
            })
            .await,
        Err(RepositoryError::Forbidden(_))
    ));

    let initial_owner_binding = winning_repository
        .find_active_platform_role_binding_for_principal(installation_id, initial_owner)
        .await?
        .ok_or("initial owner binding disappeared")?;
    let revoke_initial = RevokePlatformRoleBindingWrite {
        installation_id,
        binding_id: initial_owner_binding.id,
        expected_version: 1,
        actor_principal_id: initial_owner,
        revoked_at: Utc::now() + ChronoDuration::seconds(1),
        request_id: Uuid::now_v7(),
        idempotency: idempotency("platform-role-bindings", "mt2:revoke-owner-a")?,
    };
    let revoke_second = RevokePlatformRoleBindingWrite {
        installation_id,
        binding_id: second_owner_binding.id,
        expected_version: 1,
        actor_principal_id: second_owner,
        revoked_at: Utc::now() + ChronoDuration::seconds(1),
        request_id: Uuid::now_v7(),
        idempotency: idempotency("platform-role-bindings", "mt2:revoke-owner-b")?,
    };
    let (revoked_initial, revoked_second) = tokio::join!(
        repository_a.revoke_platform_role_binding(revoke_initial),
        repository_b.revoke_platform_role_binding(revoke_second)
    );
    let remaining_owner = match (revoked_initial, revoked_second) {
        (Ok(_), Err(RepositoryError::Conflict(_))) => second_owner,
        (Err(RepositoryError::Conflict(_)), Ok(_)) => initial_owner,
        results => {
            return Err(format!(
                "concurrent owner revocation did not preserve exactly one owner: {results:?}"
            )
            .into())
        }
    };

    let current = repository_a
        .current_platform_role_policy(installation_id)
        .await?
        .ok_or("current policy disappeared")?;
    let policy_a = successor_policy(
        &current,
        remaining_owner,
        PlatformRole::PlatformAdmin,
        PlatformPermission::CapacityManage,
    )?;
    let policy_b = successor_policy(
        &current,
        remaining_owner,
        PlatformRole::PlatformOperator,
        PlatformPermission::OperationsExecute,
    )?;
    let write_a = AcceptPlatformRolePolicyRevisionWrite {
        revision: policy_a.clone(),
        expected_current_revision_id: current.id,
        actor_principal_id: remaining_owner,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("platform-role-policy", "mt2:policy-a")?,
    };
    let write_b = AcceptPlatformRolePolicyRevisionWrite {
        revision: policy_b.clone(),
        expected_current_revision_id: current.id,
        actor_principal_id: remaining_owner,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("platform-role-policy", "mt2:policy-b")?,
    };
    let (accepted_a, accepted_b) = tokio::join!(
        repository_a.accept_platform_role_policy_revision(write_a.clone()),
        repository_b.accept_platform_role_policy_revision(write_b.clone())
    );
    let (accepted, replay_write) = match (accepted_a, accepted_b) {
        (Ok(value), Err(RepositoryError::Conflict(_))) => (value.value, write_a),
        (Err(RepositoryError::Conflict(_)), Ok(value)) => (value.value, write_b),
        results => {
            return Err(format!(
                "concurrent policy CAS did not accept exactly one successor: {results:?}"
            )
            .into())
        }
    };
    assert_eq!(accepted.revision_number, 2);
    assert_eq!(
        repository_a
            .current_platform_role_policy(installation_id)
            .await?,
        Some(accepted.clone())
    );
    assert!(
        repository_b
            .accept_platform_role_policy_revision(replay_write)
            .await?
            .replayed
    );

    let remaining_binding = repository_a
        .find_active_platform_role_binding_for_principal(installation_id, remaining_owner)
        .await?
        .ok_or("remaining owner binding disappeared")?;
    assert_eq!(remaining_binding.role, PlatformRole::PlatformOwner);
    assert!(repository_a
        .find_active_platform_role_binding_for_principal(
            installation_id,
            if remaining_owner == initial_owner {
                second_owner
            } else {
                initial_owner
            },
        )
        .await?
        .is_none());

    let direct_last_owner_revocation = database
        .execute(
            sql_query::<()>("update platform_role_bindings set aggregate_version = aggregate_version + 1, updated_by = principal_id, updated_at = updated_at + interval '1 second', revoked_at = updated_at + interval '1 second' where id = ")
                .bind(remaining_binding.id.as_uuid()),
        )
        .await;
    assert!(
        direct_last_owner_revocation.is_err(),
        "database trigger must reject last-owner bypasses"
    );
    let direct_owner_disable = database
        .execute(
            sql_query::<()>("update identity_principals set disabled_at = now() where id = ")
                .bind(remaining_owner.as_uuid()),
        )
        .await;
    assert!(
        direct_owner_disable.is_err(),
        "database trigger must reject disabling the last active owner"
    );
    assert!(
        database
            .execute(
                sql_query::<()>("delete from platform_role_policy_revisions where id = ")
                    .bind(initial_policy.id.as_uuid()),
            )
            .await
            .is_err(),
        "accepted policy history must be immutable"
    );
    assert!(
        database
            .execute(
                sql_query::<()>("delete from platform_role_bindings where id = ")
                    .bind(operator_binding.id.as_uuid()),
            )
            .await
            .is_err(),
        "binding history must be undeletable"
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from platform_role_policy_heads), (select count(*) from platform_role_policy_revisions), (select count(*) from platform_role_bindings), (select count(*) from platform_role_bindings binding join identity_principals principal on principal.id = binding.principal_id and principal.disabled_at is null where binding.role = 'platform_owner' and binding.revoked_at is null), (select count(*) from outbox_events where event_key like 'identity.platform-role-%' and scope_kind = 'installation' and organization_id is null), (select count(*) from audit_records where action like 'identity.platform-role-%' and scope_kind = 'installation' and organization_id is null), (select count(*) from idempotency_records where idempotency_key like 'mt2:%')",
            ),
        )
        .await?;
    assert_eq!(
        evidence,
        (1, 2, 4, 1, 7, 7, 6),
        "policy head/history, bindings, recovery owner, facts, and idempotency must commit exactly once"
    );
    Ok(())
}

fn bootstrap_write(
    installation_id: InstallationId,
    owner: PrincipalId,
    key: &str,
) -> Result<BootstrapPlatformRbacWrite, Box<dyn std::error::Error>> {
    let policy = AcceptedPlatformRolePolicyRevision::accept(
        PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())?,
        1,
        owner,
        Utc::now(),
    )?;
    let owner_binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        owner,
        PlatformRole::PlatformOwner,
        &policy,
        owner,
        policy.accepted_at,
    )?;
    Ok(BootstrapPlatformRbacWrite {
        bootstrap: PlatformRbacBootstrap {
            policy,
            owner_binding,
        },
        actor_principal_id: owner,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("platform-rbac-bootstrap", key)?,
    })
}

fn successor_policy(
    current: &AcceptedPlatformRolePolicyRevision,
    accepted_by: PrincipalId,
    role: PlatformRole,
    removed_permission: PlatformPermission,
) -> Result<AcceptedPlatformRolePolicyRevision, Box<dyn std::error::Error>> {
    let mut spec: PlatformRolePolicySpec = current.contract.spec().clone();
    spec.role_permissions
        .iter_mut()
        .find(|entry| entry.role == role)
        .ok_or("role is missing from current platform policy")?
        .permissions
        .retain(|permission| *permission != removed_permission);
    let contract = PlatformRolePolicyContract::from_spec(spec)?;
    Ok(AcceptedPlatformRolePolicyRevision::accept(
        contract,
        current.revision_number + 1,
        accepted_by,
        Utc::now() + ChronoDuration::seconds(2),
    )?)
}

fn idempotency(scope: &str, key: &str) -> Result<IdempotencyRequest, Box<dyn std::error::Error>> {
    Ok(IdempotencyRequest::new(
        format!("installation/{scope}"),
        key,
        &serde_json::to_vec(&json!({"scope": scope, "key": key}))?,
    )?)
}
