use super::*;
use a3s_cloud_control_plane::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRbacBootstrap, PlatformRoleBinding,
    TenantSupportGrantProposal,
};
use a3s_cloud_control_plane::modules::identity::domain::repositories::{
    ApproveTenantSupportGrantWrite, BootstrapPlatformRbacWrite, CreatePlatformRoleBindingWrite,
    IPlatformRbacRepository, ITenantSupportGrantRepository, ProposeTenantSupportGrantWrite,
    RevokeTenantSupportGrantWrite,
};
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    ApiTokenScope, PlatformRole, PlatformRolePolicyContract, TenantNotificationRequirement,
    TenantSupportApprovalRequirement, TenantSupportGrantContract, TenantSupportGrantContractSpec,
    TenantSupportGrantMode, TenantSupportPermission,
};
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ApiTokenId, PlatformRoleBindingId, PlatformRolePolicyId, PrincipalId, Sha256Digest,
    TenantSupportGrantId,
};
use chrono::Duration as ChronoDuration;

const IDEMPOTENCY_SCOPE: &str = "tests/tenant-support-grants";

pub async fn exercise_tenant_support_grant_authority(
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
    let organization_id = OrganizationId::new();
    let requester = PrincipalId::new();
    let approver_a = PrincipalId::new();
    let approver_b = PrincipalId::new();
    let subject = PrincipalId::new();
    let outsider = PrincipalId::new();
    let now = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .ok_or("tenant-support test timestamp exceeds PostgreSQL precision")?;

    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'MT2 support tenant', ")
            .bind(format!("mt2-support-{organization_id}"))
            .append(", 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    for (principal_id, name) in [
        (requester, "requester"),
        (approver_a, "approver-a"),
        (approver_b, "approver-b"),
        (subject, "subject"),
        (outsider, "outsider"),
    ] {
        database
            .execute(
                sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                    .bind(principal_id.as_uuid())
                    .append(", 'human', ")
                    .bind(format!("MT2 support {name}"))
                    .append(", 1, ")
                    .bind(now)
                    .append(", null)"),
            )
            .await?;
    }

    let repository_a = PostgresIdentityRepository::new(executor.clone());
    let repository_b = PostgresIdentityRepository::new(executor.clone());
    let policy = AcceptedPlatformRolePolicyRevision::accept(
        PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())?,
        1,
        requester,
        now - ChronoDuration::minutes(5),
    )?;
    let requester_binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        requester,
        PlatformRole::PlatformOwner,
        &policy,
        requester,
        policy.accepted_at,
    )?;
    repository_a
        .bootstrap_platform_rbac(BootstrapPlatformRbacWrite {
            bootstrap: PlatformRbacBootstrap {
                policy: policy.clone(),
                owner_binding: requester_binding,
            },
            actor_principal_id: requester,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("bootstrap")?,
        })
        .await?;

    let requester_credential = test_api_token(
        organization_id,
        requester,
        "support requester",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::minutes(4),
        None,
    )?;
    let approver_a_credential = test_api_token(
        organization_id,
        approver_a,
        "support approver a",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::minutes(4),
        None,
    )?;
    let approver_b_credential = test_api_token(
        organization_id,
        approver_b,
        "support approver b",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::minutes(4),
        None,
    )?;
    let outsider_credential = test_api_token(
        organization_id,
        outsider,
        "support outsider",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::minutes(4),
        None,
    )?;
    for (seed, credential) in [
        &requester_credential,
        &approver_a_credential,
        &approver_b_credential,
        &outsider_credential,
    ]
    .into_iter()
    .enumerate()
    {
        persist_test_api_token(&database, credential, 300 + seed).await?;
    }

    create_binding(
        &repository_a,
        installation_id,
        &policy,
        approver_a,
        PlatformRole::PlatformAdmin,
        requester,
        requester_credential.id,
        "bind-approver-a",
    )
    .await?;
    create_binding(
        &repository_a,
        installation_id,
        &policy,
        approver_b,
        PlatformRole::PlatformAdmin,
        requester,
        requester_credential.id,
        "bind-approver-b",
    )
    .await?;
    let outsider_binding = create_binding(
        &repository_a,
        installation_id,
        &policy,
        outsider,
        PlatformRole::PlatformOperator,
        requester,
        requester_credential.id,
        "bind-outsider",
    )
    .await?;

    let contract = support_contract(
        installation_id,
        organization_id,
        subject,
        approver_a,
        approver_b,
        "INC-MT2-C2-1",
        'a',
        now,
    )?;
    let proposal_write = ProposeTenantSupportGrantWrite {
        contract: contract.clone(),
        actor_principal_id: requester,
        credential_id: requester_credential.id,
        requested_at: now,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("propose-main")?,
    };
    let proposed = repository_a
        .propose_tenant_support_grant(proposal_write.clone())
        .await?;
    assert!(!proposed.replayed);
    let proposal = proposed.value;
    assert_eq!(proposal.contract, contract);
    proposal.validate()?;
    assert!(
        repository_b
            .propose_tenant_support_grant(proposal_write)
            .await?
            .replayed
    );
    assert!(repository_a
        .find_tenant_support_grant(installation_id, proposal.id)
        .await?
        .is_none());

    assert!(
        database
            .execute(
                sql_query::<()>("insert into tenant_support_grants (id, aggregate_version, revocation_generation, accepted_at, revoked_at, revoked_by) values (")
                    .bind(proposal.id.as_uuid())
                    .append(", 1, 0, ")
                    .bind(now)
                    .append(", null, null)"),
            )
            .await
            .is_err(),
        "declared approver IDs alone must never activate a grant"
    );
    assert!(
        database
            .execute(
                sql_query::<()>("insert into tenant_support_grant_approvals (grant_id, approver_id, contract_digest, authentication_id, authentication_digest, policy_revision_id, policy_digest, binding_id, binding_version, approved_at, evidence_digest) values (")
                    .bind(proposal.id.as_uuid())
                    .append(", ")
                    .bind(approver_a.as_uuid())
                    .append(", ")
                    .bind(proposal.contract.digest().as_str())
                    .append(", 'urn:a3s:test:forged-authentication', ")
                    .bind(digest('b')?.as_str().to_owned())
                    .append(", ")
                    .bind(policy.id.as_uuid())
                    .append(", ")
                    .bind(policy.contract.digest().as_str())
                    .append(", ")
                    .bind(outsider_binding.id.as_uuid())
                    .append(", ")
                    .bind(outsider_binding.aggregate_version)
                    .append(", ")
                    .bind(now + ChronoDuration::seconds(1))
                    .append(", ")
                    .bind(digest('c')?.as_str().to_owned())
                    .append(")"),
            )
            .await
            .is_err(),
        "an approval row must bind the same active human, policy, and role binding"
    );

    let outsider_write = ApproveTenantSupportGrantWrite {
        installation_id,
        grant_id: proposal.id,
        expected_contract_digest: proposal.contract.digest().clone(),
        actor_principal_id: outsider,
        credential_id: outsider_credential.id,
        approved_at: now + ChronoDuration::seconds(1),
        request_id: Uuid::now_v7(),
        idempotency: idempotency("outsider-approval")?,
    };
    assert!(matches!(
        repository_a
            .approve_tenant_support_grant(outsider_write)
            .await,
        Err(RepositoryError::Forbidden(_))
    ));

    let approval_a = approval_write(
        installation_id,
        &proposal,
        approver_a,
        approver_a_credential.id,
        "approver-a-main",
        now + ChronoDuration::seconds(2),
    )?;
    let approval_b = approval_write(
        installation_id,
        &proposal,
        approver_b,
        approver_b_credential.id,
        "approver-b-main",
        now + ChronoDuration::seconds(3),
    )?;
    let (approved_a, approved_b) = tokio::join!(
        repository_a.approve_tenant_support_grant(approval_a.clone()),
        repository_b.approve_tenant_support_grant(approval_b.clone())
    );
    let approved_a = approved_a?;
    let approved_b = approved_b?;
    assert_eq!(
        usize::from(approved_a.value.grant.is_some())
            + usize::from(approved_b.value.grant.is_some()),
        1,
        "exactly the threshold-crossing transaction must activate the grant"
    );
    let accepted = repository_a
        .find_tenant_support_grant(installation_id, proposal.id)
        .await?
        .ok_or("dual approval did not activate the tenant support grant")?;
    assert_eq!(accepted.accepted_at, now + ChronoDuration::seconds(3));
    assert_eq!(
        repository_b
            .list_tenant_support_grant_approvals(installation_id, proposal.id)
            .await?
            .len(),
        2
    );
    assert!(
        repository_a
            .approve_tenant_support_grant(approval_a)
            .await?
            .replayed
    );
    assert!(
        repository_b
            .approve_tenant_support_grant(approval_b)
            .await?
            .replayed
    );

    let revoke_write = RevokeTenantSupportGrantWrite {
        installation_id,
        grant_id: proposal.id,
        expected_version: 1,
        actor_principal_id: requester,
        credential_id: requester_credential.id,
        revoked_at: now + ChronoDuration::seconds(4),
        request_id: Uuid::now_v7(),
        idempotency: idempotency("revoke-main")?,
    };
    let revoked = repository_a
        .revoke_tenant_support_grant(revoke_write.clone())
        .await?;
    assert_eq!(revoked.value.aggregate_version, 2);
    assert!(
        repository_b
            .revoke_tenant_support_grant(revoke_write)
            .await?
            .replayed
    );
    assert!(
        database
            .execute(
                sql_query::<()>("update tenant_support_grants set aggregate_version = 2, revocation_generation = 1, revoked_at = ")
                    .bind(now + ChronoDuration::seconds(5))
                    .append(", revoked_by = ")
                    .bind(requester.as_uuid())
                    .append(" where id = ")
                    .bind(proposal.id.as_uuid()),
            )
            .await
            .is_err(),
        "revocation must be terminal"
    );
    assert!(
        database
            .execute(
                sql_query::<()>("delete from tenant_support_grant_approvals where grant_id = ")
                    .bind(proposal.id.as_uuid())
                    .append(" and approver_id = ")
                    .bind(approver_a.as_uuid()),
            )
            .await
            .is_err(),
        "actual approval evidence must be immutable"
    );

    let blocked_contract = support_contract(
        installation_id,
        organization_id,
        subject,
        approver_a,
        approver_b,
        "INC-MT2-C2-2",
        'b',
        now + ChronoDuration::seconds(10),
    )?;
    let blocked = repository_a
        .propose_tenant_support_grant(ProposeTenantSupportGrantWrite {
            contract: blocked_contract,
            actor_principal_id: requester,
            credential_id: requester_credential.id,
            requested_at: now + ChronoDuration::seconds(10),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("propose-blocked")?,
        })
        .await?
        .value;
    repository_a
        .approve_tenant_support_grant(approval_write(
            installation_id,
            &blocked,
            approver_a,
            approver_a_credential.id,
            "approver-a-blocked",
            now + ChronoDuration::seconds(11),
        )?)
        .await?;
    database
        .execute(
            sql_query::<()>("update identity_principals set disabled_at = ")
                .bind(now + ChronoDuration::seconds(12))
                .append(" where id = ")
                .bind(approver_b.as_uuid()),
        )
        .await?;
    assert!(matches!(
        repository_b
            .approve_tenant_support_grant(approval_write(
                installation_id,
                &blocked,
                approver_b,
                approver_b_credential.id,
                "approver-b-blocked",
                now + ChronoDuration::seconds(13),
            )?)
            .await,
        Err(RepositoryError::Forbidden(_))
    ));
    assert!(repository_a
        .find_tenant_support_grant(installation_id, blocked.id)
        .await?
        .is_none());
    assert_eq!(
        repository_a
            .list_tenant_support_grant_approvals(installation_id, blocked.id)
            .await?
            .len(),
        1,
        "failed threshold revalidation must roll back the final approval"
    );

    assert!(
        database
            .execute(
                sql_query::<()>(
                    "update tenant_support_grant_intents set digest = digest where id = "
                )
                .bind(proposal.id.as_uuid()),
            )
            .await
            .is_err(),
        "support intent history must be immutable"
    );
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from tenant_support_grant_intents), (select count(*) from tenant_support_grant_approvals), (select count(*) from tenant_support_grants), (select count(*) from outbox_events where event_key like 'identity.tenant-support-grant.%'), (select count(*) from audit_records where action like 'identity.tenant-support-grant.%'), (select count(*) from idempotency_records where scope_key = 'tests/tenant-support-grants' and idempotency_key in ('propose-main', 'approver-a-main', 'approver-b-main', 'revoke-main', 'propose-blocked', 'approver-a-blocked'))",
            ),
        )
        .await?;
    assert_eq!(
        evidence,
        (2, 3, 1, 7, 7, 6),
        "intent, actual approvals, accepted grant, facts, and replay records must commit exactly once"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn support_contract(
    installation_id: InstallationId,
    organization_id: OrganizationId,
    subject: PrincipalId,
    approver_a: PrincipalId,
    approver_b: PrincipalId,
    case_reference: &str,
    digest_byte: char,
    requested_at: chrono::DateTime<Utc>,
) -> Result<TenantSupportGrantContract, Box<dyn std::error::Error>> {
    Ok(TenantSupportGrantContract::from_spec(
        TenantSupportGrantContractSpec {
            grant_id: TenantSupportGrantId::new(),
            principal_id: subject,
            scope: ScopeContext::organization(installation_id, organization_id)?,
            permissions: vec![
                TenantSupportPermission::HealthRead,
                TenantSupportPermission::ResourceMetadataRead,
            ],
            case_reference: case_reference.into(),
            justification_digest: digest(digest_byte)?,
            mode: TenantSupportGrantMode::Standard,
            approval_requirement: TenantSupportApprovalRequirement::Dual,
            approver_ids: vec![approver_a, approver_b],
            tenant_notification: TenantNotificationRequirement::Required,
            security_alert_required: false,
            post_incident_review_required: false,
            starts_at: requested_at - ChronoDuration::minutes(1),
            expires_at: requested_at + ChronoDuration::hours(1),
        },
    )?)
}

fn approval_write(
    installation_id: InstallationId,
    proposal: &TenantSupportGrantProposal,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    key: &str,
    approved_at: chrono::DateTime<Utc>,
) -> Result<ApproveTenantSupportGrantWrite, Box<dyn std::error::Error>> {
    Ok(ApproveTenantSupportGrantWrite {
        installation_id,
        grant_id: proposal.id,
        expected_contract_digest: proposal.contract.digest().clone(),
        actor_principal_id,
        credential_id,
        approved_at,
        request_id: Uuid::now_v7(),
        idempotency: idempotency(key)?,
    })
}

#[allow(clippy::too_many_arguments)]
async fn create_binding(
    repository: &PostgresIdentityRepository,
    installation_id: InstallationId,
    policy: &AcceptedPlatformRolePolicyRevision,
    principal_id: PrincipalId,
    role: PlatformRole,
    actor: PrincipalId,
    actor_credential_id: ApiTokenId,
    key: &str,
) -> Result<PlatformRoleBinding, Box<dyn std::error::Error>> {
    let binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        principal_id,
        role,
        policy,
        actor,
        Utc::now(),
    )?;
    repository
        .create_platform_role_binding(CreatePlatformRoleBindingWrite {
            binding: binding.clone(),
            expected_policy_revision_id: policy.id,
            actor_principal_id: actor,
            credential_id: actor_credential_id,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(key)?,
        })
        .await?;
    Ok(binding)
}

fn digest(byte: char) -> Result<Sha256Digest, Box<dyn std::error::Error>> {
    Ok(Sha256Digest::parse(format!(
        "sha256:{}",
        byte.to_string().repeat(64)
    ))?)
}

fn idempotency(key: &str) -> Result<IdempotencyRequest, Box<dyn std::error::Error>> {
    Ok(IdempotencyRequest::new(
        IDEMPOTENCY_SCOPE,
        key,
        &serde_json::to_vec(&json!({"key": key}))?,
    )?)
}
