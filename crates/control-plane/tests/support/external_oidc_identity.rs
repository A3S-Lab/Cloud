use super::*;
use a3s_cloud_control_plane::modules::identity::domain::entities::{
    IdentityBootstrap, IdentityPrincipal, IdentityPrincipalKind, Membership, OidcFlow,
    OidcFlowPurpose, Organization,
};
use a3s_cloud_control_plane::modules::identity::domain::events::{
    MembershipChanged, OrganizationCreated, PrincipalCreated,
};
use a3s_cloud_control_plane::modules::identity::domain::repositories::{
    CompleteOidcLinkWrite, CompleteOidcLoginWrite, IApiTokenRepository, IOidcIdentityRepository,
};
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    ApiTokenDigest, ApiTokenName, ExternalIdentitySubject, MembershipRole, OidcIssuer,
    OidcProviderKey, OrganizationName,
};
use a3s_cloud_control_plane::modules::identity::PostgresIdentityRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, MembershipId, OidcFlowId, OrganizationId, PrincipalId,
    ResourceName, Sha256Digest,
};

fn oidc_digest(byte: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
}

async fn exercise_oidc_repository_authority(
    executor: &PostgresExecutor,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PostgresIdentityRepository::new(executor.clone());
    let now = Utc::now();
    let organization = Organization::create(
        OrganizationId::new(),
        OrganizationName::parse("OIDC repository tenant")?,
        now,
    );
    let principal = IdentityPrincipal::create(
        PrincipalId::new(),
        IdentityPrincipalKind::Human,
        ResourceName::parse("OIDC repository human")?,
        now,
    );
    let membership = Membership::create(
        MembershipId::new(),
        organization.id,
        principal.id,
        MembershipRole::Owner,
        now,
    );
    let bootstrap_token = a3s_cloud_control_plane::modules::identity::domain::entities::ApiToken::issue(
        ApiTokenId::new(),
        organization.id,
        principal.id,
        ApiTokenName::parse("OIDC repository bootstrap")?,
        a3s_cloud_control_plane::modules::identity::domain::value_objects::ApiTokenScope::bootstrap_scopes(),
        now,
        None,
    )?;
    let correlation_id = Uuid::now_v7();
    repository
        .bootstrap(
            IdentityBootstrap {
                organization: organization.clone(),
                principal: principal.clone(),
                membership: membership.clone(),
                api_token: bootstrap_token.clone(),
            },
            ApiTokenDigest::parse(format!("sha256:{}", "8".repeat(64)))?,
            [
                OrganizationCreated::envelope(&organization, correlation_id)?,
                PrincipalCreated::envelope(organization.id, &principal, correlation_id)?,
                MembershipChanged::created(&membership, correlation_id)?,
                a3s_cloud_control_plane::modules::identity::domain::events::ApiTokenCreated::envelope(
                    &bootstrap_token,
                    correlation_id,
                )?,
            ],
            IdempotencyRequest::new(
                "tests/oidc-repository/bootstrap",
                "create",
                b"create",
            )?,
        )
        .await?;
    let provider = OidcProviderKey::parse("repository")?;
    let issuer = OidcIssuer::parse("https://identity.example.test/repository")?;
    let link_flow = OidcFlow::begin(
        OidcFlowId::new(),
        organization.id,
        provider.clone(),
        issuer.clone(),
        oidc_digest('d'),
        OidcFlowPurpose::Link,
        Some(principal.id),
        oidc_digest('a'),
        oidc_digest('b'),
        oidc_digest('c'),
        now,
        now + chrono::Duration::minutes(5),
    )?;
    repository.begin_oidc_flow(link_flow.clone()).await?;
    let pending = repository
        .find_pending_oidc_flow(&link_flow.state_digest, now)
        .await?
        .expect("pending link flow");
    assert_eq!(pending.id, link_flow.id);
    let link_write = CompleteOidcLinkWrite {
        flow_id: link_flow.id,
        provider_config_digest: link_flow.provider_config_digest,
        state_digest: link_flow.state_digest,
        nonce_digest: link_flow.nonce_digest,
        pkce_verifier_digest: link_flow.pkce_verifier_digest,
        subject: ExternalIdentitySubject::parse("repository-subject")?,
        completed_at: now + chrono::Duration::seconds(1),
        request_id: Uuid::now_v7(),
    };
    let link = repository.complete_oidc_link(link_write.clone()).await?;
    assert_eq!(link.principal_id, principal.id);
    assert!(repository.complete_oidc_link(link_write).await.is_err());

    let login_flow = OidcFlow::begin(
        OidcFlowId::new(),
        organization.id,
        provider,
        issuer,
        oidc_digest('d'),
        OidcFlowPurpose::Login,
        None,
        oidc_digest('4'),
        oidc_digest('5'),
        oidc_digest('6'),
        now,
        now + chrono::Duration::minutes(5),
    )?;
    repository.begin_oidc_flow(login_flow.clone()).await?;
    let login_write = CompleteOidcLoginWrite {
        flow_id: login_flow.id,
        provider_config_digest: login_flow.provider_config_digest,
        state_digest: login_flow.state_digest,
        nonce_digest: login_flow.nonce_digest,
        pkce_verifier_digest: login_flow.pkce_verifier_digest,
        subject: ExternalIdentitySubject::parse("repository-subject")?,
        token_id: ApiTokenId::new(),
        token_name: ApiTokenName::parse(format!("OIDC {}", login_flow.id))?,
        token_digest: ApiTokenDigest::parse(format!("sha256:{}", "9".repeat(64)))?,
        completed_at: now + chrono::Duration::seconds(2),
        token_expires_at: now + chrono::Duration::hours(1),
        request_id: Uuid::now_v7(),
    };
    let token = repository.complete_oidc_login(login_write.clone()).await?;
    assert_eq!(token.principal_id, principal.id);
    assert!(token
        .scopes
        .iter()
        .all(|scope| !matches!(scope.as_str(), "platform:write" | "token:write")));
    assert!(repository.complete_oidc_login(login_write).await.is_err());

    let concurrent_flow = OidcFlow::begin(
        OidcFlowId::new(),
        organization.id,
        OidcProviderKey::parse("repository")?,
        OidcIssuer::parse("https://identity.example.test/repository")?,
        oidc_digest('d'),
        OidcFlowPurpose::Login,
        None,
        oidc_digest('7'),
        oidc_digest('a'),
        oidc_digest('b'),
        now,
        now + chrono::Duration::minutes(5),
    )?;
    repository.begin_oidc_flow(concurrent_flow.clone()).await?;
    let concurrent_write = |token_byte: char| CompleteOidcLoginWrite {
        flow_id: concurrent_flow.id,
        provider_config_digest: concurrent_flow.provider_config_digest.clone(),
        state_digest: concurrent_flow.state_digest.clone(),
        nonce_digest: concurrent_flow.nonce_digest.clone(),
        pkce_verifier_digest: concurrent_flow.pkce_verifier_digest.clone(),
        subject: ExternalIdentitySubject::parse("repository-subject").expect("subject"),
        token_id: ApiTokenId::new(),
        token_name: ApiTokenName::parse(format!("OIDC {token_byte} {}", concurrent_flow.id))
            .expect("name"),
        token_digest: ApiTokenDigest::parse(format!(
            "sha256:{}",
            token_byte.to_string().repeat(64)
        ))
        .expect("digest"),
        completed_at: now + chrono::Duration::seconds(3),
        token_expires_at: now + chrono::Duration::hours(1),
        request_id: Uuid::now_v7(),
    };
    let (left, right) = tokio::join!(
        repository.complete_oidc_login(concurrent_write('c')),
        repository.complete_oidc_login(concurrent_write('f')),
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);

    let database = Database::new(PostgresDialect, executor.clone());
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64)>(
                "select (select count(*) from oidc_flows where id = ",
            )
            .bind(login_flow.id.as_uuid())
            .append(" and consumed_at is not null), (select count(*) from external_identity_links where id = ")
            .bind(link.id.as_uuid())
            .append(" and principal_id = ")
            .bind(principal.id.as_uuid())
            .append("), (select count(*) from api_tokens where id = ")
            .bind(token.id.as_uuid())
            .append(" and principal_id = ")
            .bind(principal.id.as_uuid())
            .append("), (select count(*) from audit_records where organization_id = ")
            .bind(organization.id.as_uuid())
            .append(" and action in ('identity.external-identity.linked', 'identity.oidc.login')), (select count(*) from api_tokens where organization_id = ")
            .bind(organization.id.as_uuid())
            .append(" and name like 'OIDC %')"),
        )
        .await?;
    assert_eq!(evidence, (1, 1, 1, 3, 3));
    Ok(())
}

