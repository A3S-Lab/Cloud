use super::*;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::inference::domain::repositories::IInferenceUsageRepository;
use crate::modules::inference::domain::{
    AcceptInferenceUsageBatchWrite, InferenceUsageRetentionPolicy, InferenceUsageRetentionSweep,
};
use crate::modules::inference::InMemoryInferenceUsageRepository;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_cloud_contracts::{
    InferenceUsageBatchV1, InferenceUsageCursorV1, InferenceUsageEndpointV1,
    InferenceUsageLifecycleEventV1, InferenceUsageLifecycleKindV1,
    InferenceUsageMeasurementCompletenessV1, InferenceUsageRecordV1,
    InferenceUsageRequestEvidenceV1, InferenceUsageTerminalOutcomeV1,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::time::Duration;

const USAGE_READ_TOKEN: &str =
    "a3s_7777777777777777777777777777777777777777777777777777777777777777";
const USAGE_CLOUD_READ_TOKEN: &str =
    "a3s_8888888888888888888888888888888888888888888888888888888888888888";
const USAGE_RESTRICTED_TOKEN: &str =
    "a3s_9999999999999999999999999999999999999999999999999999999999999999";
const USAGE_MEMBER_TOKEN: &str =
    "a3s_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab";

#[tokio::test]
async fn inference_usage_showback_http_is_scope_visible_and_fail_closed() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let usage = Arc::new(InMemoryInferenceUsageRepository::new());
    let app = build_test_application_with_inference_usage(
        identity,
        projects,
        Arc::clone(&usage),
    )?;
    let organization =
        bootstrap_organization(&app, "usage-showback-http", "Usage showback").await?;
    let project = create_project(
        &app,
        &organization,
        "usage-showback-project",
        "Usage Showback",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "usage-showback-environment",
        "Production",
    )
    .await?;
    let other_environment = create_environment(
        &app,
        &organization,
        &project,
        "usage-showback-other-environment",
        "Staging",
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let request_id = Uuid::from_u128(0x1_0000_0001);

    seed_usage_fact(
        usage.as_ref(),
        organization_id,
        environment_id.as_uuid(),
        request_id,
        "2026-01-02T10:00:00Z",
        "2026-01-02T10:00:01Z",
    )
    .await?;

    create_api_token(
        &app,
        &organization,
        "usage-showback-read-token",
        "usage-showback-read",
        USAGE_READ_TOKEN,
        &[ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "usage-showback-cloud-read-token",
        "usage-showback-cloud-read",
        USAGE_CLOUD_READ_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "usage-showback-restricted-membership",
            json!({"name": "Restricted usage reader", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted principal has no ID".into()))?;
    let restricted_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "usage-showback-restricted-token",
            json!({
                "name": "Restricted usage reader",
                "token": USAGE_RESTRICTED_TOKEN,
                "scopes": [ApiTokenScope::INFERENCE_READ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(restricted_token.status(), 201);
    let grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "usage-showback-restricted-grant",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": project,
                    "environmentId": other_environment
                }
            }),
        ))
        .await?;
    assert_eq!(grant.status(), 201);

    let rollups_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference-usage/daily-rollups?from_day=2026-01-02&to_day=2026-01-02"
    );
    let unauthenticated = app
        .call(BootRequest::new(HttpMethod::Get, &rollups_path))
        .await?;
    assert_eq!(unauthenticated.status(), 401);

    let insufficient_scope = app
        .call(get_as(&rollups_path, USAGE_CLOUD_READ_TOKEN))
        .await?;
    assert_eq!(insufficient_scope.status(), 403);

    let inverted = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference-usage/daily-rollups?from_day=2026-01-03&to_day=2026-01-02"
            ),
            USAGE_READ_TOKEN,
        ))
        .await?;
    assert_eq!(inverted.status(), 422);
    let inverted_body = response_json(&inverted)?;
    assert!(inverted_body["message"]
        .as_str()
        .is_some_and(|message| message.contains("from_day must be <= to_day")));

    let rollups = app.call(get_as(&rollups_path, USAGE_READ_TOKEN)).await?;
    assert_eq!(rollups.status(), 200);
    let rollups = response_json(&rollups)?;
    let items = rollups["data"]
        .as_array()
        .ok_or_else(|| BootError::Internal("daily rollups response is not an array".into()))?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["day"], "2026-01-02");
    assert_eq!(items[0]["environmentId"], environment);
    assert_eq!(items[0]["totalTokens"], 7);
    assert_eq!(items[0]["requestCount"], 1);

    let fact_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference-usage/requests/{request_id}"
    );
    let fact = app.call(get_as(&fact_path, USAGE_READ_TOKEN)).await?;
    assert_eq!(fact.status(), 200);
    let fact = response_json(&fact)?;
    assert_eq!(fact["data"]["requestId"], request_id.to_string());
    assert_eq!(fact["data"]["environmentId"], environment);
    assert_eq!(fact["data"]["totalTokens"], 7);

    let cross_environment = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/inference-usage/requests/{request_id}"
            ),
            USAGE_READ_TOKEN,
        ))
        .await?;
    assert_eq!(cross_environment.status(), 404);

    let missing = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference-usage/requests/{}",
                Uuid::from_u128(0x1_0000_0099)
            ),
            USAGE_READ_TOKEN,
        ))
        .await?;
    assert_eq!(missing.status(), 404);

    let ungranted = app
        .call(get_as(&rollups_path, USAGE_RESTRICTED_TOKEN))
        .await?;
    assert_eq!(
        ungranted.status(),
        403,
        "restricted tokens without environment grant must fail closed before showback"
    );
    Ok(())
}

