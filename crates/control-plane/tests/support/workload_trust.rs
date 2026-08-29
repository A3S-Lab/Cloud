use super::*;
use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass};
use a3s_cloud_control_plane::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, AcceptedTrustDomainRevision,
    AcceptedWorkloadIdentityPolicyRevision, PlatformRbacBootstrap, PlatformRoleBinding,
};
use a3s_cloud_control_plane::modules::identity::domain::events::ApiTokenRevoked;
use a3s_cloud_control_plane::modules::identity::domain::repositories::{
    AcceptTrustDomainRevisionWrite, AcceptWorkloadIdentityPolicyRevisionWrite,
    BootstrapPlatformRbacWrite, CreatePlatformRoleBindingWrite, IApiTokenRepository,
    IPlatformRbacRepository, ITrustDomainRepository, IWorkloadIdentityPolicyRepository,
    ListTrustDomainRevisions, ListWorkloadIdentityPolicyRevisions, ReadCurrentTrustDomain,
    ReadCurrentWorkloadIdentityPolicy, ReadCurrentWorkloadIdentityPolicyForWorkload,
    ReadTrustDomainRevision, ReadWorkloadIdentityPolicyRevision,
};
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    ApiTokenScope, PlatformRole, PlatformRolePolicyContract, PrivateServiceName,
    TrustDomainContract, TrustDomainContractSpec, TrustDomainName, WorkloadIdentityAudience,
    WorkloadIdentityFormat, WorkloadIdentityPolicyContract, WorkloadIdentityPolicySpec,
    WorkloadIdentityRevocationMode, WorkloadProductRole,
};
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    NodePoolId, PlatformRoleBindingId, PlatformRolePolicyId, PrincipalId, Sha256Digest,
    TrustDomainId, WorkloadId, WorkloadIdentityPolicyId, WorkloadRevisionId,
};
use chrono::Duration as ChronoDuration;

