use super::agent_checkpoint_support::{checkpoint_test_binding, checkpoint_test_fork_binding};
use super::*;
use crate::modules::agents::{BindAgentCodeRunWrite, IAgentRepository};
use crate::modules::artifacts::application::project_hosted_build_outcome;
use crate::modules::artifacts::domain::test_support::succeeded_hosted_agent_build;
use crate::modules::assets::domain::{Asset, AssetKind, AssetRelease, AssetReleaseVersion};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentExecutionId, AssetId, AssetReleaseId, GitCommitSha, ResourceName,
    Sha256Digest,
};

const AGENT_WRITER_TOKEN: &str =
    "a3s_a111111111111111111111111111111111111111111111111111111111111111";
const AGENT_READER_TOKEN: &str =
    "a3s_a222222222222222222222222222222222222222222222222222222222222222";
const RESTRICTED_AGENT_TOKEN: &str =
    "a3s_a333333333333333333333333333333333333333333333333333333333333333";

#[tokio::test]
async fn agent_conversation_api_is_tenant_scoped_idempotent_and_stream_ready() -> Result<()> {
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization = bootstrap_organization(&app, "agent-bootstrap", "Agent tenant").await?;
    let project = create_project(&app, &organization, "agent-project", "Agents").await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "agent-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;

    create_api_token(
        &app,
        &organization,
        "agent-writer-token",
        "agent-writer",
        AGENT_WRITER_TOKEN,
        &[ApiTokenScope::EXECUTION_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "agent-reader-token",
        "agent-reader",
        AGENT_READER_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let conversations_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/agent-conversations"
    );
    let denied = app
        .call(post_empty_as(
            &conversations_path,
            "agent-denied",
            AGENT_READER_TOKEN,
        ))
        .await?;
    assert_eq!(denied.status(), 403);

    let created = app
        .call(post_empty_as(
            &conversations_path,
            "agent-conversation-create",
            AGENT_WRITER_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(created["data"]["conversation"]["status"], "active");
    assert_eq!(created["data"]["conversation"]["lastEventSequence"], 0);
    let conversation_id = created["data"]["conversation"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Agent conversation ID is missing".into()))?;

    let replay = app
        .call(post_empty_as(
            &conversations_path,
            "agent-conversation-create",
            AGENT_WRITER_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    let replay = response_json(&replay)?;
    assert_eq!(replay["data"]["replayed"], true);
    assert_eq!(replay["data"]["conversation"]["id"], conversation_id);

    let listed = app
        .call(get_as(&conversations_path, AGENT_READER_TOKEN))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["id"], conversation_id);

    let conversation_path =
        format!("/api/v1/organizations/{organization}/agent-conversations/{conversation_id}");
    let detail = app
        .call(get_as(&conversation_path, AGENT_READER_TOKEN))
        .await?;
    assert_eq!(detail.status(), 200);
    assert_eq!(response_json(&detail)?["data"]["id"], conversation_id);

    let events_path = format!("{conversation_path}/events");
    let events = app.call(get_as(&events_path, AGENT_READER_TOKEN)).await?;
    assert_eq!(events.status(), 200);
    let events = response_json(&events)?;
    assert_eq!(events["data"]["headSequence"], 0);
    assert_eq!(events["data"]["records"], json!([]));
    assert_eq!(events["data"]["nextCursor"], Value::Null);

    let invalid_cursor = app
        .call(get_as(
            format!("{events_path}?cursor=invalid"),
            AGENT_READER_TOKEN,
        ))
        .await?;
    assert_eq!(invalid_cursor.status(), 400);

    let start_path = format!("{conversation_path}/executions");
    let unavailable_release = app
        .call(post_json_as(
            &start_path,
            "agent-execution-unavailable",
            json!({
                "agentAssetId": Uuid::now_v7(),
                "agentAssetReleaseId": Uuid::now_v7(),
                "input": {"prompt": "hello"}
            }),
            AGENT_WRITER_TOKEN,
        ))
        .await?;
    assert_eq!(unavailable_release.status(), 404);

    let unsupported_provider = app
        .call(post_json_as(
            &start_path,
            "agent-execution-unsupported-provider",
            json!({
                "agentAssetId": Uuid::now_v7(),
                "agentAssetReleaseId": Uuid::now_v7(),
                "providerKind": "unknown.provider",
                "input": {"prompt": "hello"}
            }),
            AGENT_WRITER_TOKEN,
        ))
        .await?;
    assert_eq!(unsupported_provider.status(), 422);
    Ok(())
}

#[tokio::test]
async fn restricted_agent_execution_boundaries_resolve_environment_before_reads_mutations_streams_and_replay(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let assets = Arc::new(UnavailableAssetStore::default());
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let agents = Arc::new(InMemoryAgentRepository::new());
    let app = build_test_application_with_agent_repositories(
        identity,
        projects,
        Arc::clone(&assets),
        Arc::clone(&builds),
        agents,
    )?;
    let organization = bootstrap_organization(&app, "agent-grants", "Agent grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "agent-grants-membership",
            json!({"name": "Restricted Agent operator", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id =
        required_agent_string(&membership["data"]["id"], "restricted Agent membership ID")?;
    let principal_id = required_agent_string(
        &membership["data"]["principalId"],
        "restricted Agent principal ID",
    )?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "agent-grants-token",
            json!({
                "name": "Restricted Agent operator",
                "token": RESTRICTED_AGENT_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::EXECUTION_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "agent-granted-project", "Granted").await?;
    let denied_project =
        create_project(&app, &organization, "agent-denied-project", "Denied").await?;
    let fallback_project =
        create_project(&app, &organization, "agent-fallback-project", "Fallback").await?;
    let granted_environment = create_agent_environment(
        &app,
        &organization,
        &granted_project,
        "agent-granted-environment",
        "Granted",
    )
    .await?;
    let denied_environment = create_agent_environment(
        &app,
        &organization,
        &denied_project,
        "agent-denied-environment",
        "Denied",
    )
    .await?;

    let organization_id =
        OrganizationId::from_uuid(parse_agent_uuid(&organization, "Agent organization")?);
    let (asset, release, build) = published_agent_release(organization_id);
    assets.seed_asset(asset.clone());
    assets.seed_release(release.clone());
    builds.seed_build(build).await;

    let granted_collection = format!(
        "/api/v1/organizations/{organization}/projects/{granted_project}/environments/{granted_environment}/agent-conversations"
    );
    let denied_collection = format!(
        "/api/v1/organizations/{organization}/projects/{denied_project}/environments/{denied_environment}/agent-conversations"
    );
    let granted_conversation = create_agent_conversation(
        &app,
        &granted_collection,
        "agent-grants-create-granted",
        ADMIN_TOKEN,
    )
    .await?;
    let denied_conversation = create_agent_conversation(
        &app,
        &denied_collection,
        "agent-grants-create-denied",
        ADMIN_TOKEN,
    )
    .await?;
    let denied_execution = start_agent_execution(
        &app,
        &organization,
        &denied_conversation,
        "agent-grants-start-denied-fixture",
        asset.id,
        release.id,
        ADMIN_TOKEN,
    )
    .await?;

    let resource_grants =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    let environment_grant = app
        .call(post_json(
            &resource_grants,
            "agent-grants-create-environment",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": granted_project,
                    "environmentId": granted_environment
                }
            }),
        ))
        .await?;
    assert_eq!(environment_grant.status(), 201);
    let environment_grant_id = required_agent_string(
        &response_json(&environment_grant)?["data"]["id"],
        "Agent environment grant ID",
    )?;

    assert_eq!(
        app.call(get_as(&granted_collection, RESTRICTED_AGENT_TOKEN))
            .await?
            .status(),
        200
    );
    assert_eq!(
        app.call(post_empty_as(
            &granted_collection,
            "agent-grants-create-restricted",
            RESTRICTED_AGENT_TOKEN,
        ))
        .await?
        .status(),
        201
    );
    assert_eq!(
        app.call(get_as(&denied_collection, RESTRICTED_AGENT_TOKEN))
            .await?
            .status(),
        403
    );
    assert_eq!(
        app.call(post_empty_as(
            &denied_collection,
            "agent-grants-create-restricted-denied",
            RESTRICTED_AGENT_TOKEN,
        ))
        .await?
        .status(),
        403
    );

    let granted_root =
        format!("/api/v1/organizations/{organization}/agent-conversations/{granted_conversation}");
    let denied_root =
        format!("/api/v1/organizations/{organization}/agent-conversations/{denied_conversation}");
    let missing_conversation = Uuid::now_v7();
    let missing_root =
        format!("/api/v1/organizations/{organization}/agent-conversations/{missing_conversation}");
    for suffix in ["", "/executions", "/events"] {
        assert_resource_not_found_equivalent(
            &app,
            get_as(format!("{denied_root}{suffix}"), RESTRICTED_AGENT_TOKEN),
            get_as(format!("{missing_root}{suffix}"), RESTRICTED_AGENT_TOKEN),
        )
        .await?;
    }
    assert_eq!(
        app.call(get_as(&granted_root, RESTRICTED_AGENT_TOKEN))
            .await?
            .status(),
        200
    );

    let granted_events = format!("{granted_root}/events");
    let live_events = app
        .call(agent_live_events_request(
            format!("{granted_events}/stream"),
            RESTRICTED_AGENT_TOKEN,
        ))
        .await?;
    assert_eq!(live_events.status(), 200);
    assert!(live_events.is_streaming());
    assert!(live_events.is_event_stream());
    assert_resource_not_found_equivalent(
        &app,
        agent_live_events_request(
            format!("{denied_root}/events/stream"),
            RESTRICTED_AGENT_TOKEN,
        ),
        agent_live_events_request(
            format!("{missing_root}/events/stream"),
            RESTRICTED_AGENT_TOKEN,
        ),
    )
    .await?;

    let denied_start = || {
        start_agent_execution_request(
            &organization,
            &denied_conversation,
            "agent-grants-start-denied",
            asset.id,
            release.id,
            RESTRICTED_AGENT_TOKEN,
        )
    };
    let missing_start = || {
        start_agent_execution_request(
            &organization,
            &missing_conversation.to_string(),
            "agent-grants-start-denied",
            asset.id,
            release.id,
            RESTRICTED_AGENT_TOKEN,
        )
    };
    assert_resource_not_found_equivalent(&app, denied_start(), missing_start()).await?;

    let start = || {
        start_agent_execution_request(
            &organization,
            &granted_conversation,
            "agent-grants-start-granted",
            asset.id,
            release.id,
            RESTRICTED_AGENT_TOKEN,
        )
    };
    let started = app.call(start()).await?;
    assert_eq!(started.status(), 202);
    let started_body = response_json(&started)?;
    assert_eq!(
        started_body["data"]["execution"]["provider"]["kind"],
        "a3s.code"
    );
    assert_eq!(
        started_body["data"]["execution"]["provider"]["protocol"],
        "a3s.cloud.agent-provider.v1"
    );
    assert!(started_body["data"]["execution"]["provider"]
        .get("profileAcl")
        .is_none());
    let execution_id = required_agent_string(
        &started_body["data"]["execution"]["id"],
        "granted Agent execution ID",
    )?;
    let start_replay = app.call(start()).await?;
    assert_eq!(start_replay.status(), 200);
    assert_eq!(response_json(&start_replay)?["data"]["replayed"], true);
    assert_eq!(
        app.call(get_as(
            format!("{granted_root}/executions"),
            RESTRICTED_AGENT_TOKEN,
        ))
        .await?
        .status(),
        200
    );

    let execution_root =
        format!("/api/v1/organizations/{organization}/agent-executions/{execution_id}");
    let denied_execution_root =
        format!("/api/v1/organizations/{organization}/agent-executions/{denied_execution}");
    let missing_execution = Uuid::now_v7();
    let missing_execution_root =
        format!("/api/v1/organizations/{organization}/agent-executions/{missing_execution}");
    assert_eq!(
        app.call(get_as(&execution_root, RESTRICTED_AGENT_TOKEN))
            .await?
            .status(),
        200
    );
    for suffix in ["", "/changes"] {
        assert_resource_not_found_equivalent(
            &app,
            get_as(
                format!("{denied_execution_root}{suffix}"),
                RESTRICTED_AGENT_TOKEN,
            ),
            get_as(
                format!("{missing_execution_root}{suffix}"),
                RESTRICTED_AGENT_TOKEN,
            ),
        )
        .await?;
    }
    assert_resource_not_found_equivalent(
        &app,
        post_empty_as(
            format!("{denied_execution_root}/cancel"),
            "agent-grants-cancel-denied",
            RESTRICTED_AGENT_TOKEN,
        ),
        post_empty_as(
            format!("{missing_execution_root}/cancel"),
            "agent-grants-cancel-denied",
            RESTRICTED_AGENT_TOKEN,
        ),
    )
    .await?;

    let cancel = || {
        post_empty_as(
            format!("{execution_root}/cancel"),
            "agent-grants-cancel-granted",
            RESTRICTED_AGENT_TOKEN,
        )
    };
    assert_eq!(app.call(cancel()).await?.status(), 202);
    let cancel_replay = app.call(cancel()).await?;
    assert_eq!(cancel_replay.status(), 200);
    assert_eq!(response_json(&cancel_replay)?["data"]["replayed"], true);

    let project_grant = app
        .call(post_json(
            &resource_grants,
            "agent-grants-create-project",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(project_grant.status(), 201);
    let project_grant_id = required_agent_string(
        &response_json(&project_grant)?["data"]["id"],
        "Agent project grant ID",
    )?;
    let fallback_grant = app
        .call(post_json(
            &resource_grants,
            "agent-grants-create-fallback",
            json!({"scope": {"kind": "project", "projectId": fallback_project}}),
        ))
        .await?;
    assert_eq!(fallback_grant.status(), 201);
    let revoked_environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{environment_grant_id}/revocation"
            ),
            "agent-grants-revoke-environment",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked_environment.status(), 200);
    assert_eq!(
        app.call(get_as(&execution_root, RESTRICTED_AGENT_TOKEN))
            .await?
            .status(),
        200,
        "a project grant must cover descendant Agent conversations and executions"
    );

    let revoked_project = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{project_grant_id}/revocation"
            ),
            "agent-grants-revoke-project",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked_project.status(), 200);
    assert_eq!(
        app.call(get_as(&granted_collection, RESTRICTED_AGENT_TOKEN))
            .await?
            .status(),
        403
    );
    assert_resource_not_found_equivalent(
        &app,
        get_as(&execution_root, RESTRICTED_AGENT_TOKEN),
        get_as(&missing_execution_root, RESTRICTED_AGENT_TOKEN),
    )
    .await?;
    assert_resource_not_found_equivalent(&app, start(), missing_start()).await?;
    assert_resource_not_found_equivalent(
        &app,
        cancel(),
        post_empty_as(
            format!("{missing_execution_root}/cancel"),
            "agent-grants-cancel-granted",
            RESTRICTED_AGENT_TOKEN,
        ),
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn agent_checkpoint_and_fork_api_is_immutable_replayable_and_trajectory_bound() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let assets = Arc::new(UnavailableAssetStore::default());
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let agents = Arc::new(InMemoryAgentRepository::new());
    let app = build_test_application_with_agent_repositories(
        identity,
        projects,
        Arc::clone(&assets),
        Arc::clone(&builds),
        agents.clone(),
    )?;
    let organization = bootstrap_organization(
        &app,
        "agent-checkpoint-bootstrap",
        "Agent checkpoint tenant",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "agent-checkpoint-project",
        "Agent checkpoints",
    )
    .await?;
    let environment = create_agent_environment(
        &app,
        &organization,
        &project,
        "agent-checkpoint-environment",
        "Production",
    )
    .await?;
    let organization_id =
        OrganizationId::from_uuid(parse_agent_uuid(&organization, "Agent organization")?);
    let (asset, release, build) = published_agent_release(organization_id);
    assets.seed_asset(asset.clone());
    assets.seed_release(release.clone());
    builds.seed_build(build).await;
    let conversations = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/agent-conversations"
    );
    let conversation = create_agent_conversation(
        &app,
        &conversations,
        "agent-checkpoint-conversation",
        ADMIN_TOKEN,
    )
    .await?;
    let execution = start_agent_execution(
        &app,
        &organization,
        &conversation,
        "agent-checkpoint-execution",
        asset.id,
        release.id,
        ADMIN_TOKEN,
    )
    .await?;
    let execution_id =
        AgentExecutionId::from_uuid(parse_agent_uuid(&execution, "Agent execution")?);
    let execution_aggregate = agents
        .find_execution(organization_id, execution_id)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .ok_or_else(|| BootError::Internal("Agent execution is missing".into()))?;
    agents
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id,
            execution_id,
            binding: checkpoint_test_binding(&execution_aggregate)?,
        })
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let checkpoint_collection =
        format!("/api/v1/organizations/{organization}/agent-executions/{execution}/checkpoints");
    let captured = app
        .call(post_json_as(
            &checkpoint_collection,
            "agent-checkpoint-capture",
            json!({}),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(captured.status(), 201);
    let captured = response_json(&captured)?;
    assert_eq!(captured["data"]["replayed"], false);
    assert_eq!(
        captured["data"]["checkpoint"]["object"]["namespace"],
        "agent-checkpoints"
    );
    assert_eq!(captured["data"]["checkpoint"]["throughEventSequence"], 1);
    let checkpoint_id =
        required_agent_string(&captured["data"]["checkpoint"]["id"], "Agent checkpoint ID")?;
    let checkpoint_digest = captured["data"]["checkpoint"]["object"]["digest"].clone();

    let replay = app
        .call(post_json_as(
            &checkpoint_collection,
            "agent-checkpoint-capture",
            json!({}),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);

    let listed = app
        .call(get_as(
            format!("{checkpoint_collection}?limit=10"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(response_json(&listed)?["data"][0]["id"], checkpoint_id);
    let checkpoint_path = format!("{checkpoint_collection}/{checkpoint_id}");
    assert_eq!(
        app.call(get_as(&checkpoint_path, ADMIN_TOKEN))
            .await?
            .status(),
        200
    );
    let snapshot = app
        .call(get_as(format!("{checkpoint_path}/snapshot"), ADMIN_TOKEN))
        .await?;
    assert_eq!(snapshot.status(), 200);
    let snapshot = response_json(&snapshot)?;
    assert_eq!(snapshot["data"]["events"].as_array().map(Vec::len), Some(1));
    assert_eq!(snapshot["data"]["events"][0]["kind"], "execution_requested");
    let trajectory = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/agent-executions/{execution}/trajectory?limit=10&throughSequence=1"
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(trajectory.status(), 200);
    let trajectory = response_json(&trajectory)?;
    assert_eq!(trajectory["data"]["records"][0]["sequence"], 1);
    assert_eq!(trajectory["data"]["nextCursor"], Value::Null);

    let fork_path = format!("{checkpoint_path}/fork");
    let fork_input = json!({"input": {"prompt": "continue differently"}});
    let forked = app
        .call(post_json_as(
            &fork_path,
            "agent-checkpoint-fork",
            fork_input.clone(),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(forked.status(), 202);
    let forked = response_json(&forked)?;
    assert_eq!(forked["data"]["replayed"], false);
    assert_eq!(
        forked["data"]["execution"]["lineage"]["parentExecutionId"],
        execution
    );
    assert_eq!(
        forked["data"]["execution"]["lineage"]["parentCheckpointId"],
        checkpoint_id
    );
    assert_eq!(
        forked["data"]["execution"]["lineage"]["parentCheckpointDigest"],
        checkpoint_digest
    );
    let fork_execution_id = required_agent_string(
        &forked["data"]["execution"]["id"],
        "Forked Agent execution ID",
    )?;
    let replay = app
        .call(post_json_as(
            &fork_path,
            "agent-checkpoint-fork",
            fork_input,
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(
        response_json(&replay)?["data"]["execution"]["id"],
        fork_execution_id
    );

    let parent_execution = agents
        .find_execution(organization_id, execution_id)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .ok_or_else(|| BootError::Internal("Parent Agent execution is missing".into()))?;
    let parent_binding = parent_execution
        .code
        .as_ref()
        .ok_or_else(|| BootError::Internal("Parent Agent Runtime binding is missing".into()))?
        .clone();
    let fork_execution_uuid = Uuid::parse_str(&fork_execution_id).map_err(|error| {
        BootError::Internal(format!("Forked Agent execution ID is invalid: {error}"))
    })?;
    let fork_execution = agents
        .find_execution(
            organization_id,
            AgentExecutionId::from_uuid(fork_execution_uuid),
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .ok_or_else(|| BootError::Internal("Forked Agent execution is missing".into()))?;
    agents
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id,
            execution_id: fork_execution.id,
            binding: checkpoint_test_fork_binding(&fork_execution, &parent_binding)?,
        })
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    let fork_checkpoint_collection = format!(
        "/api/v1/organizations/{organization}/agent-executions/{fork_execution_id}/checkpoints"
    );
    let fork_checkpoint = app
        .call(post_json_as(
            &fork_checkpoint_collection,
            "agent-nested-checkpoint-capture",
            json!({}),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(fork_checkpoint.status(), 201);
    let fork_checkpoint = response_json(&fork_checkpoint)?;
    assert_eq!(fork_checkpoint["data"]["checkpoint"]["eventCount"], 2);
    let nested_checkpoint_id = required_agent_string(
        &fork_checkpoint["data"]["checkpoint"]["id"],
        "Nested Agent checkpoint ID",
    )?;
    let nested_snapshot = app
        .call(get_as(
            format!("{fork_checkpoint_collection}/{nested_checkpoint_id}/snapshot"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(nested_snapshot.status(), 200);
    let nested_snapshot = response_json(&nested_snapshot)?;
    assert_eq!(
        nested_snapshot["data"]["events"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(nested_snapshot["data"]["events"][0]["sequence"], 1);
    assert_eq!(nested_snapshot["data"]["events"][1]["sequence"], 2);
    let parent = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/agent-executions/{execution}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&parent)?["data"]["lineage"], Value::Null);
    Ok(())
}

async fn create_agent_environment(
    app: &BootApplication,
    organization: &str,
    project: &str,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}

async fn create_agent_conversation(
    app: &BootApplication,
    collection: &str,
    idempotency_key: &str,
    token: &str,
) -> Result<String> {
    let response = app
        .call(post_empty_as(collection, idempotency_key, token))
        .await?;
    assert_eq!(response.status(), 201);
    required_agent_string(
        &response_json(&response)?["data"]["conversation"]["id"],
        "Agent conversation ID",
    )
}

async fn start_agent_execution(
    app: &BootApplication,
    organization: &str,
    conversation: &str,
    idempotency_key: &str,
    asset_id: AssetId,
    release_id: AssetReleaseId,
    token: &str,
) -> Result<String> {
    let response = app
        .call(start_agent_execution_request(
            organization,
            conversation,
            idempotency_key,
            asset_id,
            release_id,
            token,
        ))
        .await?;
    assert_eq!(response.status(), 202);
    required_agent_string(
        &response_json(&response)?["data"]["execution"]["id"],
        "Agent execution ID",
    )
}

fn start_agent_execution_request(
    organization: &str,
    conversation: &str,
    idempotency_key: &str,
    asset_id: AssetId,
    release_id: AssetReleaseId,
    token: &str,
) -> BootRequest {
    post_json_as(
        format!(
            "/api/v1/organizations/{organization}/agent-conversations/{conversation}/executions"
        ),
        idempotency_key,
        json!({
            "agentAssetId": asset_id,
            "agentAssetReleaseId": release_id,
            "input": {"prompt": "authorize this execution"}
        }),
        token,
    )
}

fn agent_live_events_request(path: impl Into<String>, token: &str) -> BootRequest {
    get_as(path, token).with_header("accept", "text/event-stream")
}

fn published_agent_release(
    organization_id: OrganizationId,
) -> (Asset, AssetRelease, crate::modules::artifacts::BuildRun) {
    let drafted_at = canonical_timestamp(Utc::now() - chrono::Duration::minutes(1));
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("authorized-agent").expect("Agent Asset name"),
        AssetKind::Agent,
        drafted_at,
    )
    .expect("Agent Asset");
    let mut release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0").expect("Agent release version"),
        GitCommitSha::parse("a".repeat(40)).expect("Agent commit SHA"),
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("Agent manifest digest"),
        drafted_at,
    )
    .expect("Agent release");
    let build = succeeded_hosted_agent_build(organization_id, asset.id, release.id, drafted_at);
    let outcome = project_hosted_build_outcome(&build)
        .expect("project hosted outcome")
        .expect("successful hosted outcome");
    release
        .publish_from_hosted_build(&asset, &outcome)
        .expect("publish Agent release");
    (asset, release, build)
}

fn required_agent_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("{label} is missing")))
}

fn parse_agent_uuid(value: &str, label: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::Internal(format!("{label} is invalid: {error}")))
}

fn post_empty_as(path: impl Into<String>, idempotency_key: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path)
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
}