#[tokio::test]
async fn inference_usage_retention_http_is_admin_only() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let usage = Arc::new(InMemoryInferenceUsageRepository::new());
    let app = build_test_application_with_inference_usage(identity, projects, usage)?;
    let organization =
        bootstrap_organization(&app, "usage-retention-http", "Usage retention").await?;

    let member = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "usage-retention-member",
            json!({"name": "Usage member", "role": "member"}),
        ))
        .await?;
    assert_eq!(member.status(), 201);
    let member_principal = response_json(&member)?["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("member Principal ID is missing".into()))?
        .to_owned();
    let member_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "usage-retention-member-token",
            json!({
                "name": "Usage member",
                "token": USAGE_MEMBER_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ],
                "principalId": member_principal,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(member_token.status(), 201);

    let retention_path =
        format!("/api/v1/organizations/{organization}/inference-usage/retention");
    let retention = app.call(get_as(&retention_path, ADMIN_TOKEN)).await?;
    assert_eq!(retention.status(), 200);
    let retention = response_json(&retention)?;
    assert_eq!(retention["data"]["organizationId"], organization);
    assert_eq!(retention["data"]["retentionMs"], 7_776_000_000_u64);
    assert!(retention["data"]["policyDigest"]
        .as_str()
        .is_some_and(|value| value.starts_with("sha256:")));
    assert_eq!(retention["data"]["version"], 0);

    let member_denied = app
        .call(get_as(&retention_path, USAGE_MEMBER_TOKEN))
        .await?;
    assert_eq!(member_denied.status(), 403);
    Ok(())
}

#[tokio::test]
async fn inference_usage_showback_http_fails_closed_before_records_available_from() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let usage = Arc::new(InMemoryInferenceUsageRepository::new());
    let app = build_test_application_with_inference_usage(
        identity,
        projects,
        Arc::clone(&usage),
    )?;
    let organization =
        bootstrap_organization(&app, "usage-retention-boundary", "Usage boundary").await?;
    let project = create_project(
        &app,
        &organization,
        "usage-boundary-project",
        "Usage Boundary",
    )
    .await?;
    let environment = create_environment(
        &app,
        &organization,
        &project,
        "usage-boundary-environment",
        "Production",
    )
    .await?;
    let organization_id = OrganizationId::from_uuid(parse_uuid(&organization, "organization")?);
    let environment_id = EnvironmentId::from_uuid(parse_uuid(&environment, "environment")?);
    let request_id = Uuid::from_u128(0x1_0000_0002);

    seed_usage_fact(
        usage.as_ref(),
        organization_id,
        environment_id.as_uuid(),
        request_id,
        "2026-01-02T10:00:00Z",
        "2026-01-02T10:00:01Z",
    )
    .await?;

    create_api_token(
        &app,
        &organization,
        "usage-boundary-read-token",
        "usage-boundary-read",
        USAGE_READ_TOKEN,
        &[ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let policy =
        InferenceUsageRetentionPolicy::new(Duration::from_millis(86_400_000)).expect("policy");
    let cutoff = DateTime::parse_from_rfc3339("2026-01-03T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let swept_at = DateTime::parse_from_rfc3339("2026-01-03T01:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let next_scan_at = DateTime::parse_from_rfc3339("2026-01-03T02:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    usage
        .sweep_retention(InferenceUsageRetentionSweep {
            cutoff,
            swept_at,
            next_scan_at,
            policy_digest: policy.digest().clone(),
            organization_batch_size: 10,
            record_batch_size: 100,
        })
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let rollups = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference-usage/daily-rollups?from_day=2026-01-02&to_day=2026-01-02"
            ),
            USAGE_READ_TOKEN,
        ))
        .await?;
    assert_eq!(rollups.status(), 409);

    let purged_fact = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference-usage/requests/{request_id}"
            ),
            USAGE_READ_TOKEN,
        ))
        .await?;
    assert_eq!(purged_fact.status(), 404);
    Ok(())
}

