use super::{github_webhook_request, post_json, response_id, response_json};
use a3s_cloud_control_plane::ControlPlane;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use serde_json::{json, Value};
use uuid::Uuid;

pub(super) async fn exercise_source_subscriptions(
    app: &ControlPlane,
    executor: &PostgresExecutor,
    organization_id: &str,
    project_id: &str,
    environment_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = Database::new(PostgresDialect, executor.clone());
    let subscriptions_path = format!(
        "/api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-subscriptions/github"
    );
    let subscription_request = |target: Option<&str>| {
        json!({
            "repository": {
                "provider": "github",
                "url": "https://github.com/A3S-Lab/Cloud.git"
            },
            "branch": "main",
            "recipe": {
                "schema": "a3s.cloud.build-recipe.v1",
                "kind": "dockerfile",
                "contextPath": "./services/api",
                "dockerfilePath": "Dockerfile",
                "target": target,
                "platforms": ["linux/arm64", "linux/amd64"]
            }
        })
    };
    let first_subscription = app
        .call(post_json(
            &subscriptions_path,
            "postgres-source-subscription-api",
            subscription_request(Some("release")),
        ))
        .await?;
    let first_subscription_replay = app
        .call(post_json(
            &subscriptions_path,
            "postgres-source-subscription-api",
            subscription_request(Some("release")),
        ))
        .await?;
    let first_subscription_canonical = app
        .call(post_json(
            &subscriptions_path,
            "postgres-source-subscription-api-canonical",
            subscription_request(Some("release")),
        ))
        .await?;
    assert_eq!(first_subscription.status(), 201);
    assert_eq!(first_subscription_replay.status(), 200);
    assert_eq!(first_subscription_canonical.status(), 200);
    assert_eq!(
        response_id(&first_subscription)?,
        response_id(&first_subscription_replay)?
    );
    assert_eq!(
        response_id(&first_subscription)?,
        response_id(&first_subscription_canonical)?
    );
    let first_subscription_id = response_id(&first_subscription)?;
    let second_subscription = app
        .call(post_json(
            &subscriptions_path,
            "postgres-source-subscription-worker",
            subscription_request(None),
        ))
        .await?;
    assert_eq!(second_subscription.status(), 201);
    assert_ne!(response_id(&second_subscription)?, first_subscription_id);
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from github_repository_subscriptions where status = 'active'",
            ))
            .await?,
        2
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.github-repository-subscription.created'",
            ))
            .await?,
        2
    );

    let subscription_push_body = serde_json::to_vec(&json!({
        "ref": "refs/heads/main",
        "after": "52b6a42b75f7e8405ddb2cab1c8f9c4285302a57",
        "deleted": false,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42}
    }))?;
    let subscription_push = app
        .call(github_webhook_request(
            "push",
            "postgres-subscription-push-a",
            &subscription_push_body,
        ))
        .await?;
    let subscription_push_replay = app
        .call(github_webhook_request(
            "push",
            "postgres-subscription-push-a",
            &subscription_push_body,
        ))
        .await?;
    assert_eq!(subscription_push.status(), 202);
    assert_eq!(subscription_push_replay.status(), 202);
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from external_source_revisions",
            ))
            .await?,
        3
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.revision.accepted'",
            ))
            .await?,
        3
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from source_webhook_deliveries",
            ))
            .await?,
        2
    );
    let changed_subscription_push = serde_json::to_vec(&json!({
        "ref": "refs/heads/main",
        "after": "cccccccccccccccccccccccccccccccccccccccc",
        "deleted": false,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42}
    }))?;
    assert_eq!(
        app.call(github_webhook_request(
            "push",
            "postgres-subscription-push-a",
            &changed_subscription_push,
        ))
        .await?
        .status(),
        409
    );

    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "create function reject_source_fanout_outbox() returns trigger language plpgsql as $$
               begin
                 if new.event_key = 'source.revision.accepted'
                    and new.payload ->> 'commit_sha' = 'dddddddddddddddddddddddddddddddddddddddd' then
                   raise exception 'injected source fanout outbox failure';
                 end if;
                 return new;
               end
             $$;
             create trigger reject_source_fanout_outbox before insert on outbox_events
               for each row execute function reject_source_fanout_outbox();",
        )
        .await?;
    let rollback_push_body = serde_json::to_vec(&json!({
        "ref": "refs/heads/main",
        "after": "dddddddddddddddddddddddddddddddddddddddd",
        "deleted": false,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42}
    }))?;
    let rolled_back_fanout = app
        .call(github_webhook_request(
            "push",
            "postgres-subscription-push-rollback",
            &rollback_push_body,
        ))
        .await?;
    assert_eq!(rolled_back_fanout.status(), 500);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from source_webhook_inbox where delivery_id = ",)
                    .bind("postgres-subscription-push-rollback"),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from external_source_revisions where commit_sha = ",
                )
                .bind("dddddddddddddddddddddddddddddddddddddddd"),
            )
            .await?,
        0
    );
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop trigger reject_source_fanout_outbox on outbox_events;
             drop function reject_source_fanout_outbox();",
        )
        .await?;

    let deactivation_path = format!("{subscriptions_path}/{first_subscription_id}/deactivate");
    let deactivation = app
        .call(post_json(
            &deactivation_path,
            "postgres-source-subscription-api-deactivate",
            json!({}),
        ))
        .await?;
    let deactivation_replay = app
        .call(post_json(
            &deactivation_path,
            "postgres-source-subscription-api-deactivate",
            json!({}),
        ))
        .await?;
    assert_eq!(deactivation.status(), 200);
    assert_eq!(deactivation_replay.status(), 200);
    assert_eq!(response_json(&deactivation)?["data"]["status"], "inactive");
    assert_eq!(
        response_json(&deactivation_replay)?["data"]["replayed"],
        true
    );
    let active_only_push_body = serde_json::to_vec(&json!({
        "ref": "refs/heads/main",
        "after": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "deleted": false,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42}
    }))?;
    assert_eq!(
        app.call(github_webhook_request(
            "push",
            "postgres-subscription-push-active-only",
            &active_only_push_body,
        ))
        .await?
        .status(),
        202
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from external_source_revisions",
            ))
            .await?,
        4
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.revision.accepted'",
            ))
            .await?,
        4
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.github-repository-subscription.deactivated'",
            ))
            .await?,
        1
    );
    let durable_source_state = database
        .fetch_one_as(
            sql_query::<Value>(
                "select jsonb_build_object('subscriptions', coalesce((select jsonb_agg(to_jsonb(subscription)) from github_repository_subscriptions subscription where organization_id = ",
            )
            .bind(Uuid::parse_str(organization_id)?)
            .append("), '[]'::jsonb), 'events', coalesce((select jsonb_agg(payload) from outbox_events where organization_id = ")
            .bind(Uuid::parse_str(organization_id)?)
            .append(" and event_key like 'source.%'), '[]'::jsonb))"),
        )
        .await?;
    let durable_source_text = durable_source_state.to_string().to_ascii_lowercase();
    for forbidden in [
        "access_token",
        "client_secret",
        "private_key",
        "pkce_verifier",
        "password",
    ] {
        assert!(!durable_source_text.contains(forbidden), "{forbidden}");
    }
    Ok(())
}
