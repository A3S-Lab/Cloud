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
    let second_subscription_id = response_id(&second_subscription)?;
    assert_ne!(second_subscription_id, first_subscription_id);
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

    let initial_pull_request_body =
        pull_request_body(73, "fedcba9876543210fedcba9876543210fedcba98")?;
    let pull_request = app
        .call(github_webhook_request(
            "pull_request",
            "postgres-subscription-pr-a",
            &initial_pull_request_body,
        ))
        .await?;
    let pull_request_replay = app
        .call(github_webhook_request(
            "pull_request",
            "postgres-subscription-pr-a",
            &initial_pull_request_body,
        ))
        .await?;
    assert_eq!(pull_request.status(), 202);
    assert_eq!(pull_request_replay.status(), 202);
    let pull_request_inbox = database
        .fetch_one_as(
            sql_query::<Value>(
                "select jsonb_build_object('event_kind', event_kind, 'repository_identity', repository_identity, 'branch_name', branch_name, 'commit_sha', commit_sha, 'head_repository_identity', head_repository_identity, 'head_branch_name', head_branch_name, 'pull_request_id', pull_request_id, 'pull_request_number', pull_request_number, 'pull_request_change_kind', pull_request_change_kind, 'pull_request_merged', pull_request_merged, 'provider_created_at', provider_created_at, 'provider_updated_at', provider_updated_at) from source_webhook_inbox where delivery_id = ",
            )
            .bind("postgres-subscription-pr-a"),
        )
        .await?;
    assert_eq!(pull_request_inbox["event_kind"], "pull_request");
    assert_eq!(
        pull_request_inbox["repository_identity"],
        "github:github.com/a3s-lab/cloud"
    );
    assert_eq!(pull_request_inbox["branch_name"], "main");
    assert_eq!(
        pull_request_inbox["head_repository_identity"],
        "github:github.com/contributor/cloud"
    );
    assert_eq!(pull_request_inbox["head_branch_name"], "feature/preview");
    assert_eq!(pull_request_inbox["pull_request_number"], 73);
    assert_eq!(
        pull_request_inbox["pull_request_change_kind"],
        "synchronize"
    );
    assert_eq!(pull_request_inbox["pull_request_merged"], false);
    let pull_request_events = database
        .fetch_all_as(sql_query::<Value>(
            "select payload from outbox_events where event_key = 'source.pull-request-change.committed' order by payload ->> 'source_subscription_id'",
        ))
        .await?
        .rows;
    assert_eq!(pull_request_events.len(), 2);
    let mut published_subscription_ids = pull_request_events
        .iter()
        .filter_map(|payload| payload["source_subscription_id"].as_str())
        .collect::<Vec<_>>();
    published_subscription_ids.sort_unstable();
    let mut expected_subscription_ids = vec![
        first_subscription_id.as_str(),
        second_subscription_id.as_str(),
    ];
    expected_subscription_ids.sort_unstable();
    assert_eq!(published_subscription_ids, expected_subscription_ids);
    for payload in &pull_request_events {
        assert_eq!(payload["kind"], "synchronized");
        assert_eq!(payload["pull_request_number"], 73);
        assert_eq!(payload["base_branch"], "main");
        assert_eq!(payload["head_branch"], "feature/preview");
    }
    let published_pull_request_text = serde_json::to_string(&pull_request_events)?;
    for private_evidence in [
        "postgres-subscription-pr-a",
        "payload_digest",
        "sha256:",
        "signature",
        "raw_payload",
    ] {
        assert!(
            !published_pull_request_text.contains(private_evidence),
            "Sources Published Language leaked {private_evidence}"
        );
    }
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from external_source_revisions",
            ))
            .await?,
        3,
        "pull-request facts must not create Source revisions"
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from source_webhook_deliveries",
            ))
            .await?,
        2,
        "pull-request facts must not create the push revision reservation mechanism"
    );
    let changed_pull_request_body =
        pull_request_body(73, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")?;
    assert_eq!(
        app.call(github_webhook_request(
            "pull_request",
            "postgres-subscription-pr-a",
            &changed_pull_request_body,
        ))
        .await?
        .status(),
        409
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.pull-request-change.committed'",
            ))
            .await?,
        2
    );

    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "create function reject_source_fanout_outbox() returns trigger language plpgsql as $$
               begin
                 if (
                      new.event_key = 'source.revision.accepted'
                      and new.payload ->> 'commit_sha' = 'dddddddddddddddddddddddddddddddddddddddd'
                    ) or (
                      new.event_key = 'source.pull-request-change.committed'
                      and new.payload ->> 'pull_request_number' = '74'
                    ) then
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
    let rollback_pull_request_body =
        pull_request_body(74, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")?;
    let rolled_back_pull_request_fanout = app
        .call(github_webhook_request(
            "pull_request",
            "postgres-subscription-pr-rollback",
            &rollback_pull_request_body,
        ))
        .await?;
    assert_eq!(rolled_back_pull_request_fanout.status(), 500);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from source_webhook_inbox where delivery_id = ",)
                    .bind("postgres-subscription-pr-rollback"),
            )
            .await?,
        0,
        "the authenticated inbox write must roll back with PR fact publication"
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.pull-request-change.committed'",
            ))
            .await?,
        2
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
    let active_only_pull_request_body =
        pull_request_body(75, "cccccccccccccccccccccccccccccccccccccccc")?;
    assert_eq!(
        app.call(github_webhook_request(
            "pull_request",
            "postgres-subscription-pr-active-only",
            &active_only_pull_request_body,
        ))
        .await?
        .status(),
        202
    );
    let active_only_pull_request_events = database
        .fetch_all_as(sql_query::<Value>(
            "select payload from outbox_events where event_key = 'source.pull-request-change.committed' and payload ->> 'pull_request_number' = '75'",
        ))
        .await?
        .rows;
    assert_eq!(active_only_pull_request_events.len(), 1);
    assert_eq!(
        active_only_pull_request_events[0]["source_subscription_id"],
        second_subscription_id
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events where event_key = 'source.pull-request-change.committed'",
            ))
            .await?,
        3,
        "inactive subscriptions must not receive committed pull-request facts"
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

fn pull_request_body(
    pull_request_number: u64,
    head_commit_sha: &str,
) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&json!({
        "action": "synchronize",
        "number": pull_request_number,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42},
        "pull_request": {
            "id": 1_000_000 + pull_request_number,
            "number": pull_request_number,
            "state": "open",
            "merged": false,
            "created_at": "2026-08-24T04:30:00Z",
            "updated_at": "2026-08-24T05:30:00.123456789Z",
            "head": {
                "ref": "feature/preview",
                "sha": head_commit_sha,
                "repo": {
                    "full_name": "contributor/cloud",
                    "html_url": "https://github.com/contributor/cloud"
                }
            },
            "base": {
                "ref": "main",
                "sha": "0123456789abcdef0123456789abcdef01234567",
                "repo": {
                    "full_name": "A3S-Lab/Cloud",
                    "html_url": "https://github.com/A3S-Lab/Cloud"
                }
            }
        },
        "ignored": {"providerFields": true}
    }))
}
