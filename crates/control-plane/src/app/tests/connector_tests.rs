use super::*;
use crate::modules::connectors::{
    BeginConnectorExecutionDispatch, ConnectorExecutionAttemptBinding, ConnectorExecutionRequest,
    ConnectorExecutionReservation, ConnectorHttpAuthentication, ConnectorHttpDefinition,
    ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
    ConnectorHttpStatusPolicy, IConnectorExecutionAttemptRepository, IConnectorProfileRepository,
    ReserveConnectorExecutionAttempt,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId,
};

const CONNECTOR_TOKEN: &str =
    "a3s_f411111111111111111111111111111111111111111111111111111111111111";
const CONNECTOR_READ_TOKEN: &str =
    "a3s_f422222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn connector_profile_api_is_acl_native_scoped_revisioned_and_replay_safe() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let connector_profiles = Arc::new(InMemoryConnectorProfileRepository::new());
    let connector_execution = Arc::new(InMemoryConnectorExecutionRepository::new());
    let app = build_test_application_with_connector_repositories(
        identity,
        projects,
        connector_profiles.clone(),
        connector_execution.clone(),
    )?;
    let organization = bootstrap_organization(&app, "connector-bootstrap", "Connectors").await?;
    let project = create_project(
        &app,
        &organization,
        "connector-project",
        "Connector project",
    )
    .await?;
    let environment =
        create_connector_environment(&app, &organization, &project, "connector-environment")
            .await?;
    let other_environment =
        create_connector_environment(&app, &organization, &project, "connector-other-environment")
            .await?;

    create_api_token(
        &app,
        &organization,
        "connector-writer-token",
        "connector-writer",
        CONNECTOR_TOKEN,
        &[ApiTokenScope::CONNECTOR_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "connector-reader-token",
        "connector-reader",
        CONNECTOR_READ_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let profiles_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/connector-profiles"
    );
    let initial_acl = connector_acl(1_000)?;
    let create_body = json!({
        "name": "Incident webhook",
        "definitionAcl": initial_acl,
    });
    assert_eq!(
        app.call(post_json_as(
            &profiles_path,
            "connector-create-denied",
            create_body.clone(),
            CONNECTOR_READ_TOKEN,
        ))
        .await?
        .status(),
        403
    );

    let created = app
        .call(post_json_as(
            &profiles_path,
            "connector-create",
            create_body.clone(),
            CONNECTOR_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    let record = &created["data"]["record"];
    assert_eq!(record["profile"]["name"], "Incident webhook");
    assert_eq!(record["profile"]["aggregateVersion"], 1);
    assert_eq!(record["revision"]["revisionNumber"], 1);
    assert_eq!(
        record["revision"]["definitionSchema"],
        "cloud.connector.http.v1"
    );
    assert_eq!(record["revision"]["definitionAcl"], initial_acl);
    assert!(record["revision"].get("endpoint").is_none());
    let profile_id = required_connector_string(&record["profile"]["profileId"], "profile ID")?;
    let initial_revision_id =
        required_connector_string(&record["revision"]["revisionId"], "revision ID")?;

    let replay = app
        .call(post_json_as(
            &profiles_path,
            "connector-create",
            create_body,
            CONNECTOR_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);

    assert_eq!(
        app.call(post_json_as(
            &profiles_path,
            "connector-create",
            json!({"name": "Changed", "definitionAcl": initial_acl}),
            CONNECTOR_TOKEN,
        ))
        .await?
        .status(),
        409
    );
    assert_eq!(
        app.call(get_as(&profiles_path, CONNECTOR_TOKEN))
            .await?
            .status(),
        403
    );
    let listed = app
        .call(get_as(&profiles_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["profileId"], profile_id);
    assert_eq!(
        app.call(get_as(
            format!("{profiles_path}?limit=201"),
            CONNECTOR_READ_TOKEN,
        ))
        .await?
        .status(),
        400
    );

    let profile_path = format!("{profiles_path}/{profile_id}");
    let fetched = app
        .call(get_as(&profile_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(
        response_json(&fetched)?["data"]["profile"]["profileId"],
        profile_id
    );

    let revised_acl = connector_acl(2_000)?;
    let revisions_path = format!("{profile_path}/revisions");
    let revised = app
        .call(post_json_as(
            &revisions_path,
            "connector-revise",
            json!({"expectedVersion": 1, "definitionAcl": revised_acl}),
            CONNECTOR_TOKEN,
        ))
        .await?;
    assert_eq!(revised.status(), 201);
    let revised = response_json(&revised)?;
    assert_eq!(revised["data"]["record"]["profile"]["aggregateVersion"], 2);
    assert_eq!(revised["data"]["record"]["revision"]["revisionNumber"], 2);
    assert_eq!(
        revised["data"]["record"]["revision"]["parentRevisionId"],
        initial_revision_id
    );
    let revised_revision_id = required_connector_string(
        &revised["data"]["record"]["revision"]["revisionId"],
        "revised revision ID",
    )?;

    let history = app
        .call(get_as(&revisions_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(history.status(), 200);
    let history = response_json(&history)?;
    assert_eq!(history["data"].as_array().map(Vec::len), Some(2));
    assert_eq!(history["data"][0]["revisionId"], revised_revision_id);
    assert_eq!(history["data"][1]["revisionId"], initial_revision_id);

    let initial_revision_path = format!("{revisions_path}/{initial_revision_id}");
    let initial = app
        .call(get_as(&initial_revision_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(initial.status(), 200);
    assert_eq!(response_json(&initial)?["data"]["revisionNumber"], 1);

    let revocation_path = format!("{initial_revision_path}/revocation");
    assert_eq!(
        app.call(get_as(&revocation_path, CONNECTOR_READ_TOKEN))
            .await?
            .status(),
        404
    );
    assert_eq!(
        app.call(post_json_as(
            &revocation_path,
            "connector-revoke-denied",
            json!({"reason": "Destination was compromised"}),
            CONNECTOR_READ_TOKEN,
        ))
        .await?
        .status(),
        403
    );
    let revoked = app
        .call(post_json_as(
            &revocation_path,
            "connector-revoke",
            json!({"reason": "Destination was compromised"}),
            CONNECTOR_TOKEN,
        ))
        .await?;
    let revoked_status = revoked.status();
    let revoked = response_json(&revoked)?;
    assert_eq!(
        revoked_status, 201,
        "unexpected revocation response: {revoked}"
    );
    assert_eq!(revoked["data"]["replayed"], false);
    assert_eq!(
        revoked["data"]["revocation"]["revisionId"],
        initial_revision_id
    );
    assert_eq!(
        revoked["data"]["revocation"]["reason"],
        "Destination was compromised"
    );
    assert_eq!(
        app.call(post_json_as(
            &revocation_path,
            "connector-revoke",
            json!({"reason": "Destination was compromised"}),
            CONNECTOR_TOKEN,
        ))
        .await?
        .status(),
        200
    );
    assert_eq!(
        app.call(post_json_as(
            &revocation_path,
            "connector-revoke",
            json!({"reason": "Changed reason"}),
            CONNECTOR_TOKEN,
        ))
        .await?
        .status(),
        409
    );
    assert_eq!(
        app.call(post_json_as(
            &revocation_path,
            "connector-revoke-again",
            json!({"reason": "Destination was compromised"}),
            CONNECTOR_TOKEN,
        ))
        .await?
        .status(),
        409
    );
    let loaded_revocation = app
        .call(get_as(&revocation_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(loaded_revocation.status(), 200);
    assert_eq!(
        response_json(&loaded_revocation)?["data"]["revisionId"],
        initial_revision_id
    );

    let foreign_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/connector-profiles/{profile_id}"
    );
    assert_eq!(
        app.call(get_as(&foreign_path, CONNECTOR_READ_TOKEN))
            .await?
            .status(),
        404
    );

    let organization_id = OrganizationId::from_uuid(connector_uuid(&organization)?);
    let project_id = ProjectId::from_uuid(connector_uuid(&project)?);
    let environment_id = EnvironmentId::from_uuid(connector_uuid(&environment)?);
    let profile_id = ConnectorProfileId::from_uuid(connector_uuid(&profile_id)?);
    let revision_id = ConnectorRevisionId::from_uuid(connector_uuid(&revised_revision_id)?);
    let revision = connector_profiles
        .find_revision(
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision_id,
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .ok_or_else(|| BootError::Internal("Connector test revision is missing".into()))?;
    let attempt_request = ConnectorExecutionRequest::new(
        revision.id,
        Uuid::now_v7(),
        "application/json",
        b"bounded operator recovery".to_vec(),
    )
    .map_err(BootError::Internal)?;
    let reserved_at = canonical_timestamp(Utc::now()) - chrono::Duration::seconds(30);
    let fence = match connector_execution
        .reserve(
            ReserveConnectorExecutionAttempt::new(
                ConnectorExecutionAttemptBinding::from_exact(&revision, &attempt_request)
                    .map_err(BootError::Internal)?,
                Uuid::now_v7(),
                reserved_at,
                reserved_at + chrono::Duration::seconds(30),
            )
            .map_err(BootError::Internal)?,
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
    {
        ConnectorExecutionReservation::Acquired { fence, .. } => fence,
        other => {
            return Err(BootError::Internal(format!(
                "unexpected Connector reservation: {other:?}"
            )))
        }
    };
    let dispatch_started_at = reserved_at + chrono::Duration::seconds(1);
    connector_execution
        .begin_dispatch(
            BeginConnectorExecutionDispatch::new(
                fence,
                dispatch_started_at,
                dispatch_started_at + chrono::Duration::seconds(5),
            )
            .map_err(BootError::Internal)?,
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let attempts_path = format!("{revisions_path}/{revised_revision_id}/execution-attempts");
    let attempt_path = format!("{attempts_path}/{}", attempt_request.attempt_id());
    let resolution_path = format!("{attempt_path}/resolution");
    let unresolved = app
        .call(get_as(&attempts_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(unresolved.status(), 200);
    let unresolved = response_json(&unresolved)?;
    assert_eq!(
        unresolved["data"]["attempts"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(unresolved["data"]["attempts"][0]["state"], "dispatching");
    assert_eq!(
        unresolved["data"]["attempts"][0]["recoveryState"],
        "indeterminate"
    );
    assert!(unresolved["data"]["attempts"][0]
        .get("fenceToken")
        .is_none());
    assert_eq!(
        app.call(get_as(&attempt_path, CONNECTOR_READ_TOKEN))
            .await?
            .status(),
        200
    );
    assert_eq!(
        app.call(post_json_as(
            &resolution_path,
            "connector-attempt-resolution-denied",
            json!({"reason": "Provider outcome could not be established"}),
            CONNECTOR_READ_TOKEN,
        ))
        .await?
        .status(),
        403
    );
    let resolved = app
        .call(post_json_as(
            &resolution_path,
            "connector-attempt-resolution",
            json!({"reason": "  Provider outcome could not be established  "}),
            CONNECTOR_TOKEN,
        ))
        .await?;
    assert_eq!(resolved.status(), 201);
    let resolved = response_json(&resolved)?;
    assert_eq!(resolved["data"]["replayed"], false);
    assert_eq!(
        resolved["data"]["resolution"]["resolution"],
        "indeterminate"
    );
    assert_eq!(
        resolved["data"]["resolution"]["reason"],
        "Provider outcome could not be established"
    );
    assert_eq!(
        app.call(post_json_as(
            &resolution_path,
            "connector-attempt-resolution",
            json!({"reason": "Provider outcome could not be established"}),
            CONNECTOR_TOKEN,
        ))
        .await?
        .status(),
        200
    );
    assert_eq!(
        app.call(post_json_as(
            &resolution_path,
            "connector-attempt-resolution",
            json!({"reason": "Changed conclusion"}),
            CONNECTOR_TOKEN,
        ))
        .await?
        .status(),
        409
    );
    let loaded_resolution = app
        .call(get_as(&resolution_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(loaded_resolution.status(), 200);
    assert_eq!(
        response_json(&loaded_resolution)?["data"]["attemptId"],
        attempt_request.attempt_id().to_string()
    );
    let remaining = app
        .call(get_as(&attempts_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(remaining.status(), 200);
    assert_eq!(
        response_json(&remaining)?["data"]["attempts"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    let terminal = app
        .call(get_as(&attempt_path, CONNECTOR_READ_TOKEN))
        .await?;
    assert_eq!(terminal.status(), 200);
    let terminal = response_json(&terminal)?;
    assert_eq!(terminal["data"]["state"], "terminal");
    assert_eq!(terminal["data"]["recoveryState"], "completed");
    assert_eq!(terminal["data"]["evidenceOutcome"], "indeterminate");
    assert_eq!(terminal["data"]["responseStatus"], Value::Null);
    Ok(())
}

pub(super) async fn create_connector_environment(
    app: &BootApplication,
    organization_id: &str,
    project_id: &str,
    key: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/projects/{project_id}/environments"),
            key,
            json!({"name": key}),
        ))
        .await?;
    if response.status() != 201 {
        return Err(BootError::Internal(format!(
            "failed to create Connector test environment: {}",
            response.status()
        )));
    }
    response_id(&response)
}

pub(super) fn connector_acl(timeout_milliseconds: u64) -> Result<String> {
    ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
        destination: ConnectorHttpDestination::LiteralHttps {
            endpoint: "https://hooks.example.test/a3s-cloud".into(),
        },
        method: ConnectorHttpMethod::Post,
        request_content_type: "application/json".into(),
        maximum_request_bytes: 16 * 1024,
        maximum_response_bytes: 16 * 1024,
        timeout_milliseconds,
        status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
        authentication: ConnectorHttpAuthentication::None,
    })
    .map(|definition| definition.canonical_acl().to_owned())
    .map_err(BootError::Internal)
}

pub(super) fn required_connector_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("Connector response has no {label}")))
}

fn connector_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|error| BootError::Internal(error.to_string()))
}