pub async fn exercise_workload_trust_authority(
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
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let node_pool_id = NodePoolId::new();
    let owner = PrincipalId::new();
    let operator = PrincipalId::new();
    let now = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .ok_or("workload trust test timestamp exceeds PostgreSQL precision")?;
    seed_owner_lineage(
        &database,
        organization_id,
        project_id,
        environment_id,
        workload_id,
        workload_revision_id,
        node_pool_id,
        owner,
        operator,
        now,
    )
    .await?;

    let owner_credential = test_api_token(
        organization_id,
        owner,
        "workload trust owner",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::minutes(30),
        None,
    )?;
    let operator_credential = test_api_token(
        organization_id,
        operator,
        "workload trust operator",
        ApiTokenScope::bootstrap_scopes(),
        now - ChronoDuration::minutes(30),
        None,
    )?;
    persist_test_api_token(&database, &owner_credential, 701).await?;
    persist_test_api_token(&database, &operator_credential, 702).await?;

    let repository_a = PostgresIdentityRepository::new(executor.clone());
    let repository_b = PostgresIdentityRepository::new(executor.clone());
    let platform_policy = AcceptedPlatformRolePolicyRevision::accept(
        PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())?,
        1,
        owner,
        now,
    )?;
    let owner_binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        owner,
        PlatformRole::PlatformOwner,
        &platform_policy,
        owner,
        now,
    )?;
    repository_a
        .bootstrap_platform_rbac(BootstrapPlatformRbacWrite {
            bootstrap: PlatformRbacBootstrap {
                policy: platform_policy.clone(),
                owner_binding,
            },
            actor_principal_id: owner,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("platform-bootstrap", "wi1-owner")?,
        })
        .await?;
    let operator_binding = PlatformRoleBinding::create(
        PlatformRoleBindingId::new(),
        installation_id,
        operator,
        PlatformRole::PlatformOperator,
        &platform_policy,
        owner,
        now + ChronoDuration::milliseconds(1),
    )?;
    repository_a
        .create_platform_role_binding(CreatePlatformRoleBindingWrite {
            binding: operator_binding,
            expected_policy_revision_id: platform_policy.id,
            actor_principal_id: owner,
            credential_id: owner_credential.id,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("platform-binding", "wi1-operator")?,
        })
        .await?;

    let trust_domain_id = TrustDomainId::new();
    let trust_one = trust_revision(
        installation_id,
        trust_domain_id,
        1,
        owner,
        now + ChronoDuration::seconds(1),
        'a',
        'b',
    )?;
    let trust_one_write = AcceptTrustDomainRevisionWrite {
        revision: trust_one.clone(),
        expected_previous_revision_id: None,
        actor_principal_id: owner,
        credential_id: owner_credential.id,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("trust-domain", "wi1-trust-one")?,
    };
    let accepted_trust_one =
        ITrustDomainRepository::accept(&repository_a, trust_one_write.clone()).await?;
    assert!(!accepted_trust_one.replayed);
    assert!(
        ITrustDomainRepository::accept(&repository_b, trust_one_write.clone())
            .await?
            .replayed
    );
    assert!(matches!(
        ITrustDomainRepository::accept(
            &repository_b,
            AcceptTrustDomainRevisionWrite {
                revision: trust_revision(
                    installation_id,
                    TrustDomainId::new(),
                    1,
                    operator,
                    now + ChronoDuration::seconds(2),
                    'c',
                    'd',
                )?,
                expected_previous_revision_id: None,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("trust-domain", "wi1-operator-denied")?,
            },
        )
        .await,
        Err(RepositoryError::Forbidden(_))
    ));
    let duplicate_name = trust_revision(
        installation_id,
        TrustDomainId::new(),
        1,
        owner,
        now + ChronoDuration::seconds(2),
        'e',
        'f',
    )?;
    assert!(matches!(
        ITrustDomainRepository::accept(
            &repository_a,
            AcceptTrustDomainRevisionWrite {
                revision: duplicate_name,
                expected_previous_revision_id: None,
                actor_principal_id: owner,
                credential_id: owner_credential.id,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("trust-domain", "wi1-duplicate-name")?,
            },
        )
        .await,
        Err(RepositoryError::Conflict(_))
    ));

    assert_eq!(
        ITrustDomainRepository::read_current(
            &repository_a,
            ReadCurrentTrustDomain {
                installation_id,
                trust_domain_id,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
            },
        )
        .await?,
        Some(trust_one.clone())
    );
    assert_eq!(
        ITrustDomainRepository::read_revision(
            &repository_b,
            ReadTrustDomainRevision {
                installation_id,
                trust_domain_id,
                revision_id: trust_one.id,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
            },
        )
        .await?,
        Some(trust_one.clone())
    );

    let trust_a = successor_trust(&trust_one, owner, '2', now + ChronoDuration::seconds(3))?;
    let trust_b = successor_trust(&trust_one, owner, '3', now + ChronoDuration::seconds(3))?;
    let write_a = trust_write(
        trust_a.clone(),
        trust_one.id,
        owner,
        owner_credential.id,
        "wi1-trust-two-a",
    )?;
    let write_b = trust_write(
        trust_b.clone(),
        trust_one.id,
        owner,
        owner_credential.id,
        "wi1-trust-two-b",
    )?;
    let (accepted_a, accepted_b) = tokio::join!(
        ITrustDomainRepository::accept(&repository_a, write_a.clone()),
        ITrustDomainRepository::accept(&repository_b, write_b.clone())
    );
    let (current_trust, winning_trust_write) = match (accepted_a, accepted_b) {
        (Ok(value), Err(RepositoryError::Conflict(_))) => (value.value, write_a),
        (Err(RepositoryError::Conflict(_)), Ok(value)) => (value.value, write_b),
        results => {
            return Err(format!(
                "concurrent trust-domain CAS did not accept exactly one successor: {results:?}"
            )
            .into())
        }
    };
    assert!(
        ITrustDomainRepository::accept(&repository_a, winning_trust_write)
            .await?
            .replayed
    );
    assert_eq!(
        ITrustDomainRepository::list_revisions(
            &repository_b,
            ListTrustDomainRevisions {
                installation_id,
                trust_domain_id,
                limit: 10,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
            },
        )
        .await?
        .len(),
        2
    );

    let stale_policy = policy_revision(
        installation_id,
        organization_id,
        project_id,
        environment_id,
        WorkloadIdentityPolicyId::new(),
        workload_id,
        workload_revision_id,
        node_pool_id,
        &trust_one,
        1,
        owner,
        now + ChronoDuration::seconds(4),
        "stale.model.internal",
    )?;
    assert!(matches!(
        IWorkloadIdentityPolicyRepository::accept(
            &repository_a,
            AcceptWorkloadIdentityPolicyRevisionWrite {
                revision: stale_policy,
                expected_previous_revision_id: None,
                actor_principal_id: owner,
                credential_id: owner_credential.id,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("workload-policy", "wi1-stale-trust")?,
            },
        )
        .await,
        Err(RepositoryError::Conflict(_))
    ));

    let policy_id = WorkloadIdentityPolicyId::new();
    let policy_one = policy_revision(
        installation_id,
        organization_id,
        project_id,
        environment_id,
        policy_id,
        workload_id,
        workload_revision_id,
        node_pool_id,
        &current_trust,
        1,
        owner,
        now + ChronoDuration::seconds(5),
        "model.internal",
    )?;
    let policy_one_write = AcceptWorkloadIdentityPolicyRevisionWrite {
        revision: policy_one.clone(),
        expected_previous_revision_id: None,
        actor_principal_id: owner,
        credential_id: owner_credential.id,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("workload-policy", "wi1-policy-one")?,
    };
    IWorkloadIdentityPolicyRepository::accept(&repository_a, policy_one_write.clone()).await?;
    assert!(
        IWorkloadIdentityPolicyRepository::accept(&repository_b, policy_one_write)
            .await?
            .replayed
    );
    assert_eq!(
        IWorkloadIdentityPolicyRepository::read_current(
            &repository_a,
            ReadCurrentWorkloadIdentityPolicy {
                installation_id,
                organization_id,
                policy_id,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
            },
        )
        .await?,
        Some(policy_one.clone())
    );
    assert_eq!(
        IWorkloadIdentityPolicyRepository::read_current_for_workload(
            &repository_b,
            ReadCurrentWorkloadIdentityPolicyForWorkload {
                installation_id,
                organization_id,
                workload_id,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
            },
        )
        .await?,
        Some(policy_one.clone())
    );
    assert_eq!(
        IWorkloadIdentityPolicyRepository::read_revision(
            &repository_a,
            ReadWorkloadIdentityPolicyRevision {
                installation_id,
                organization_id,
                policy_id,
                revision_id: policy_one.id,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
            },
        )
        .await?,
        Some(policy_one.clone())
    );

    let duplicate_policy = policy_revision(
        installation_id,
        organization_id,
        project_id,
        environment_id,
        WorkloadIdentityPolicyId::new(),
        workload_id,
        workload_revision_id,
        node_pool_id,
        &current_trust,
        1,
        owner,
        now + ChronoDuration::seconds(6),
        "duplicate.model.internal",
    )?;
    assert!(matches!(
        IWorkloadIdentityPolicyRepository::accept(
            &repository_b,
            AcceptWorkloadIdentityPolicyRevisionWrite {
                revision: duplicate_policy,
                expected_previous_revision_id: None,
                actor_principal_id: owner,
                credential_id: owner_credential.id,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("workload-policy", "wi1-duplicate-workload")?,
            },
        )
        .await,
        Err(RepositoryError::Conflict(_))
    ));

    let policy_a = successor_policy(
        &policy_one,
        owner,
        "model-a.internal",
        now + ChronoDuration::seconds(7),
    )?;
    let policy_b = successor_policy(
        &policy_one,
        owner,
        "model-b.internal",
        now + ChronoDuration::seconds(7),
    )?;
    let policy_write_a = policy_write(
        policy_a,
        policy_one.id,
        owner,
        owner_credential.id,
        "wi1-policy-two-a",
    )?;
    let policy_write_b = policy_write(
        policy_b,
        policy_one.id,
        owner,
        owner_credential.id,
        "wi1-policy-two-b",
    )?;
    let (policy_result_a, policy_result_b) = tokio::join!(
        IWorkloadIdentityPolicyRepository::accept(&repository_a, policy_write_a),
        IWorkloadIdentityPolicyRepository::accept(&repository_b, policy_write_b)
    );
    let current_policy = match (policy_result_a, policy_result_b) {
        (Ok(value), Err(RepositoryError::Conflict(_)))
        | (Err(RepositoryError::Conflict(_)), Ok(value)) => value.value,
        results => {
            return Err(format!(
                "concurrent workload policy CAS did not accept exactly one successor: {results:?}"
            )
            .into())
        }
    };
    assert_eq!(current_policy.revision_number, 2);
    assert_eq!(
        IWorkloadIdentityPolicyRepository::list_revisions(
            &repository_a,
            ListWorkloadIdentityPolicyRevisions {
                installation_id,
                organization_id,
                policy_id,
                limit: 10,
                actor_principal_id: operator,
                credential_id: operator_credential.id,
                request_id: Uuid::now_v7(),
            },
        )
        .await?
        .len(),
        2
    );

    let mut drifted_replay = trust_one_write;
    drifted_replay.idempotency = IdempotencyRequest::new(
        "tests/workload-trust/trust-domain",
        "wi1-trust-one",
        b"drifted canonical request",
    )?;
    assert!(matches!(
        ITrustDomainRepository::accept(&repository_a, drifted_replay).await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    assert!(
        database
            .execute(
                sql_query::<()>("delete from trust_domain_revisions where id = ")
                    .bind(trust_one.id.as_uuid()),
            )
            .await
            .is_err(),
        "trust-domain history must be immutable"
    );
    assert!(
        database
            .execute(
                sql_query::<()>(
                    "update workload_identity_policy_revisions set digest = digest where id = "
                )
                .bind(policy_one.id.as_uuid()),
            )
            .await
            .is_err(),
        "workload identity policy history must be immutable"
    );

    let trust_three =
        successor_trust(&current_trust, owner, '4', now + ChronoDuration::seconds(8))?;
    let race_request_id = Uuid::now_v7();
    let race_write = AcceptTrustDomainRevisionWrite {
        revision: trust_three.clone(),
        expected_previous_revision_id: Some(current_trust.id),
        actor_principal_id: owner,
        credential_id: owner_credential.id,
        request_id: race_request_id,
        idempotency: idempotency("trust-domain", "wi1-token-race")?,
    };
    let mut revoked_owner_credential = owner_credential.clone();
    assert!(revoked_owner_credential.revoke(Utc::now()));
    let revocation_event = ApiTokenRevoked::envelope(&revoked_owner_credential, Uuid::now_v7())?;
    let (race_result, revocation_result) = tokio::join!(
        ITrustDomainRepository::accept(&repository_a, race_write.clone()),
        repository_b.revoke(
            revoked_owner_credential,
            Some(revocation_event),
            idempotency("api-token", "wi1-token-race")?,
        )
    );
    revocation_result?;
    let race_committed = match race_result {
        Ok(value) => {
            assert_eq!(value.value, trust_three);
            true
        }
        Err(RepositoryError::Forbidden(_)) => false,
        Err(error) => {
            return Err(format!(
                "workload trust mutation and token revocation were not serialized: {error:?}"
            )
            .into())
        }
    };
    assert!(matches!(
        ITrustDomainRepository::accept(&repository_b, race_write).await,
        Err(RepositoryError::Forbidden(_))
    ));
    let race_facts = database
        .fetch_one_as(
            sql_query::<(i64, i64)>(
                "select (select count(*) from audit_records where request_id = ",
            )
            .bind(race_request_id)
            .append(" and action = 'identity.privileged-access.authorize'), (select count(*) from audit_records where request_id = ")
            .bind(race_request_id)
            .append(" and action = 'identity.trust-domain.revision-accepted')"),
        )
        .await?;
    assert_eq!(
        race_facts,
        if race_committed { (1, 1) } else { (0, 0) },
        "authorization decision and workload trust fact must commit or roll back together"
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from trust_domain_heads), (select count(*) from trust_domain_revisions), (select count(*) from workload_identity_policy_heads), (select count(*) from workload_identity_policy_revisions), (select count(*) from outbox_events where event_key in ('identity.trust-domain.revision-accepted', 'identity.workload-identity-policy.revision-accepted')), (select count(*) from audit_records where action in ('identity.trust-domain.revision-accepted', 'identity.workload-identity-policy.revision-accepted'))",
            ),
        )
        .await?;
    assert_eq!(
        evidence,
        if race_committed {
            (1, 3, 1, 2, 5, 5)
        } else {
            (1, 2, 1, 2, 4, 4)
        },
        "workload trust heads, immutable histories, Outbox and Audit must commit exactly once"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn seed_owner_lineage(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    node_pool_id: NodePoolId,
    owner: PrincipalId,
    operator: PrincipalId,
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let node_pool_digest = digest('9');
    database
        .execute(
            sql_query::<()>("insert into organizations (id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", 'WI1 workload trust', ")
                .bind(format!("wi1-workload-trust-{organization_id}"))
                .append(", 1, ")
                .bind(now)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", 'WI1 project', 'wi1-project', 1, ")
                .bind(now)
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
                .append(", 'WI1 environment', 'wi1-environment', 1, ")
                .bind(now)
                .append(")"),
        )
        .await?;
    for (principal_id, name) in [(owner, "WI1 owner"), (operator, "WI1 operator")] {
        database
            .execute(
                sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                    .bind(principal_id.as_uuid())
                    .append(", 'human', ")
                    .bind(name)
                    .append(", 1, ")
                    .bind(now - ChronoDuration::hours(1))
                    .append(", null)"),
            )
            .await?;
    }
    database
        .execute(
            sql_query::<()>("insert into node_pools (organization_id, id, name, name_key, spec_digest, aggregate_version, created_at, updated_at) values (")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(node_pool_id.as_uuid())
                .append(", 'WI1 node pool', 'wi1-node-pool', ")
                .bind(node_pool_digest.as_str())
                .append(", 1, ")
                .bind(now)
                .append(", ")
                .bind(now)
                .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>("insert into workloads (id, organization_id, project_id, environment_id, name, name_key, desired_state, active_revision_id, aggregate_version, created_at, updated_at) values (")
                .bind(workload_id.as_uuid())
                .append(", ")
                .bind(organization_id.as_uuid())
                .append(", ")
                .bind(project_id.as_uuid())
                .append(", ")
                .bind(environment_id.as_uuid())
                .append(", 'WI1 workload', 'wi1-workload', 'running', null, 1, ")
                .bind(now)
                .append(", ")
                .bind(now)
                .append(")"),
        )
        .await?;
    let artifact_digest = digest('8');
    let template_digest = digest('7');
    let request_digest = digest('6');
    database
        .execute(
            sql_query::<()>("insert into workload_revisions (id, workload_id, generation, artifact_uri, artifact_digest, artifact_media_type, template, template_digest, created_at, resolution_state, artifact_source_uri, expected_artifact_digest, template_request, request_digest, resolved_at) values (")
                .bind(workload_revision_id.as_uuid())
                .append(", ")
                .bind(workload_id.as_uuid())
                .append(", 1, ")
                .bind(format!("oci://registry.example/wi1@{}", artifact_digest.as_str()))
                .append(", ")
                .bind(artifact_digest.as_str())
                .append(", 'application/vnd.oci.image.manifest.v1+json', '{}'::jsonb, ")
                .bind(template_digest.as_str())
                .append(", ")
                .bind(now)
                .append(", 'resolved', 'oci://registry.example/wi1:latest', ")
                .bind(artifact_digest.as_str())
                .append(", '{}'::jsonb, ")
                .bind(request_digest.as_str())
                .append(", ")
                .bind(now)
                .append(")"),
        )
        .await?;
    Ok(())
}

fn trust_revision(
    installation_id: InstallationId,
    trust_domain_id: TrustDomainId,
    revision_number: u64,
    accepted_by: PrincipalId,
    accepted_at: chrono::DateTime<Utc>,
    provider_digest: char,
    bundle_digest: char,
) -> Result<AcceptedTrustDomainRevision, Box<dyn std::error::Error>> {
    let contract = TrustDomainContract::from_spec(TrustDomainContractSpec {
        installation_id,
        trust_domain_id,
        name: TrustDomainName::parse("prod.a3s.internal")?,
        provider_profile_digest: digest(provider_digest),
        trust_bundle_digest: digest(bundle_digest),
        node_attestation_profile_digests: vec![digest('c')],
        identity_formats: vec![WorkloadIdentityFormat::X509Svid],
        max_credential_lifetime_seconds: 600,
        rotation_overlap_seconds: 60,
        revocation_mode: WorkloadIdentityRevocationMode::EpochAndExpiry,
        federation_bundle_digests: vec![],
    })?;
    Ok(AcceptedTrustDomainRevision::accept(
        contract,
        revision_number,
        accepted_by,
        accepted_at,
    )?)
}

fn successor_trust(
    current: &AcceptedTrustDomainRevision,
    accepted_by: PrincipalId,
    bundle_digest: char,
    accepted_at: chrono::DateTime<Utc>,
) -> Result<AcceptedTrustDomainRevision, Box<dyn std::error::Error>> {
    let mut spec = current.contract.spec().clone();
    spec.trust_bundle_digest = digest(bundle_digest);
    Ok(AcceptedTrustDomainRevision::accept(
        TrustDomainContract::from_spec(spec)?,
        current.revision_number + 1,
        accepted_by,
        accepted_at,
    )?)
}

fn trust_write(
    revision: AcceptedTrustDomainRevision,
    previous_revision_id: a3s_cloud_control_plane::modules::shared_kernel::domain::TrustDomainRevisionId,
    actor_principal_id: PrincipalId,
    credential_id: a3s_cloud_control_plane::modules::shared_kernel::domain::ApiTokenId,
    key: &str,
) -> Result<AcceptTrustDomainRevisionWrite, Box<dyn std::error::Error>> {
    Ok(AcceptTrustDomainRevisionWrite {
        revision,
        expected_previous_revision_id: Some(previous_revision_id),
        actor_principal_id,
        credential_id,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("trust-domain", key)?,
    })
}

#[allow(clippy::too_many_arguments)]
fn policy_revision(
    installation_id: InstallationId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    policy_id: WorkloadIdentityPolicyId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    node_pool_id: NodePoolId,
    trust: &AcceptedTrustDomainRevision,
    revision_number: u64,
    accepted_by: PrincipalId,
    accepted_at: chrono::DateTime<Utc>,
    audience: &str,
) -> Result<AcceptedWorkloadIdentityPolicyRevision, Box<dyn std::error::Error>> {
    let contract = WorkloadIdentityPolicyContract::from_spec(WorkloadIdentityPolicySpec {
        installation_id,
        trust_domain_id: trust.trust_domain_id,
        trust_domain_revision_id: trust.id,
        organization_id,
        project_id,
        environment_id,
        policy_id,
        workload_id,
        workload_revision_id,
        product_role: WorkloadProductRole::AgentService,
        runtime_class: RuntimeUnitClass::Service,
        semantics_profile_digest: digest('d'),
        node_pool_id,
        isolation_level: RuntimeIsolationLevel::Container,
        attestation_profile_digest: digest('c'),
        confidential_compute: false,
        identity_formats: vec![WorkloadIdentityFormat::X509Svid],
        credential_lifetime_seconds: 300,
        rotate_before_expiry_seconds: 60,
        drain_on_rotation_failure: true,
        revoke_on_stop: true,
        audiences: vec![WorkloadIdentityAudience::parse(audience)?],
        service_names: vec![PrivateServiceName::parse("agent.prod.a3s.internal")?],
        peer_policy_revision_digests: vec![],
    })?;
    Ok(AcceptedWorkloadIdentityPolicyRevision::accept(
        contract,
        revision_number,
        accepted_by,
        accepted_at,
    )?)
}

fn successor_policy(
    current: &AcceptedWorkloadIdentityPolicyRevision,
    accepted_by: PrincipalId,
    audience: &str,
    accepted_at: chrono::DateTime<Utc>,
) -> Result<AcceptedWorkloadIdentityPolicyRevision, Box<dyn std::error::Error>> {
    let mut spec = current.contract.spec().clone();
    spec.audiences = vec![WorkloadIdentityAudience::parse(audience)?];
    Ok(AcceptedWorkloadIdentityPolicyRevision::accept(
        WorkloadIdentityPolicyContract::from_spec(spec)?,
        current.revision_number + 1,
        accepted_by,
        accepted_at,
    )?)
}

fn policy_write(
    revision: AcceptedWorkloadIdentityPolicyRevision,
    previous_revision_id: a3s_cloud_control_plane::modules::shared_kernel::domain::WorkloadIdentityPolicyRevisionId,
    actor_principal_id: PrincipalId,
    credential_id: a3s_cloud_control_plane::modules::shared_kernel::domain::ApiTokenId,
    key: &str,
) -> Result<AcceptWorkloadIdentityPolicyRevisionWrite, Box<dyn std::error::Error>> {
    Ok(AcceptWorkloadIdentityPolicyRevisionWrite {
        revision,
        expected_previous_revision_id: Some(previous_revision_id),
        actor_principal_id,
        credential_id,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("workload-policy", key)?,
    })
}

fn digest(byte: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
}

fn idempotency(scope: &str, key: &str) -> Result<IdempotencyRequest, Box<dyn std::error::Error>> {
    Ok(IdempotencyRequest::new(
        format!("tests/workload-trust/{scope}"),
        key,
        &serde_json::to_vec(&json!({"scope": scope, "key": key}))?,
    )?)
}
