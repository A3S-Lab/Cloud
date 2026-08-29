use super::*;
use a3s_cloud_control_plane::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRbacBootstrap, PlatformRoleBinding,
    TenantSupportGrant,
};
use a3s_cloud_control_plane::modules::identity::domain::events::ApiTokenRevoked;
use a3s_cloud_control_plane::modules::identity::domain::repositories::{
    ApproveTenantSupportGrantWrite, BootstrapPlatformRbacWrite, CreatePlatformRoleBindingWrite,
    IApiTokenRepository, IOrganizationRepository, IPlatformRbacRepository,
    IPrivilegedAuthorizationDecisionRepository, ITenantSupportGrantRepository,
    ProposeTenantSupportGrantWrite, ReadOrganizationCatalog, RevokePlatformRoleBindingWrite,
    RevokeTenantSupportGrantWrite,
};
use a3s_cloud_control_plane::modules::identity::domain::services::{
    PrivilegedAuthorizationDecision, PrivilegedAuthorizationDecisionRequest,
};
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    ApiTokenScope, PlatformPermission, PlatformRole, PlatformRolePolicyContract,
    TenantNotificationRequirement, TenantSupportApprovalRequirement, TenantSupportGrantContract,
    TenantSupportGrantContractSpec, TenantSupportGrantMode, TenantSupportPermission,
};
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ApiTokenId, AuthorizationDecisionRef, PlatformRoleBindingId, PlatformRolePolicyId, PrincipalId,
    Sha256Digest, TenantSupportGrantId,
};
use chrono::Duration as ChronoDuration;

const IDEMPOTENCY_SCOPE: &str = "tests/privileged-authorization-decisions";
const DECISION_AUDIT_ACTION: &str = "identity.privileged-access.authorize";
const TEST_AUTHORIZED_ACTION: &str = "identity.privileged-access.test";
const ORGANIZATION_CATALOG_READ_ACTION: &str = "identity.organization-catalog.read";
const EXPECTED_PROTECTED_MUTATION_DECISIONS: i64 = 10;

