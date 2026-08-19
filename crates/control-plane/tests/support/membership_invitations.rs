use super::*;
use a3s_cloud_control_plane::ControlPlane;

const POSTGRES_URL_ENV: &str = "A3S_CLOUD_MI_POSTGRES_URL";
const BOOTSTRAP_TOKEN_ENV: &str = "A3S_CLOUD_MI_BOOTSTRAP_TOKEN";
const BOOTSTRAP_TOKEN_VALUE: &str = "mi-bootstrap-credential-0123456789abcdef";
const INVITEE_TOKEN: &str = "a3s_6666666666666666666666666666666666666666666666666666666666666666";
const INVITEE_READ_ONLY_TOKEN: &str =
    "a3s_7777777777777777777777777777777777777777777777777777777777777777";
const REVOKED_INVITEE_TOKEN: &str =
    "a3s_8888888888888888888888888888888888888888888888888888888888888888";

pub async fn exercise_membership_invitation_persistence(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor);
    let _postgres_url = EnvironmentOverride::set(POSTGRES_URL_ENV, &postgres_url);
    let _bootstrap_token = EnvironmentOverride::set(BOOTSTRAP_TOKEN_ENV, BOOTSTRAP_TOKEN_VALUE);
    let state = tempfile::tempdir()?;
    let mut application_config = config();
    application_config.postgres.serving_url_env = POSTGRES_URL_ENV.into();
    application_config.auth.bootstrap_token_env = BOOTSTRAP_TOKEN_ENV.into();
    configure_ephemeral_application_state(&mut application_config, state.path());
    let app = build_application(application_config).await?;

    let target_organization = bootstrap(&app).await?;
    let principal_organization = create_organization(&app).await?;
    let principal_id = create_invited_principal(&app, &principal_organization, "accepted").await?;
    create_invitee_token(
        &app,
        &principal_organization,
        &principal_id,
        INVITEE_TOKEN,
        &["cloud:read", "identity:write"],
        "mi:invitee-token",
    )
    .await?;
    create_invitee_token(
        &app,
        &principal_organization,
        &principal_id,
        INVITEE_READ_ONLY_TOKEN,
        &["cloud:read"],
        "mi:invitee-read-only-token",
    )
    .await?;

    let invitation_collection =
        format!("/api/v1/organizations/{target_organization}/membership-invitations");
    let invalid_lifetime = app
        .call(post_json(
            &invitation_collection,
            "mi:invalid-lifetime",
            json!({
                "principalId": principal_id,
                "role": "restricted",
                "expiresAt": Utc::now() + chrono::Duration::days(31)
            }),
        ))
        .await?;
    assert_eq!(invalid_lifetime.status(), 422);

    let create_body = json!({
        "principalId": principal_id,
        "role": "restricted",
        "expiresAt": Utc::now() + chrono::Duration::days(7)
    });
    let created = app
        .call(post_json(
            &invitation_collection,
            "mi:create",
            create_body.clone(),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created_body = response_json(&created)?;
    let invitation_id = required_string(&created_body["data"]["id"], "invitation ID")?;
    assert_eq!(created_body["data"]["status"], "pending");
    assert_eq!(created_body["data"]["aggregateVersion"], 1);

    let replayed = app
        .call(post_json(
            &invitation_collection,
            "mi:create",
            create_body.clone(),
        ))
        .await?;
    assert_eq!(replayed.status(), 200);
    let replayed_body = response_json(&replayed)?;
    assert_eq!(replayed_body["data"]["id"], invitation_id);
    assert_eq!(replayed_body["data"]["replayed"], true);

    let duplicate_pending = app
        .call(post_json(
            &invitation_collection,
            "mi:create-duplicate-pending",
            create_body,
        ))
        .await?;
    assert_eq!(duplicate_pending.status(), 409);

    let mine = app
        .call(get_as("/api/v1/membership-invitations", INVITEE_TOKEN))
        .await?;
    assert_eq!(mine.status(), 200);
    let mine = response_json(&mine)?;
    assert_eq!(mine["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(mine["data"][0]["id"], invitation_id);

    let acceptance_path = format!("/api/v1/membership-invitations/{invitation_id}/acceptance");
    let read_only_acceptance = app
        .call(post_json_as(
            &acceptance_path,
            "mi:read-only-acceptance",
            json!({"expectedVersion": 1}),
            INVITEE_READ_ONLY_TOKEN,
        ))
        .await?;
    assert_eq!(read_only_acceptance.status(), 403);

    let wrong_principal = app
        .call(post_json(
            &acceptance_path,
            "mi:wrong-principal",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(wrong_principal.status(), 404);

    let stale = app
        .call(post_json_as(
            &acceptance_path,
            "mi:stale-acceptance",
            json!({"expectedVersion": 2}),
            INVITEE_TOKEN,
        ))
        .await?;
    assert_eq!(stale.status(), 409);
    let target_memberships_before_acceptance = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from organization_memberships where organization_id = ",
            )
            .bind(Uuid::parse_str(&target_organization)?)
            .append(" and principal_id = ")
            .bind(Uuid::parse_str(&principal_id)?),
        )
        .await?;
    assert_eq!(target_memberships_before_acceptance, 0);

    let accepted = app
        .call(post_json_as(
            &acceptance_path,
            "mi:accept",
            json!({"expectedVersion": 1}),
            INVITEE_TOKEN,
        ))
        .await?;
    assert_eq!(accepted.status(), 201);
    let accepted_body = response_json(&accepted)?;
    assert_eq!(accepted_body["data"]["invitation"]["status"], "accepted");
    assert_eq!(accepted_body["data"]["invitation"]["aggregateVersion"], 2);
    assert_eq!(accepted_body["data"]["membership"]["role"], "restricted");
    assert_eq!(
        accepted_body["data"]["membership"]["organizationId"],
        target_organization
    );
    let membership_id = required_string(
        &accepted_body["data"]["membership"]["id"],
        "accepted membership ID",
    )?;

    let accepted_replay = app
        .call(post_json_as(
            &acceptance_path,
            "mi:accept",
            json!({"expectedVersion": 1}),
            INVITEE_TOKEN,
        ))
        .await?;
    assert_eq!(accepted_replay.status(), 200);
    let accepted_replay = response_json(&accepted_replay)?;
    assert_eq!(accepted_replay["data"]["replayed"], true);
    assert_eq!(accepted_replay["data"]["membership"]["id"], membership_id);

    let duplicate_membership = app
        .call(post_json(
            &invitation_collection,
            "mi:create-after-acceptance",
            json!({
                "principalId": principal_id,
                "role": "member",
                "expiresAt": Utc::now() + chrono::Duration::days(7)
            }),
        ))
        .await?;
    assert_eq!(duplicate_membership.status(), 409);

    let revoked_principal_id =
        create_invited_principal(&app, &principal_organization, "revoked").await?;
    create_invitee_token(
        &app,
        &principal_organization,
        &revoked_principal_id,
        REVOKED_INVITEE_TOKEN,
        &["identity:write"],
        "mi:revoked-invitee-token",
    )
    .await?;
    let revoked_created = app
        .call(post_json(
            &invitation_collection,
            "mi:create-revoked",
            json!({
                "principalId": revoked_principal_id,
                "role": "member",
                "expiresAt": Utc::now() + chrono::Duration::days(7)
            }),
        ))
        .await?;
    assert_eq!(revoked_created.status(), 201);
    let revoked_invitation_id = required_string(
        &response_json(&revoked_created)?["data"]["id"],
        "revoked invitation ID",
    )?;
    let revocation_path = format!(
        "/api/v1/organizations/{target_organization}/membership-invitations/{revoked_invitation_id}/revocation"
    );
    let revoked = app
        .call(post_json(
            &revocation_path,
            "mi:revoke",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    let revoked = response_json(&revoked)?;
    assert_eq!(revoked["data"]["status"], "revoked");
    assert_eq!(revoked["data"]["aggregateVersion"], 2);
    let revoked_replay = app
        .call(post_json(
            &revocation_path,
            "mi:revoke",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked_replay.status(), 200);
    assert_eq!(response_json(&revoked_replay)?["data"]["replayed"], true);
    let revoked_acceptance = app
        .call(post_json_as(
            format!("/api/v1/membership-invitations/{revoked_invitation_id}/acceptance"),
            "mi:accept-revoked",
            json!({"expectedVersion": 2}),
            REVOKED_INVITEE_TOKEN,
        ))
        .await?;
    assert_eq!(revoked_acceptance.status(), 409);

    let invitation_id = Uuid::parse_str(&invitation_id)?;
    let membership_id = Uuid::parse_str(&membership_id)?;
    let target_organization = Uuid::parse_str(&target_organization)?;
    let principal_id = Uuid::parse_str(&principal_id)?;
    let revoked_invitation_id = Uuid::parse_str(&revoked_invitation_id)?;
    let revoked_principal_id = Uuid::parse_str(&revoked_principal_id)?;
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from membership_invitations where id = ",
            )
            .bind(invitation_id)
            .append(" and organization_id = ")
            .bind(target_organization)
            .append(" and principal_id = ")
            .bind(principal_id)
            .append(" and accepted_membership_id = ")
            .bind(membership_id)
            .append(" and aggregate_version = 2 and accepted_at is not null and revoked_at is null), (select count(*) from organization_memberships where id = ")
            .bind(membership_id)
            .append(" and organization_id = ")
            .bind(target_organization)
            .append(" and principal_id = ")
            .bind(principal_id)
            .append(" and role = 'restricted' and aggregate_version = 1 and revoked_at is null), (select count(*) from audit_records where aggregate_id = ")
            .bind(invitation_id)
            .append(" and action like 'identity.membership-invitation.%'), (select count(*) from outbox_events where aggregate_id = ")
            .bind(invitation_id)
            .append(" and event_key like 'identity.membership-invitation.%'), (select count(*) from idempotency_records where idempotency_key in ('mi:create', 'mi:accept')), (select count(*) from outbox_events where aggregate_id = ")
            .bind(membership_id)
            .append(" and event_key = 'identity.membership.created')"),
        )
        .await?;
    assert_eq!(
        evidence,
        (1, 1, 2, 2, 2, 1),
        "invitation, Membership, audit, Outbox, and idempotency must commit exactly once"
    );
    let revocation_evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64)>(
                "select (select count(*) from membership_invitations where id = ",
            )
            .bind(revoked_invitation_id)
            .append(" and organization_id = ")
            .bind(target_organization)
            .append(" and principal_id = ")
            .bind(revoked_principal_id)
            .append(" and aggregate_version = 2 and accepted_at is null and accepted_membership_id is null and revoked_at is not null), (select count(*) from organization_memberships where organization_id = ")
            .bind(target_organization)
            .append(" and principal_id = ")
            .bind(revoked_principal_id)
            .append("), (select count(*) from audit_records where aggregate_id = ")
            .bind(revoked_invitation_id)
            .append(" and action like 'identity.membership-invitation.%'), (select count(*) from outbox_events where aggregate_id = ")
            .bind(revoked_invitation_id)
            .append(" and event_key like 'identity.membership-invitation.%')"),
        )
        .await?;
    assert_eq!(
        revocation_evidence,
        (1, 0, 2, 2),
        "revocation must remain terminal without creating a Membership"
    );

    let identity_mutation = database
        .execute(
            sql_query::<()>("update membership_invitations set role = 'member' where id = ")
                .bind(invitation_id),
        )
        .await;
    assert!(
        identity_mutation.is_err(),
        "stored invitation identity must be immutable"
    );
    let terminal_mutation = database
        .execute(
            sql_query::<()>(
                "update membership_invitations set aggregate_version = aggregate_version + 1, updated_at = updated_at + interval '1 second', accepted_at = accepted_at + interval '1 second' where id = ",
            )
            .bind(invitation_id),
        )
        .await;
    assert!(
        terminal_mutation.is_err(),
        "accepted invitation history must remain terminal and immutable"
    );
    let deletion = database
        .execute(
            sql_query::<()>("delete from membership_invitations where id = ").bind(invitation_id),
        )
        .await;
    assert!(
        deletion.is_err(),
        "stored invitation history must be immutable"
    );
    let retained = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from membership_invitations where id = ")
                .bind(invitation_id),
        )
        .await?;
    assert_eq!(retained, 1);
    Ok(())
}

async fn bootstrap(app: &ControlPlane) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(
            post_json(
                "/api/v1/bootstrap",
                "mi:bootstrap",
                json!({
                    "organizationName": "Membership Invitation Target",
                    "tokenName": "membership-invitation-owner",
                    "token": ADMIN_TOKEN,
                    "expiresAt": null
                }),
            )
            .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN_VALUE),
        )
        .await?;
    assert_eq!(response.status(), 201);
    required_string(
        &response_json(&response)?["data"]["organization"]["id"],
        "bootstrap organization ID",
    )
}

async fn create_organization(app: &ControlPlane) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            "/api/v1/organizations",
            "mi:principal-organization",
            json!({"name": "Membership Invitation Principal Home"}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    Ok(response_id(&response)?)
}

async fn create_invited_principal(
    app: &ControlPlane,
    organization: &str,
    suffix: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            &format!("mi:principal-membership-{suffix}"),
            json!({"name": format!("Invited automation {suffix}"), "role": "member"}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    required_string(
        &response_json(&response)?["data"]["principalId"],
        "invited principal ID",
    )
}

async fn create_invitee_token(
    app: &ControlPlane,
    organization: &str,
    principal_id: &str,
    token: &str,
    scopes: &[&str],
    idempotency_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            idempotency_key,
            json!({
                "name": idempotency_key,
                "token": token,
                "scopes": scopes,
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    Ok(())
}

fn required_string(value: &Value, label: &str) -> Result<String, Box<dyn std::error::Error>> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} is missing").into())
}
