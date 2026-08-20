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