pub async fn exercise_external_oidc_identity_foundation(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = connect_and_migrate(&postgres_url, 4).await?;
    exercise_oidc_repository_authority(&executor).await?;
    let database = Database::new(PostgresDialect, executor);
    let organization_id = Uuid::now_v7();
    let principal_id = Uuid::now_v7();
    let other_principal_id = Uuid::now_v7();
    let link_id = Uuid::now_v7();
    let now = Utc::now();
    let issuer = "https://identity.example.test/tenant/";

    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", 'OIDC tenant', 'oidc-tenant', 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    for (principal_id, name) in [
        (principal_id, "OIDC human"),
        (other_principal_id, "Other OIDC human"),
    ] {
        database
            .execute(
                sql_query::<()>(
                    "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
                )
                .bind(principal_id)
                .append(", 'human', ")
                .bind(name)
                .append(", 1, ")
                .bind(now)
                .append(", null)"),
            )
            .await?;
    }
    database
        .execute(
            sql_query::<()>(
                "insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (",
            )
            .bind(Uuid::now_v7())
            .append(", ")
            .bind(organization_id)
            .append(", ")
            .bind(principal_id)
            .append(", 'member', 1, ")
            .bind(now)
            .append(", ")
            .bind(now)
            .append(", null)"),
        )
        .await?;

    database
        .execute(
            sql_query::<()>(
                "insert into external_identity_links (id, provider_key, issuer, subject, principal_id, aggregate_version, created_at, last_verified_at, revoked_at) values (",
            )
            .bind(link_id)
            .append(", 'workforce', ")
            .bind(issuer)
            .append(", 'subject-42', ")
            .bind(principal_id)
            .append(", 1, ")
            .bind(now)
            .append(", ")
            .bind(now)
            .append(", null)"),
        )
        .await?;
    let exact_link = database
        .fetch_one_as(
            sql_query::<(Uuid, String, String, i64)>(
                "select principal_id, issuer, subject, aggregate_version from external_identity_links where id = ",
            )
            .bind(link_id),
        )
        .await?;
    assert_eq!(
        exact_link,
        (principal_id, issuer.into(), "subject-42".into(), 1)
    );

    let duplicate_external_identity = database
        .execute(
            sql_query::<()>(
                "insert into external_identity_links (id, provider_key, issuer, subject, principal_id, aggregate_version, created_at, last_verified_at, revoked_at) values (",
            )
            .bind(Uuid::now_v7())
            .append(", 'provider-alias', ")
            .bind(issuer)
            .append(", 'subject-42', ")
            .bind(other_principal_id)
            .append(", 1, ")
            .bind(now)
            .append(", ")
            .bind(now)
            .append(", null)"),
        )
        .await;
    assert!(
        duplicate_external_identity.is_err(),
        "one exact issuer/subject must never resolve to two Principals"
    );
    let duplicate_active_provider = database
        .execute(
            sql_query::<()>(
                "insert into external_identity_links (id, provider_key, issuer, subject, principal_id, aggregate_version, created_at, last_verified_at, revoked_at) values (",
            )
            .bind(Uuid::now_v7())
            .append(", 'workforce', ")
            .bind(issuer)
            .append(", 'changed-subject', ")
            .bind(principal_id)
            .append(", 1, ")
            .bind(now)
            .append(", ")
            .bind(now)
            .append(", null)"),
        )
        .await;
    assert!(
        duplicate_active_provider.is_err(),
        "one Principal must have at most one active subject per issuer"
    );

    let verified_at = now + chrono::Duration::minutes(1);
    let verified = database
        .execute(
            sql_query::<()>(
                "update external_identity_links set aggregate_version = 2, last_verified_at = ",
            )
            .bind(verified_at)
            .append(" where id = ")
            .bind(link_id),
        )
        .await?;
    assert_eq!(verified.rows_affected, 1);
    let identity_mutation = database
        .execute(
            sql_query::<()>(
                "update external_identity_links set provider_key = 'other', aggregate_version = 3, last_verified_at = ",
            )
            .bind(now + chrono::Duration::minutes(2))
            .append(" where id = ")
            .bind(link_id),
        )
        .await;
    assert!(
        identity_mutation.is_err(),
        "linked identity must be immutable"
    );

    let revoked_at = now + chrono::Duration::minutes(2);
    let revoked = database
        .execute(
            sql_query::<()>(
                "update external_identity_links set aggregate_version = 3, revoked_at = ",
            )
            .bind(revoked_at)
            .append(" where id = ")
            .bind(link_id),
        )
        .await?;
    assert_eq!(revoked.rows_affected, 1);
    let terminal_mutation = database
        .execute(
            sql_query::<()>(
                "update external_identity_links set aggregate_version = 4, last_verified_at = ",
            )
            .bind(now + chrono::Duration::minutes(3))
            .append(" where id = ")
            .bind(link_id),
        )
        .await;
    assert!(
        terminal_mutation.is_err(),
        "revoked identity history must remain terminal"
    );
    let deletion = database
        .execute(sql_query::<()>("delete from external_identity_links where id = ").bind(link_id))
        .await;
    assert!(
        deletion.is_err(),
        "linked identity history must be retained"
    );

    let flow_id = Uuid::now_v7();
    let config_digest = format!("sha256:{}", "e".repeat(64));
    let state_digest = format!("sha256:{}", "1".repeat(64));
    let nonce_digest = format!("sha256:{}", "2".repeat(64));
    let pkce_digest = format!("sha256:{}", "3".repeat(64));
    let expires_at = now + chrono::Duration::minutes(5);
    database
        .execute(
            sql_query::<()>(
                "insert into oidc_flows (id, organization_id, provider_key, issuer, provider_config_digest, purpose, principal_id, state_digest, nonce_digest, pkce_verifier_digest, created_at, expires_at, consumed_at) values (",
            )
            .bind(flow_id)
            .append(", ")
            .bind(organization_id)
            .append(", 'workforce', ")
            .bind(issuer)
            .append(", ")
            .bind(config_digest.as_str())
            .append(", 'login', null, ")
            .bind(state_digest.as_str())
            .append(", ")
            .bind(nonce_digest.as_str())
            .append(", ")
            .bind(pkce_digest.as_str())
            .append(", ")
            .bind(now)
            .append(", ")
            .bind(expires_at)
            .append(", null)"),
        )
        .await?;
    let invalid_link_flow = database
        .execute(
            sql_query::<()>(
                "insert into oidc_flows (id, organization_id, provider_key, issuer, provider_config_digest, purpose, principal_id, state_digest, nonce_digest, pkce_verifier_digest, created_at, expires_at, consumed_at) values (",
            )
            .bind(Uuid::now_v7())
            .append(", ")
            .bind(organization_id)
            .append(", 'workforce', ")
            .bind(issuer)
            .append(", ")
            .bind(config_digest.as_str())
            .append(", 'link', null, ")
            .bind(format!("sha256:{}", "7".repeat(64)))
            .append(", ")
            .bind(format!("sha256:{}", "a".repeat(64)))
            .append(", ")
            .bind(format!("sha256:{}", "b".repeat(64)))
            .append(", ")
            .bind(now)
            .append(", ")
            .bind(expires_at)
            .append(", null)"),
        )
        .await;
    assert!(
        invalid_link_flow.is_err(),
        "link flows must bind one authenticated Principal"
    );
    let consumed_at = now + chrono::Duration::minutes(1);
    let consumed = database
        .execute(
            sql_query::<()>("update oidc_flows set consumed_at = ")
                .bind(consumed_at)
                .append(" where id = ")
                .bind(flow_id),
        )
        .await?;
    assert_eq!(consumed.rows_affected, 1);
    let replay = database
        .execute(
            sql_query::<()>("update oidc_flows set consumed_at = ")
                .bind(consumed_at + chrono::Duration::seconds(1))
                .append(" where id = ")
                .bind(flow_id),
        )
        .await;
    assert!(
        replay.is_err(),
        "OIDC callback flow must consume exactly once"
    );
    let flow_mutation = database
        .execute(
            sql_query::<()>("update oidc_flows set provider_config_digest = ")
                .bind(format!("sha256:{}", "9".repeat(64)))
                .append(", consumed_at = ")
                .bind(consumed_at)
                .append(" where id = ")
                .bind(flow_id),
        )
        .await;
    assert!(
        flow_mutation.is_err(),
        "flow provider configuration identity must be immutable"
    );

    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>(
                "select (select count(*) from external_identity_links where id = ",
            )
            .bind(link_id)
            .append(" and principal_id = ")
            .bind(principal_id)
            .append(" and aggregate_version = 3 and revoked_at = ")
            .bind(revoked_at)
            .append("), (select count(*) from oidc_flows where id = ")
            .bind(flow_id)
            .append(" and organization_id = ")
            .bind(organization_id)
            .append(" and purpose = 'login' and principal_id is null and consumed_at = ")
            .bind(consumed_at)
            .append("), (select count(*) from a3s_orm_migrations where version = '102' and char_length(checksum) = 64)"),
        )
        .await?;
    assert_eq!(
        evidence,
        (1, 1, 1),
        "a3s-orm migration, exact identity history, and one-time flow must persist"
    );
    Ok(())
}
