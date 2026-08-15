use super::*;
use crate::modules::connectors::{
    ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
    ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy,
};

const CONNECTOR_TOKEN: &str =
    "a3s_f411111111111111111111111111111111111111111111111111111111111111";
const CONNECTOR_READ_TOKEN: &str =
    "a3s_f422222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn connector_profile_api_is_acl_native_scoped_revisioned_and_replay_safe() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
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

    let foreign_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/connector-profiles/{profile_id}"
    );
    assert_eq!(
        app.call(get_as(&foreign_path, CONNECTOR_READ_TOKEN))
            .await?
            .status(),
        404
    );
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