async fn seed_usage_fact(
    usage: &InMemoryInferenceUsageRepository,
    organization_id: OrganizationId,
    environment_id: Uuid,
    request_id: Uuid,
    started_at: &str,
    finished_at: &str,
) -> Result<()> {
    let started = lifecycle_payload(
        InferenceUsageLifecycleKindV1::RequestStarted,
        started_at,
        environment_id,
        request_id,
        false,
    )?;
    let finished = lifecycle_payload(
        InferenceUsageLifecycleKindV1::RequestTerminal,
        finished_at,
        environment_id,
        request_id,
        true,
    )?;
    let epoch = Uuid::from_u128(0x1_0000_00e0);
    usage
        .accept_usage_batch(
            AcceptInferenceUsageBatchWrite::new(
                organization_id,
                NodeId::from_uuid(Uuid::from_u128(0x1_0000_00a0)),
                InferenceUsageBatchV1 {
                    schema: InferenceUsageBatchV1::SCHEMA.into(),
                    gateway_id: Uuid::from_u128(0x1_0000_00b0),
                    batch_id: Uuid::now_v7(),
                    after: None,
                    records: vec![
                        usage_record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 1,
                            },
                            Uuid::now_v7(),
                            &started,
                        ),
                        usage_record(
                            InferenceUsageCursorV1 {
                                boot_epoch: epoch,
                                sequence: 2,
                            },
                            Uuid::now_v7(),
                            &finished,
                        ),
                    ],
                },
                Utc::now(),
            )
            .map_err(|error| BootError::Internal(error.to_string()))?,
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    Ok(())
}

fn lifecycle_payload(
    kind: InferenceUsageLifecycleKindV1,
    at: &str,
    environment_id: Uuid,
    request_id: Uuid,
    terminal: bool,
) -> Result<Vec<u8>> {
    let event = InferenceUsageLifecycleEventV1 {
        schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
        kind,
        occurred_at: DateTime::parse_from_rfc3339(at)
            .map_err(|error| BootError::Internal(error.to_string()))?
            .with_timezone(&Utc),
        request: InferenceUsageRequestEvidenceV1 {
            request_id,
            correlation_id: "corr".into(),
            environment_id,
            credential_id: Uuid::from_u128(0x1_0000_00c0),
            credential_generation: 1,
            route_id: Uuid::from_u128(0x1_0000_00d0),
            route_policy_revision: 1,
            endpoint: InferenceUsageEndpointV1::ChatCompletions,
            model_alias: "alias".into(),
            model_id: Uuid::from_u128(0x1_0000_00f0),
        },
        attempt: None,
        outcome: terminal.then_some(InferenceUsageTerminalOutcomeV1::Succeeded),
        http_status: terminal.then_some(200),
        duration_ms: terminal.then_some(1),
        measurement_completeness: terminal
            .then_some(InferenceUsageMeasurementCompletenessV1::UpstreamUsage),
        total_tokens: terminal.then_some(7),
    };
    serde_json::to_vec(&event).map_err(|error| BootError::Internal(error.to_string()))
}

fn usage_record(
    cursor: InferenceUsageCursorV1,
    event_id: Uuid,
    payload: &[u8],
) -> InferenceUsageRecordV1 {
    InferenceUsageRecordV1 {
        cursor,
        event_id,
        payload_base64: base64::engine::general_purpose::STANDARD.encode(payload),
        payload_sha256: format!("{:x}", Sha256::digest(payload)),
    }
}

async fn create_environment(
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

fn parse_uuid(value: &str, label: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::Internal(format!("invalid {label} ID: {error}")))
}
