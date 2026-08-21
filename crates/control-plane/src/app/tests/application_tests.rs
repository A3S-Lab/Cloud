use super::*;
use crate::modules::applications::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationResponseMode, ApplicationWorkflowBinding,
};
use crate::modules::shared_kernel::domain::{
    Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};

const APPLICATION_READ_TOKEN: &str =
    "a3s_3333333333333333333333333333333333333333333333333333333333333333";
const APPLICATION_WRITE_TOKEN: &str =
    "a3s_4444444444444444444444444444444444444444444444444444444444444444";
const APPLICATION_ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));

#[tokio::test]
async fn applications_are_release_versioned_across_rest_client_contract_and_management_mcp(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "application-bootstrap", "Applications").await?;
    let project =
        create_project(&app, &organization, "application-project", "Applications").await?;
    create_api_token(
        &app,
        &organization,
        "application-read-token",
        "Application reader",
        APPLICATION_READ_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "application-write-token",
        "Application writer",
        APPLICATION_WRITE_TOKEN,
        &[ApiTokenScope::APPLICATION_WRITE],
        None,
    )
    .await?;

    let workflow_fixture =
        super::workflow_tests::semantic_workflow_fixture("Application publication authority")
            .map_err(BootError::Internal)?;
    let workflow = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/workflow-definitions"),
            "application-workflow",
            workflow_fixture.transport.clone(),
        ))
        .await?;
    assert_eq!(workflow.status(), 201);
    let workflow = response_json(&workflow)?;
    let workflow_definition_id = required_uuid(
        &workflow["data"]["workflowDefinition"]["id"],
        "WorkflowDefinition ID",
    )?;
    let workflow_revision_id =
        required_uuid(&workflow["data"]["revision"]["id"], "Workflow revision ID")?;
    let workflow_contract_digest = required_string(
        &workflow["data"]["revision"]["contentDigest"],
        "Workflow content digest",
    )?;
    assert_eq!(
        workflow["data"]["revision"]["payloadSetDigest"].as_str(),
        Some(workflow_fixture.payload_set_digest.as_str())
    );
    assert_eq!(
        workflow["data"]["revision"]["semanticContractSetDigest"].as_str(),
        workflow_fixture.semantic_contract_set_digest.as_deref()
    );

    let first_acl = application_release_acl(
        workflow_definition_id,
        workflow_revision_id,
        &workflow_contract_digest,
        &workflow_fixture,
        'a',
    )?;
    let applications_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/applications");
    let denied_write = app
        .call(post_json_as(
            &applications_path,
            "application-denied-create",
            json!({
                "name": "Support assistant",
                "description": "Project support",
                "releaseAcl": first_acl,
            }),
            APPLICATION_READ_TOKEN,
        ))
        .await?;
    assert_eq!(denied_write.status(), 403);

    let created = app
        .call(post_json(
            &applications_path,
            "application-create",
            json!({
                "name": "Support assistant",
                "description": "Project support",
                "releaseAcl": first_acl,
            }),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(
        created["data"]["record"]["application"]["aggregateVersion"],
        1
    );
    assert_eq!(created["data"]["record"]["release"]["releaseNumber"], 1);
    assert_eq!(
        created["data"]["record"]["release"]["workflowRevisionId"],
        workflow_revision_id.to_string()
    );
    let application_id = required_string(
        &created["data"]["record"]["application"]["applicationId"],
        "Application ID",
    )?;
    let first_release_id = required_string(
        &created["data"]["record"]["release"]["releaseId"],
        "Application release ID",
    )?;

    let ontology = app
        .call(post_acl(
            format!("/api/v1/organizations/{organization}/projects/{project}/ontologies"),
            "application-delivery-ontology",
            APPLICATION_ONTOLOGY_ACL.as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(ontology.status(), 201);
    let ontology = response_json(&ontology)?;
    let ontology_id = required_string(&ontology["data"]["ontology"]["id"], "Ontology ID")?;
    let ontology_revision_id =
        required_string(&ontology["data"]["revision"]["id"], "Ontology revision ID")?;

    let sessions_path = format!("{applications_path}/{application_id}/sessions");
    let opened = app
        .call(post_json_as(
            &sessions_path,
            "application-session-open",
            json!({
                "releaseId": first_release_id,
                "initialVariables": {"locale": "en-US"}
            }),
            APPLICATION_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(opened.status(), 201);
    let opened = response_json(&opened)?;
    assert_eq!(opened["data"]["replayed"], false);
    assert_eq!(opened["data"]["session"]["applicationReleaseNumber"], 1);
    let session_id = required_string(
        &opened["data"]["session"]["sessionId"],
        "Application session ID",
    )?;

    let invocations_path = format!("{sessions_path}/{session_id}/invocations");
    let invocation_request = json!({
        "ontologyId": ontology_id,
        "ontologyRevisionId": ontology_revision_id,
        "responseMode": "blocking",
        "input": {"query": "hello"},
        "timeoutSeconds": 300
    });
    let invoked = app
        .call(post_json_as(
            &invocations_path,
            "application-invocation-request",
            invocation_request.clone(),
            APPLICATION_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(invoked.status(), 201);
    let invoked = response_json(&invoked)?;
    assert_eq!(invoked["data"]["replayed"], false);
    assert_eq!(invoked["data"]["invocation"]["status"], "running");
    assert_eq!(
        invoked["data"]["invocation"]["workflowRunId"],
        invoked["data"]["workflow"]["workflowRunId"]
    );
    let invocation_id = required_string(
        &invoked["data"]["invocation"]["invocationId"],
        "Application invocation ID",
    )?;

    let invocation_replay = app
        .call(post_json_as(
            &invocations_path,
            "application-invocation-request",
            invocation_request,
            APPLICATION_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(invocation_replay.status(), 200);
    assert_eq!(response_json(&invocation_replay)?["data"]["replayed"], true);

    let fetched_session = app
        .call(get_as(
            format!("{sessions_path}/{session_id}"),
            APPLICATION_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched_session.status(), 200);
    assert_eq!(
        response_json(&fetched_session)?["data"]["lastMessageSequence"],
        1
    );
    let fetched_invocation = app
        .call(get_as(
            format!("{invocations_path}/{invocation_id}"),
            APPLICATION_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(fetched_invocation.status(), 200);
    assert_eq!(
        response_json(&fetched_invocation)?["data"]["invocationId"],
        invocation_id
    );
    let messages = app
        .call(get_as(
            format!("{sessions_path}/{session_id}/messages?afterSequence=0&limit=50"),
            APPLICATION_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(messages.status(), 200);
    let messages = response_json(&messages)?;
    assert_eq!(messages["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(messages["data"][0]["kind"], "input");

    let denied_delivery_read = app
        .call(get_as(
            format!("{sessions_path}/{session_id}"),
            APPLICATION_READ_TOKEN,
        ))
        .await?;
    assert_eq!(denied_delivery_read.status(), 403);

    let mcp_session = mcp_call(
        &app,
        20,
        "a3s_cloud_application_sessions_open",
        json!({
            "projectId": project,
            "applicationId": application_id,
            "releaseId": first_release_id,
            "initialVariables": {"channel": "mcp"},
            "idempotencyKey": "application-mcp-session-open"
        }),
    )
    .await?;
    assert_eq!(mcp_session["code"], 201);
    let mcp_session_id = required_string(
        &mcp_session["data"]["session"]["sessionId"],
        "MCP Application session ID",
    )?;
    let mcp_invocation = mcp_call(
        &app,
        21,
        "a3s_cloud_application_invocations_request",
        json!({
            "projectId": project,
            "applicationId": application_id,
            "sessionId": mcp_session_id,
            "ontologyId": ontology_id,
            "ontologyRevisionId": ontology_revision_id,
            "responseMode": "blocking",
            "input": {"query": "from MCP"},
            "timeoutSeconds": 300,
            "idempotencyKey": "application-mcp-invocation"
        }),
    )
    .await?;
    assert_eq!(mcp_invocation["code"], 201);
    let mcp_invocation_id = required_string(
        &mcp_invocation["data"]["invocation"]["invocationId"],
        "MCP Application invocation ID",
    )?;
    let mcp_session_read = mcp_call(
        &app,
        22,
        "a3s_cloud_application_sessions_get",
        json!({
            "projectId": project,
            "applicationId": application_id,
            "sessionId": mcp_session_id
        }),
    )
    .await?;
    assert_eq!(mcp_session_read["data"]["lastMessageSequence"], 1);
    let mcp_invocation_read = mcp_call(
        &app,
        23,
        "a3s_cloud_application_invocations_get",
        json!({
            "projectId": project,
            "applicationId": application_id,
            "sessionId": mcp_session_id,
            "invocationId": mcp_invocation_id
        }),
    )
    .await?;
    assert_eq!(mcp_invocation_read["data"]["status"], "running");
    let mcp_messages = mcp_call(
        &app,
        24,
        "a3s_cloud_application_messages_list",
        json!({
            "projectId": project,
            "applicationId": application_id,
            "sessionId": mcp_session_id,
            "afterSequence": 0,
            "limit": 50
        }),
    )
    .await?;
    assert_eq!(mcp_messages["data"].as_array().map(Vec::len), Some(1));

    let replay = app
        .call(post_json(
            &applications_path,
            "application-create",
            json!({
                "name": "Support assistant",
                "description": "Project support",
                "releaseAcl": first_acl,
            }),
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);

    let denied_read = app
        .call(get_as(
            format!("{applications_path}/{application_id}"),
            APPLICATION_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_read.status(), 403);

    let second_acl = application_release_acl(
        workflow_definition_id,
        workflow_revision_id,
        &workflow_contract_digest,
        &workflow_fixture,
        'b',
    )?;
    let published = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_application_releases_publish",
            json!({
                "projectId": project,
                "applicationId": application_id,
                "expectedVersion": 1,
                "releaseAcl": second_acl,
                "idempotencyKey": "application-mcp-publish"
            }),
            ADMIN_TOKEN,
        ))
        .await?;
    let published = response_json(&published)?;
    assert_eq!(published["result"]["structuredContent"]["code"], 201);
    assert_eq!(
        published["result"]["structuredContent"]["data"]["record"]["release"]["releaseNumber"],
        2
    );

    let listed = mcp_call(
        &app,
        2,
        "a3s_cloud_applications_list",
        json!({"projectId": project, "limit": 50}),
    )
    .await?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["currentReleaseNumber"], 2);

    let fetched = mcp_call(
        &app,
        3,
        "a3s_cloud_applications_get",
        json!({"projectId": project, "applicationId": application_id}),
    )
    .await?;
    assert_eq!(fetched["data"]["currentReleaseNumber"], 2);

    let releases = mcp_call(
        &app,
        4,
        "a3s_cloud_application_releases_list",
        json!({"projectId": project, "applicationId": application_id, "limit": 50}),
    )
    .await?;
    assert_eq!(releases["data"].as_array().map(Vec::len), Some(2));
    assert_eq!(releases["data"][0]["releaseNumber"], 2);
    assert_eq!(releases["data"][1]["releaseNumber"], 1);

    let first_release = mcp_call(
        &app,
        5,
        "a3s_cloud_application_releases_get",
        json!({
            "projectId": project,
            "applicationId": application_id,
            "releaseId": first_release_id
        }),
    )
    .await?;
    assert_eq!(first_release["data"]["releaseNumber"], 1);
    assert_eq!(first_release["data"]["parentReleaseId"], Value::Null);

    let second_application = mcp_call(
        &app,
        6,
        "a3s_cloud_applications_create",
        json!({
            "projectId": project,
            "name": "Workflow console",
            "releaseAcl": first_acl,
            "idempotencyKey": "application-mcp-create"
        }),
    )
    .await?;
    assert_eq!(second_application["code"], 201);
    assert_eq!(
        second_application["data"]["record"]["application"]["description"],
        ""
    );

    let other_project = create_project(
        &app,
        &organization,
        "application-other-project",
        "Other Applications",
    )
    .await?;
    let wrong_project = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{other_project}/applications/{application_id}"
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(wrong_project.status(), 404);
    Ok(())
}

fn application_release_acl(
    workflow_definition_id: Uuid,
    workflow_revision_id: Uuid,
    workflow_contract_digest: &str,
    workflow_fixture: &super::workflow_tests::WorkflowFixture,
    presentation_marker: char,
) -> Result<String> {
    let semantic_digest = workflow_fixture
        .semantic_contract_set_digest
        .as_deref()
        .ok_or_else(|| BootError::Internal("semantic Workflow fixture has no digest".into()))?;
    ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Workflow,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Invocation,
            response_modes: vec![ApplicationResponseMode::Blocking],
        },
        workflow: ApplicationWorkflowBinding {
            workflow_definition_id: WorkflowDefinitionId::from_uuid(workflow_definition_id),
            workflow_revision_id: WorkflowRevisionId::from_uuid(workflow_revision_id),
            workflow_contract_digest: digest(workflow_contract_digest)?,
            workflow_payload_set_digest: digest(&workflow_fixture.payload_set_digest)?,
            workflow_semantic_contract_set_digest: digest(semantic_digest)?,
            input_schema_digest: digest(&workflow_fixture.schema_digest)?,
            output_schema_digest: digest(&workflow_fixture.schema_digest)?,
        },
        presentation_digest: digest(&format!(
            "sha256:{}",
            presentation_marker.to_string().repeat(64)
        ))?,
    })
    .map(|contract| contract.canonical_acl().to_owned())
    .map_err(BootError::Internal)
}

fn digest(value: &str) -> Result<Sha256Digest> {
    Sha256Digest::parse(value).map_err(BootError::Internal)
}

fn required_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("{label} is missing")))
}

fn required_uuid(value: &Value, label: &str) -> Result<Uuid> {
    let value = required_string(value, label)?;
    Uuid::parse_str(&value)
        .map_err(|error| BootError::Internal(format!("{label} is invalid: {error}")))
}

async fn mcp_call(app: &BootApplication, id: u64, name: &str, arguments: Value) -> Result<Value> {
    let response = app
        .call(mcp_tool_call_as(id, name, arguments, ADMIN_TOKEN))
        .await?;
    assert_eq!(response.status(), 200);
    let response = response_json(&response)?;
    assert_eq!(response["result"]["isError"], false);
    Ok(response["result"]["structuredContent"].clone())
}