pub async fn exercise_privileged_authorization_decision_authority(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&postgres_url, 16).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let installation_id = InstallationId::from_uuid(
        database
            .fetch_one_as(sql_query::<Uuid>(
                "select id from cloud_installations where singleton_key",
            ))
            .await?,
    );
    let organization_id = OrganizationId::new();
    let catalog_organization_id = OrganizationId::new();
    let owner = PrincipalId::new();
    let role_race_principal = PrincipalId::new();
    let token_race_principal = PrincipalId::new();
    let grant_race_principal = PrincipalId::new();
    let approver_a = PrincipalId::new();
    let approver_b = PrincipalId::new();
    let now = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .ok_or("privileged authorization test timestamp exceeds PostgreSQL precision")?;

    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'MT2 privileged authorization', ")
            .bind(format!("mt2-privileged-{organization_id}"))
            .append(", 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(catalog_organization_id.as_uuid())
            .append(", 'MT2 organization catalog', ")
            .bind(format!("mt2-catalog-{catalog_organization_id}"))
            .append(", 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    for (principal_id, name) in [
        (owner, "owner"),
        (role_race_principal, "role-race"),
        (token_race_principal, "token-race"),
        (grant_race_principal, "grant-race"),
        (approver_a, "approver-a"),
        (approver_b, "approver-b"),
    ] {
        database
            .execute(
                sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                    .bind(principal_id.as_uuid())
                    .append(", 'human', ")
                    .bind(format!("MT2 privileged {name}"))
                    .append(", 1, ")
                    .bind(now - ChronoDuration::hours(2))
                    .append(", null)"),
            )
            .await?;
    }

    let repository_a = PostgresIdentityRepository::new(executor.clone());
    let repository_b = PostgresIdentityRepository::new(executor.clone());
    let policy = AcceptedPlatformRolePolicyRevision::accept(
        PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())?,
        1,
        owner,
        now - ChronoDuration::hours(1),
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
    repository_a
        .bootstrap_platform_rbac(BootstrapPlatformRbacWrite {
            bootstrap: PlatformRbacBootstrap {
                policy: policy.clone(),
                owner_binding,
            },
            actor_principal_id: owner,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("bootstrap-rbac")?,
        })
        .await?;
    let owner_token = test_api_token(
        organization_id,
        owner,
        "platform owner",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    let approver_a_token = test_api_token(
        organization_id,
        approver_a,
        "support approver a",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    let approver_b_token = test_api_token(
        organization_id,
        approver_b,
        "support approver b",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    for (seed, credential) in [&owner_token, &approver_a_token, &approver_b_token]
        .into_iter()
        .enumerate()
    {
        persist_test_api_token(&database, credential, 200 + seed).await?;
    }
    let role_race_binding = create_binding(
        &repository_a,
        installation_id,
        &policy,
        role_race_principal,
        PlatformRole::PlatformOperator,
        owner,
        owner_token.id,
        "bind-role-race",
        now,
    )
    .await?;
    create_binding(
        &repository_a,
        installation_id,
        &policy,
        token_race_principal,
        PlatformRole::PlatformOperator,
        owner,
        owner_token.id,
        "bind-token-race",
        now,
    )
    .await?;
    create_binding(
        &repository_a,
        installation_id,
        &policy,
        grant_race_principal,
        PlatformRole::PlatformOperator,
        owner,
        owner_token.id,
        "bind-grant-race",
        now,
    )
    .await?;
    let approver_a_binding = create_binding(
        &repository_a,
        installation_id,
        &policy,
        approver_a,
        PlatformRole::PlatformAdmin,
        owner,
        owner_token.id,
        "bind-approver-a",
        now,
    )
    .await?;
    create_binding(
        &repository_a,
        installation_id,
        &policy,
        approver_b,
        PlatformRole::PlatformAdmin,
        owner,
        owner_token.id,
        "bind-approver-b",
        now,
    )
    .await?;

    let role_race_token = test_api_token(
        organization_id,
        role_race_principal,
        "role race",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    let token_race_token = test_api_token(
        organization_id,
        token_race_principal,
        "token race",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    let grant_race_token = test_api_token(
        organization_id,
        grant_race_principal,
        "grant race",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    let read_only_token = test_api_token(
        organization_id,
        token_race_principal,
        "read only",
        [ApiTokenScope::parse(ApiTokenScope::CLOUD_READ)?]
            .into_iter()
            .collect(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    let catalog_without_read_token = test_api_token(
        organization_id,
        token_race_principal,
        "catalog without read",
        [ApiTokenScope::parse(ApiTokenScope::PROJECT_WRITE)?]
            .into_iter()
            .collect(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    let expired_token = test_api_token(
        organization_id,
        token_race_principal,
        "expired",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(2),
        Some(now - ChronoDuration::minutes(1)),
    )?;
    let mut revoked_token = test_api_token(
        organization_id,
        token_race_principal,
        "revoked",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::hours(1),
        None,
    )?;
    assert!(revoked_token.revoke(now - ChronoDuration::minutes(1)));
    for (index, value) in [
        &role_race_token,
        &token_race_token,
        &grant_race_token,
        &read_only_token,
        &catalog_without_read_token,
        &expired_token,
        &revoked_token,
    ]
    .into_iter()
    .enumerate()
    {
        persist_test_api_token(&database, value, 210 + index).await?;
    }

    let grant = create_support_grant(
        &repository_a,
        installation_id,
        organization_id,
        grant_race_principal,
        owner,
        owner_token.id,
        approver_a,
        approver_a_token.id,
        approver_b,
        approver_b_token.id,
        now,
    )
    .await?;

    let platform_success_request = platform_request(
        installation_id,
        token_race_principal,
        token_race_token.id,
        PlatformPermission::OperationsExecute,
    )?;
    let platform_request_id = platform_success_request.request_id;
    let platform_reference = repository_a
        .authorize_privileged(platform_success_request)
        .await?;
    assert_persisted_decision(
        &database,
        platform_request_id,
        &platform_reference,
        token_race_token.id,
    )
    .await?;

    let support_success_request = support_request(
        installation_id,
        organization_id,
        grant_race_principal,
        grant_race_token.id,
        grant.id,
    )?;
    let support_request_id = support_success_request.request_id;
    let support_reference = repository_b
        .authorize_privileged(support_success_request)
        .await?;
    let support_decision = assert_persisted_decision(
        &database,
        support_request_id,
        &support_reference,
        grant_race_token.id,
    )
    .await?;
    assert_eq!(
        support_decision
            .support_grant
            .as_ref()
            .map(|value| value.grant_id),
        Some(grant.id)
    );

    for denied in [
        platform_request(
            installation_id,
            token_race_principal,
            read_only_token.id,
            PlatformPermission::OperationsExecute,
        )?,
        platform_request(
            installation_id,
            token_race_principal,
            expired_token.id,
            PlatformPermission::OperationsExecute,
        )?,
        platform_request(
            installation_id,
            token_race_principal,
            revoked_token.id,
            PlatformPermission::OperationsExecute,
        )?,
        platform_request(
            installation_id,
            token_race_principal,
            owner_token.id,
            PlatformPermission::OperationsExecute,
        )?,
    ] {
        let request_id = denied.request_id;
        assert!(matches!(
            repository_a.authorize_privileged(denied).await,
            Err(RepositoryError::Forbidden(_))
        ));
        assert_eq!(decision_audit_count(&database, Some(request_id)).await?, 0);
    }
    let wrong_grant = PrivilegedAuthorizationDecisionRequest {
        support_grant_id: Some(TenantSupportGrantId::new()),
        ..support_request(
            installation_id,
            organization_id,
            grant_race_principal,
            grant_race_token.id,
            grant.id,
        )?
    };
    let wrong_grant_request_id = wrong_grant.request_id;
    assert!(matches!(
        repository_a.authorize_privileged(wrong_grant).await,
        Err(RepositoryError::Forbidden(_))
    ));
    assert_eq!(
        decision_audit_count(&database, Some(wrong_grant_request_id)).await?,
        0
    );

    let mut successful_decisions = 2_i64;
    let role_race_request = platform_request(
        installation_id,
        role_race_principal,
        role_race_token.id,
        PlatformPermission::OperationsRead,
    )?;
    let role_race_request_id = role_race_request.request_id;
    let (role_decision, role_revocation) = tokio::join!(
        repository_a.authorize_privileged(role_race_request),
        repository_b.revoke_platform_role_binding(RevokePlatformRoleBindingWrite {
            installation_id,
            binding_id: role_race_binding.id,
            expected_version: role_race_binding.aggregate_version,
            actor_principal_id: owner,
            credential_id: owner_token.id,
            revoked_at: Utc::now(),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("revoke-role-race")?,
        })
    );
    role_revocation?;
    successful_decisions += record_serialized_outcome(
        &database,
        role_race_request_id,
        role_race_token.id,
        role_decision,
    )
    .await?;
    assert!(matches!(
        repository_a
            .authorize_privileged(platform_request(
                installation_id,
                role_race_principal,
                role_race_token.id,
                PlatformPermission::OperationsRead,
            )?)
            .await,
        Err(RepositoryError::Forbidden(_))
    ));

    let token_race_request = platform_request(
        installation_id,
        token_race_principal,
        token_race_token.id,
        PlatformPermission::OperationsExecute,
    )?;
    let token_race_request_id = token_race_request.request_id;
    let mut revoked_race_token = token_race_token.clone();
    assert!(revoked_race_token.revoke(Utc::now()));
    let token_revocation_event = ApiTokenRevoked::envelope(&revoked_race_token, Uuid::now_v7())?;
    let (token_decision, token_revocation) = tokio::join!(
        repository_a.authorize_privileged(token_race_request),
        repository_b.revoke(
            revoked_race_token,
            Some(token_revocation_event),
            idempotency("revoke-token-race")?,
        )
    );
    token_revocation?;
    successful_decisions += record_serialized_outcome(
        &database,
        token_race_request_id,
        token_race_token.id,
        token_decision,
    )
    .await?;
    assert!(matches!(
        repository_b
            .authorize_privileged(platform_request(
                installation_id,
                token_race_principal,
                token_race_token.id,
                PlatformPermission::OperationsExecute,
            )?)
            .await,
        Err(RepositoryError::Forbidden(_))
    ));

    let grant_race_request = support_request(
        installation_id,
        organization_id,
        grant_race_principal,
        grant_race_token.id,
        grant.id,
    )?;
    let grant_race_request_id = grant_race_request.request_id;
    let (grant_decision, grant_revocation) = tokio::join!(
        repository_a.authorize_privileged(grant_race_request),
        repository_b.revoke_tenant_support_grant(RevokeTenantSupportGrantWrite {
            installation_id,
            grant_id: grant.id,
            expected_version: grant.aggregate_version,
            actor_principal_id: owner,
            credential_id: owner_token.id,
            revoked_at: Utc::now(),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("revoke-grant-race")?,
        })
    );
    grant_revocation?;
    successful_decisions += record_serialized_outcome(
        &database,
        grant_race_request_id,
        grant_race_token.id,
        grant_decision,
    )
    .await?;
    assert!(matches!(
        repository_b
            .authorize_privileged(support_request(
                installation_id,
                organization_id,
                grant_race_principal,
                grant_race_token.id,
                grant.id,
            )?)
            .await,
        Err(RepositoryError::Forbidden(_))
    ));

    assert_eq!(
        authorized_action_decision_audit_count(&database, TEST_AUTHORIZED_ACTION).await?,
        successful_decisions,
        "every successful standalone allow and only a successful standalone allow must persist one shared Audit fact"
    );
    assert_eq!(
        decision_audit_count(&database, None).await? - successful_decisions,
        EXPECTED_PROTECTED_MUTATION_DECISIONS,
        "each protected RBAC/support mutation must persist one decision in its business transaction"
    );

    let catalog_request_id = Uuid::now_v7();
    let catalog = repository_a
        .list_visible(ReadOrganizationCatalog {
            installation_id,
            actor_principal_id: owner,
            credential_id: owner_token.id,
            request_id: catalog_request_id,
        })
        .await?;
    assert_eq!(
        catalog
            .iter()
            .map(|organization| organization.id)
            .collect::<std::collections::BTreeSet<_>>(),
        [organization_id, catalog_organization_id]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    let catalog_decision = serde_json::from_value::<PrivilegedAuthorizationDecision>(
        database
            .fetch_one_as(
                sql_query::<Value>(
                    "select details from audit_records where action = 'identity.privileged-access.authorize' and request_id = ",
                )
                .bind(catalog_request_id),
            )
            .await?,
    )?;
    catalog_decision.validate()?;
    assert_eq!(catalog_decision.credential.id, owner_token.id);
    assert_eq!(
        catalog_decision.platform_permission,
        PlatformPermission::TenantLifecycleRead
    );
    assert_eq!(catalog_decision.action, ORGANIZATION_CATALOG_READ_ACTION);

    let tenant_catalog_request_id = Uuid::now_v7();
    let tenant_catalog = repository_b
        .list_visible(ReadOrganizationCatalog {
            installation_id,
            actor_principal_id: token_race_principal,
            credential_id: read_only_token.id,
            request_id: tenant_catalog_request_id,
        })
        .await?;
    assert_eq!(
        tenant_catalog
            .iter()
            .map(|organization| organization.id)
            .collect::<Vec<_>>(),
        vec![organization_id]
    );
    assert_eq!(
        decision_audit_count(&database, Some(tenant_catalog_request_id)).await?,
        0,
        "tenant-only catalog fallback must not manufacture privileged allow evidence"
    );

    let missing_read_scope_request_id = Uuid::now_v7();
    assert!(matches!(
        repository_a
            .list_visible(ReadOrganizationCatalog {
                installation_id,
                actor_principal_id: token_race_principal,
                credential_id: catalog_without_read_token.id,
                request_id: missing_read_scope_request_id,
            })
            .await,
        Err(RepositoryError::Forbidden(_))
    ));
    assert_eq!(
        decision_audit_count(&database, Some(missing_read_scope_request_id)).await?,
        0,
        "a credential without cloud:read must not receive tenant or Installation catalog access"
    );

    let catalog_race_request_id = Uuid::now_v7();
    let (catalog_race, catalog_role_revocation) = tokio::join!(
        repository_a.list_visible(ReadOrganizationCatalog {
            installation_id,
            actor_principal_id: approver_a,
            credential_id: approver_a_token.id,
            request_id: catalog_race_request_id,
        }),
        repository_b.revoke_platform_role_binding(RevokePlatformRoleBindingWrite {
            installation_id,
            binding_id: approver_a_binding.id,
            expected_version: approver_a_binding.aggregate_version,
            actor_principal_id: owner,
            credential_id: owner_token.id,
            revoked_at: Utc::now(),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("revoke-catalog-race")?,
        })
    );
    catalog_role_revocation?;
    let catalog_race = catalog_race?;
    let catalog_race_decisions =
        decision_audit_count(&database, Some(catalog_race_request_id)).await?;
    match catalog_race.len() {
        2 => assert_eq!(
            catalog_race_decisions, 1,
            "a catalog read serialized before revocation must retain its allow evidence"
        ),
        1 => assert_eq!(
            catalog_race_decisions, 0,
            "a catalog read serialized after revocation must use tenant-only fallback"
        ),
        count => panic!("catalog/revocation race exposed {count} organizations"),
    }
    let post_revocation_request_id = Uuid::now_v7();
    let post_revocation_catalog = repository_a
        .list_visible(ReadOrganizationCatalog {
            installation_id,
            actor_principal_id: approver_a,
            credential_id: approver_a_token.id,
            request_id: post_revocation_request_id,
        })
        .await?;
    assert_eq!(
        post_revocation_catalog
            .iter()
            .map(|organization| organization.id)
            .collect::<Vec<_>>(),
        vec![organization_id],
        "a revoked platform binding must never retain Installation catalog access"
    );
    assert_eq!(
        decision_audit_count(&database, Some(post_revocation_request_id)).await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from outbox_events where event_key = 'identity.privileged-access.authorize'",
                ),
            )
            .await?,
        0,
        "a request-time allow must not introduce a second event mechanism"
    );
    Ok(())
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
    created_at: chrono::DateTime<Utc>,
) -> Result<PlatformRoleBinding, Box<dyn std::error::Error>> {
    let binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        principal_id,
        role,
        policy,
        actor,
        created_at,
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

#[allow(clippy::too_many_arguments)]
async fn create_support_grant(
    repository: &PostgresIdentityRepository,
    installation_id: InstallationId,
    organization_id: OrganizationId,
    subject: PrincipalId,
    requester: PrincipalId,
    requester_credential_id: ApiTokenId,
    approver_a: PrincipalId,
    approver_a_credential_id: ApiTokenId,
    approver_b: PrincipalId,
    approver_b_credential_id: ApiTokenId,
    now: chrono::DateTime<Utc>,
) -> Result<TenantSupportGrant, Box<dyn std::error::Error>> {
    let contract = TenantSupportGrantContract::from_spec(TenantSupportGrantContractSpec {
        grant_id: TenantSupportGrantId::new(),
        principal_id: subject,
        scope: ScopeContext::organization(installation_id, organization_id)?,
        permissions: vec![TenantSupportPermission::HealthRead],
        case_reference: "INC-MT2-C3".into(),
        justification_digest: digest('a')?,
        mode: TenantSupportGrantMode::Standard,
        approval_requirement: TenantSupportApprovalRequirement::Dual,
        approver_ids: vec![approver_a, approver_b],
        tenant_notification: TenantNotificationRequirement::Required,
        security_alert_required: false,
        post_incident_review_required: false,
        starts_at: now - ChronoDuration::minutes(1),
        expires_at: now + ChronoDuration::hours(1),
    })?;
    let proposal = repository
        .propose_tenant_support_grant(ProposeTenantSupportGrantWrite {
            contract,
            actor_principal_id: requester,
            credential_id: requester_credential_id,
            requested_at: now - ChronoDuration::seconds(10),
            request_id: Uuid::now_v7(),
            idempotency: idempotency("propose-grant")?,
        })
        .await?
        .value;
    for (approver, credential_id, key, offset) in [
        (
            approver_a,
            approver_a_credential_id,
            "approve-grant-a",
            -8_i64,
        ),
        (
            approver_b,
            approver_b_credential_id,
            "approve-grant-b",
            -7_i64,
        ),
    ] {
        repository
            .approve_tenant_support_grant(ApproveTenantSupportGrantWrite {
                installation_id,
                grant_id: proposal.id,
                expected_contract_digest: proposal.contract.digest().clone(),
                actor_principal_id: approver,
                credential_id,
                approved_at: now + ChronoDuration::seconds(offset),
                request_id: Uuid::now_v7(),
                idempotency: idempotency(key)?,
            })
            .await?;
    }
    repository
        .find_tenant_support_grant(installation_id, proposal.id)
        .await?
        .ok_or_else(|| "dual approval did not activate the privileged test grant".into())
}

fn platform_request(
    installation_id: InstallationId,
    principal_id: PrincipalId,
    credential_id: ApiTokenId,
    permission: PlatformPermission,
) -> Result<PrivilegedAuthorizationDecisionRequest, Box<dyn std::error::Error>> {
    Ok(PrivilegedAuthorizationDecisionRequest {
        principal_id,
        credential_id,
        platform_permission: permission,
        support_permission: None,
        support_grant_id: None,
        action: TEST_AUTHORIZED_ACTION.into(),
        scope: ScopeContext::installation(installation_id)?,
        resource_id: Uuid::now_v7(),
        request_id: Uuid::now_v7(),
    })
}

fn support_request(
    installation_id: InstallationId,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    credential_id: ApiTokenId,
    grant_id: TenantSupportGrantId,
) -> Result<PrivilegedAuthorizationDecisionRequest, Box<dyn std::error::Error>> {
    Ok(PrivilegedAuthorizationDecisionRequest {
        principal_id,
        credential_id,
        platform_permission: PlatformPermission::TenantSupportUse,
        support_permission: Some(TenantSupportPermission::HealthRead),
        support_grant_id: Some(grant_id),
        action: TEST_AUTHORIZED_ACTION.into(),
        scope: ScopeContext::organization(installation_id, organization_id)?,
        resource_id: Uuid::now_v7(),
        request_id: Uuid::now_v7(),
    })
}

async fn assert_persisted_decision(
    database: &Database<PostgresDialect, PostgresExecutor>,
    request_id: Uuid,
    reference: &AuthorizationDecisionRef,
    credential_id: ApiTokenId,
) -> Result<PrivilegedAuthorizationDecision, Box<dyn std::error::Error>> {
    let details = database
        .fetch_one_as(
            sql_query::<Value>(
                "select details from audit_records where action = 'identity.privileged-access.authorize' and request_id = ",
            )
            .bind(request_id),
        )
        .await?;
    let decision = serde_json::from_value::<PrivilegedAuthorizationDecision>(details)?;
    decision.validate()?;
    assert_eq!(decision.credential.id, credential_id);
    assert_eq!(decision.reference()?, *reference);
    Ok(decision)
}

async fn record_serialized_outcome(
    database: &Database<PostgresDialect, PostgresExecutor>,
    request_id: Uuid,
    credential_id: ApiTokenId,
    result: Result<AuthorizationDecisionRef, RepositoryError>,
) -> Result<i64, Box<dyn std::error::Error>> {
    match result {
        Ok(reference) => {
            assert_persisted_decision(database, request_id, &reference, credential_id).await?;
            Ok(1)
        }
        Err(RepositoryError::Forbidden(_)) => {
            assert_eq!(decision_audit_count(database, Some(request_id)).await?, 0);
            Ok(0)
        }
        Err(error) => Err(error.into()),
    }
}

async fn decision_audit_count(
    database: &Database<PostgresDialect, PostgresExecutor>,
    request_id: Option<Uuid>,
) -> Result<i64, Box<dyn std::error::Error>> {
    let mut query = sql_query::<i64>("select count(*) from audit_records where action = ")
        .bind(DECISION_AUDIT_ACTION);
    if let Some(request_id) = request_id {
        query = query.append(" and request_id = ").bind(request_id);
    }
    Ok(database.fetch_one_as(query).await?)
}

async fn authorized_action_decision_audit_count(
    database: &Database<PostgresDialect, PostgresExecutor>,
    authorized_action: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from audit_records where action = ")
                .bind(DECISION_AUDIT_ACTION)
                .append(" and details ->> 'action' = ")
                .bind(authorized_action),
        )
        .await?)
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
