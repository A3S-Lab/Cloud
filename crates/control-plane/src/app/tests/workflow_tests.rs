use super::*;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::{
    WorkflowRevisionSemanticContracts, WorkflowStepDescriptorAdmission,
    WorkflowStepDescriptorBinding, WorkflowStepDescriptorBindings,
    WorkflowStepDescriptorBindingsSpec, WorkflowStepDescriptorRegistry,
    WorkflowStepDescriptorRegistrySpec, WorkflowStepDescriptorSpec, WorkflowStepExecutionClass,
    WorkflowStepFailureContract, WorkflowStepFallbackMode, WorkflowStepOwner, WorkflowStepPort,
    WorkflowStepPortCardinality, WorkflowStepPresentationSpec, WorkflowStepRetryClassification,
    WorkflowVariableContract, WorkflowVariableContractSpec, WorkflowVariableDeclaration,
    WorkflowVariableDefault, WorkflowVariableDefaults, WorkflowVariableDefaultsSpec,
    WorkflowVariableMutationMode, WorkflowVariableRead, WorkflowVariableReadMode,
    WorkflowVariableScope, WorkflowVariableStorageClass,
};
use crate::modules::workflow::{
    AssignmentPolicyRef, CapabilityReference, CapabilityType, HumanTaskInteractionSpec,
    WorkflowContract, WorkflowDataSchema, WorkflowDataType, WorkflowEdgeSpec, WorkflowGoalContract,
    WorkflowGoalSpec, WorkflowPayload, WorkflowPayloadContent, WorkflowSpec,
    WorkflowStepConfiguration, WorkflowStepKind, WorkflowStepSpec,
};
use a3s_form_core::{
    digest_interaction_value, parse_json, FormInteractionOutcome, FormInteractionRequest,
    FormInteractionSubmission, FormInteractionSubmissionAssignment, FormReleaseRef,
    FORM_INTERACTION_SUBMISSION_API_VERSION,
};

const ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));
const RESTRICTED_WORKFLOW_TOKEN: &str =
    "a3s_8888888888888888888888888888888888888888888888888888888888888888";

#[tokio::test]
async fn workflow_node_catalog_is_deterministic_project_authorized_and_cross_surface_exact(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "workflow-node-catalog", "Workflow node catalog").await?;
    let project = create_project(
        &app,
        &organization,
        "workflow-node-catalog-project",
        "Workflow node catalog",
    )
    .await?;
    let path =
        format!("/api/v1/organizations/{organization}/projects/{project}/workflow-node-catalog");

    let first = app.call(get_as(&path, ADMIN_TOKEN)).await?;
    assert_eq!(first.status(), 200);
    let first = response_json(&first)?;
    let catalog = first["data"].clone();
    assert_eq!(
        catalog["schema"],
        "a3s.cloud.app-platform.workflow-node-profiles.v1"
    );
    assert_eq!(catalog["revision"], "1.0.0");
    assert_eq!(catalog["baseline"], "2026-08-13");
    assert_eq!(catalog["parityClaim"], false);
    assert!(catalog["parityManifestDigest"]
        .as_str()
        .is_some_and(|value| value.starts_with("sha256:")));
    assert!(catalog["profileSetDigest"]
        .as_str()
        .is_some_and(|value| value.starts_with("sha256:")));
    let nodes = catalog["nodes"]
        .as_array()
        .ok_or_else(|| BootError::Internal("Workflow node catalog has no nodes".into()))?;
    assert_eq!(nodes.len(), 23);
    assert!(nodes
        .windows(2)
        .all(|pair| { pair[0]["capabilityId"].as_str() < pair[1]["capabilityId"].as_str() }));
    assert!(nodes.iter().all(|node| node["availability"] != "public"));
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node["availability"] == "internal")
            .map(|node| node["capabilityId"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        [
            "node.human-input",
            "node.if-else",
            "node.output",
            "node.template",
            "node.user-input",
        ]
    );
    let by_id = |id: &str| {
        nodes
            .iter()
            .find(|node| node["capabilityId"] == id)
            .expect("checked-in node")
    };
    assert_eq!(by_id("node.code")["owner"], "executions");
    assert_eq!(by_id("node.code")["kind"], "execution");
    assert_eq!(by_id("node.http-request")["owner"], "connectors");
    assert_eq!(
        by_id("node.schedule-trigger")["executionClass"],
        "invocation_only"
    );
    assert!(by_id("node.schedule-trigger")["kind"].is_null());
    assert_eq!(
        by_id("node.agent")["semanticProfiles"],
        json!(["agent.classic", "agent.release"])
    );

    let second = app.call(get_as(&path, ADMIN_TOKEN)).await?;
    assert_eq!(second.status(), 200);
    assert_eq!(response_json(&second)?["data"], catalog);

    let mcp = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_workflow_node_catalog_get",
            json!({"projectId": project}),
            ADMIN_TOKEN,
        ))
        .await?;
    let mcp = response_json(&mcp)?;
    assert_eq!(mcp["result"]["structuredContent"]["code"], 200);
    assert_eq!(mcp["result"]["structuredContent"]["data"], catalog);

    let missing_project = Uuid::now_v7();
    let missing = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{missing_project}/workflow-node-catalog"
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(missing.status(), 404);
    let missing_mcp = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_workflow_node_catalog_get",
            json!({"projectId": missing_project}),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(
        response_json(&missing_mcp)?["result"]["structuredContent"]["code"],
        404
    );
    Ok(())
}

#[tokio::test]
async fn human_task_submission_reuses_native_form_and_persists_identity_evidence() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let human_tasks = Arc::new(TestHumanTaskRepository::new(Vec::new()));
    let app =
        build_test_application_with_human_tasks(identity, projects, Arc::clone(&human_tasks))?;
    let organization =
        bootstrap_organization(&app, "human-task-submit", "Human task submit").await?;
    let project = create_project(
        &app,
        &organization,
        "human-task-submit-project",
        "Human task submit",
    )
    .await?;
    let memberships_path = format!("/api/v1/organizations/{organization}/memberships");
    let reviewer = app
        .call(post_json(
            &memberships_path,
            "human-task-submit-reviewer",
            json!({"name": "HumanTask reviewer", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(reviewer.status(), 201);
    let reviewer = response_json(&reviewer)?;
    let membership_id =
        required_string(&reviewer["data"]["id"], "HumanTask reviewer membership ID")?;
    let principal_id = required_string(
        &reviewer["data"]["principalId"],
        "HumanTask claimant principal ID",
    )?;
    let principal_id = PrincipalId::from_uuid(
        Uuid::parse_str(&principal_id)
            .map_err(|error| BootError::Internal(format!("invalid principal ID: {error}")))?,
    );
    let reviewer_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "human-task-submit-reviewer-token",
            json!({
                "name": "HumanTask reviewer",
                "token": RESTRICTED_WORKFLOW_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::WORKFLOW_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(reviewer_token.status(), 201);
    let project_grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "human-task-submit-project-grant",
            json!({"scope": {"kind": "project", "projectId": project}}),
        ))
        .await?;
    assert_eq!(project_grant.status(), 201);

    let form_collection = format!("/api/v1/organizations/{organization}/projects/{project}/forms");
    let created_form = app
        .call(post_json(
            &form_collection,
            "human-task-submit-form",
            super::forms_tests::form_draft("Approval", "Approve this HumanTask", false),
        ))
        .await?;
    assert_eq!(created_form.status(), 201);
    let form_id = required_string(
        &response_json(&created_form)?["data"]["form"]["id"],
        "HumanTask Form ID",
    )?;
    let published_form = app
        .call(
            post_json(
                format!("/api/v1/organizations/{organization}/forms/{form_id}/releases"),
                "human-task-submit-form-release",
                json!({}),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(published_form.status(), 201);
    let release: FormReleaseRef = serde_json::from_value(
        response_json(&published_form)?["data"]["release"]["releaseRef"].clone(),
    )
    .map_err(|error| BootError::Internal(format!("invalid Form release reference: {error}")))?;

    let record = human_task_read_fixture_with_form_release(
        &organization,
        &project,
        Some(principal_id),
        "Review native Form submission",
        Some(release),
    )?;
    let task_id = record.task.id;
    let request = record
        .interaction_request
        .clone()
        .ok_or_else(|| BootError::Internal("claimed HumanTask has no Form request".into()))?;
    human_tasks
        .insert(record)
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let submission = form_submission(&request, principal_id)?;
    let submission_path =
        format!("/api/v1/organizations/{organization}/human-tasks/{task_id}/submission");
    let submit_request = |submission: &FormInteractionSubmission| {
        BootRequest::new(HttpMethod::Post, &submission_path)
            .with_header("content-type", "application/json")
            .with_header(
                "authorization",
                format!("Bearer {RESTRICTED_WORKFLOW_TOKEN}"),
            )
            .with_body(
                serde_json::to_vec(submission).expect("Form interaction submission must serialize"),
            )
    };
    let accepted = app.call(submit_request(&submission)).await?;
    assert_eq!(accepted.status(), 200);
    let accepted = response_json(&accepted)?;
    assert_eq!(accepted["data"]["humanTask"]["status"], "completed");
    assert_eq!(accepted["data"]["replayed"], false);

    let decision_id = WorkflowDecisionId::from_uuid(
        Uuid::parse_str(&required_string(
            &accepted["data"]["humanTask"]["decisionId"],
            "HumanTask decision ID",
        )?)
        .map_err(|error| BootError::Internal(format!("invalid decision ID: {error}")))?,
    );
    let decision = human_tasks
        .find_decision(
            OrganizationId::from_uuid(Uuid::parse_str(&organization).map_err(|error| {
                BootError::Internal(format!("invalid organization ID: {error}"))
            })?),
            decision_id,
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .ok_or_else(|| BootError::Internal("persisted HumanTask decision is missing".into()))?;
    let authorization = &decision
        .submission
        .as_ref()
        .ok_or_else(|| BootError::Internal("persisted Form submission is missing".into()))?
        .authorization_decision;
    assert!(authorization
        .id
        .starts_with("urn:a3s:cloud:identity:resource-authorization-decision:"));
    assert!(authorization.digest.as_str().starts_with("sha256:"));

    let replay = app.call(submit_request(&submission)).await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);
    let mcp_replay = app
        .call(mcp_tool_call_as(
            8,
            "a3s_cloud_human_tasks_submit",
            json!({"humanTaskId": task_id, "submission": submission}),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    let mcp_replay = response_json(&mcp_replay)?;
    assert_eq!(mcp_replay["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        mcp_replay["result"]["structuredContent"]["data"]["replayed"],
        true
    );
    Ok(())
}

#[tokio::test]
async fn human_task_reads_are_bounded_and_only_expose_interactions_to_the_claimant() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let human_tasks = Arc::new(TestHumanTaskRepository::new(Vec::new()));
    let app =
        build_test_application_with_human_tasks(identity, projects, Arc::clone(&human_tasks))?;
    let organization = bootstrap_organization(&app, "human-task-reads", "Human task reads").await?;
    let project = create_project(&app, &organization, "human-task-project", "Human tasks").await?;

    let memberships_path = format!("/api/v1/organizations/{organization}/memberships");
    let owner_memberships = app.call(get_as(&memberships_path, ADMIN_TOKEN)).await?;
    let owner_memberships = response_json(&owner_memberships)?;
    let owner_principal_id = owner_memberships["data"][0]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("HumanTask owner principal has no ID".into()))?
        .to_owned();
    let owner_principal_id =
        PrincipalId::from_uuid(Uuid::parse_str(&owner_principal_id).map_err(|error| {
            BootError::Internal(format!("invalid owner principal ID: {error}"))
        })?);

    let observer = app
        .call(post_json(
            &memberships_path,
            "human-task-observer",
            json!({"name": "HumanTask observer", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(observer.status(), 201);
    let observer = response_json(&observer)?;
    let observer_membership_id = observer["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("HumanTask observer membership has no ID".into()))?;
    let observer_principal_id = observer["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("HumanTask observer principal has no ID".into()))?
        .to_owned();
    let observer_token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "human-task-observer-token",
            json!({
                "name": "HumanTask observer",
                "token": RESTRICTED_WORKFLOW_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::WORKFLOW_WRITE],
                "principalId": observer_principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(observer_token.status(), 201);

    let environment_only_project = create_project(
        &app,
        &organization,
        "human-task-environment-project",
        "Environment only",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{environment_only_project}/environments"
            ),
            "human-task-environment",
            json!({"name": "Environment only"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment_id = response_id(&environment)?;

    let resource_grants = format!(
        "/api/v1/organizations/{organization}/memberships/{observer_membership_id}/resource-grants"
    );
    let project_grant = app
        .call(post_json(
            &resource_grants,
            "human-task-project-grant",
            json!({"scope": {"kind": "project", "projectId": project}}),
        ))
        .await?;
    assert_eq!(project_grant.status(), 201);
    let environment_grant = app
        .call(post_json(
            &resource_grants,
            "human-task-environment-grant",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": environment_only_project,
                    "environmentId": environment_id
                }
            }),
        ))
        .await?;
    assert_eq!(environment_grant.status(), 201);

    let claimed = human_task_read_fixture(
        &organization,
        &project,
        Some(owner_principal_id),
        "Review production change",
    )?;
    let claimed_id = claimed.task.id;
    human_tasks
        .insert(claimed)
        .map_err(|error| BootError::Internal(error.to_string()))?;
    let ready = human_task_read_fixture(&organization, &project, None, "Review staging change")?;
    let ready_id = ready.task.id;
    human_tasks
        .insert(ready)
        .map_err(|error| BootError::Internal(error.to_string()))?;
    let mcp_ready = human_task_read_fixture(&organization, &project, None, "Review MCP change")?;
    let mcp_ready_id = mcp_ready.task.id;
    human_tasks
        .insert(mcp_ready)
        .map_err(|error| BootError::Internal(error.to_string()))?;
    let denied = human_task_read_fixture(
        &organization,
        &environment_only_project,
        None,
        "Review environment-only change",
    )?;
    let denied_id = denied.task.id;
    human_tasks
        .insert(denied)
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let collection = format!("/api/v1/organizations/{organization}/projects/{project}/human-tasks");
    let listed = app
        .call(get_as(
            format!("{collection}?status=claimed&limit=1"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["id"], claimed_id.to_string());
    for private_field in [
        "details",
        "outputMapping",
        "maxValueBytes",
        "initialValue",
        "interactionRequest",
    ] {
        assert!(
            listed["data"][0].get(private_field).is_none(),
            "{private_field}"
        );
    }
    let ready_list = app
        .call(get_as(
            format!("{collection}?status=ready&limit=200"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(
        response_json(&ready_list)?["data"][0]["id"],
        ready_id.to_string()
    );
    assert_eq!(
        app.call(get_as(format!("{collection}?limit=201"), ADMIN_TOKEN))
            .await?
            .status(),
        422
    );
    assert_eq!(
        app.call(get_as(&collection, RESTRICTED_WORKFLOW_TOKEN))
            .await?
            .status(),
        200
    );
    let denied_collection = format!(
        "/api/v1/organizations/{organization}/projects/{environment_only_project}/human-tasks"
    );
    assert_eq!(
        app.call(get_as(&denied_collection, RESTRICTED_WORKFLOW_TOKEN))
            .await?
            .status(),
        403
    );

    let task_path = format!("/api/v1/organizations/{organization}/human-tasks/{claimed_id}");
    let claimant_view = app.call(get_as(&task_path, ADMIN_TOKEN)).await?;
    assert_eq!(claimant_view.status(), 200);
    let claimant_view = response_json(&claimant_view)?;
    assert_eq!(
        claimant_view["data"]["details"],
        "Approve or reject the change"
    );
    assert!(claimant_view["data"]["interactionRequest"].is_object());
    assert_eq!(
        claimant_view["data"]["interactionRequest"]["assignment"]["claimedPrincipalId"],
        owner_principal_id.to_string()
    );

    let observer_view = app
        .call(get_as(&task_path, RESTRICTED_WORKFLOW_TOKEN))
        .await?;
    assert_eq!(observer_view.status(), 200);
    assert!(response_json(&observer_view)?["data"]["interactionRequest"].is_null());

    let ready_task_path = format!("/api/v1/organizations/{organization}/human-tasks/{ready_id}");
    let claim_request = || {
        post_empty_as(
            format!("{ready_task_path}/claim"),
            "human-task-observer-claim",
            RESTRICTED_WORKFLOW_TOKEN,
        )
        .with_header("x-a3s-expected-version", "2")
    };
    let claimed = app.call(claim_request()).await?;
    assert_eq!(claimed.status(), 200);
    let claimed = response_json(&claimed)?;
    assert_eq!(claimed["data"]["replayed"], false);
    assert_eq!(claimed["data"]["humanTask"]["status"], "claimed");
    assert_eq!(
        claimed["data"]["humanTask"]["claimedBy"],
        observer_principal_id
    );
    assert!(claimed["data"]["humanTask"]["interactionRequest"].is_object());
    assert_eq!(
        claimed["data"]["humanTask"]["interactionRequest"]["task"]["version"],
        3
    );
    let claim_replay = app.call(claim_request()).await?;
    assert_eq!(claim_replay.status(), 200);
    assert_eq!(response_json(&claim_replay)?["data"]["replayed"], true);

    let foreign_release = app
        .call(
            post_empty_as(
                format!("{ready_task_path}/release"),
                "human-task-foreign-release",
                ADMIN_TOKEN,
            )
            .with_header("x-a3s-expected-version", "3"),
        )
        .await?;
    assert_eq!(foreign_release.status(), 409);

    let release_request = || {
        post_empty_as(
            format!("{ready_task_path}/release"),
            "human-task-observer-release",
            RESTRICTED_WORKFLOW_TOKEN,
        )
        .with_header("x-a3s-expected-version", "3")
    };
    let released = app.call(release_request()).await?;
    assert_eq!(released.status(), 200);
    let released = response_json(&released)?;
    assert_eq!(released["data"]["humanTask"]["status"], "ready");
    assert!(released["data"]["humanTask"]["claimedBy"].is_null());
    assert!(released["data"]["humanTask"]["interactionRequest"].is_null());
    let release_replay = app.call(release_request()).await?;
    assert_eq!(release_replay.status(), 200);
    assert_eq!(response_json(&release_replay)?["data"]["replayed"], true);

    let changed_claim_replay = app
        .call(
            post_empty_as(
                format!("{ready_task_path}/claim"),
                "human-task-observer-claim",
                RESTRICTED_WORKFLOW_TOKEN,
            )
            .with_header("x-a3s-expected-version", "4"),
        )
        .await?;
    assert_eq!(changed_claim_replay.status(), 409);
    assert_resource_not_found_equivalent(
        &app,
        get_as(
            format!("/api/v1/organizations/{organization}/human-tasks/{denied_id}"),
            RESTRICTED_WORKFLOW_TOKEN,
        ),
        get_as(
            format!(
                "/api/v1/organizations/{organization}/human-tasks/{}",
                Uuid::now_v7()
            ),
            RESTRICTED_WORKFLOW_TOKEN,
        ),
    )
    .await?;

    let mcp_list = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_human_tasks_list",
            json!({"projectId": project, "status": "claimed", "limit": 1}),
            ADMIN_TOKEN,
        ))
        .await?;
    let mcp_list = response_json(&mcp_list)?;
    assert_eq!(mcp_list["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        mcp_list["result"]["structuredContent"]["data"][0]["id"],
        claimed_id.to_string()
    );
    assert!(mcp_list["result"]["structuredContent"]["data"][0]
        .get("interactionRequest")
        .is_none());

    let mcp_observer_view = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_human_tasks_get",
            json!({"humanTaskId": claimed_id}),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    let mcp_observer_view = response_json(&mcp_observer_view)?;
    assert_eq!(
        mcp_observer_view["result"]["structuredContent"]["code"],
        200
    );
    assert!(
        mcp_observer_view["result"]["structuredContent"]["data"]["interactionRequest"].is_null()
    );
    let mcp_claim = app
        .call(mcp_tool_call_as(
            6,
            "a3s_cloud_human_tasks_claim",
            json!({
                "humanTaskId": mcp_ready_id,
                "expectedVersion": 2,
                "idempotencyKey": "human-task-mcp-claim"
            }),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    let mcp_claim = response_json(&mcp_claim)?;
    assert_eq!(mcp_claim["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        mcp_claim["result"]["structuredContent"]["data"]["humanTask"]["claimedBy"],
        observer_principal_id
    );
    assert!(
        mcp_claim["result"]["structuredContent"]["data"]["humanTask"]["interactionRequest"]
            .is_object()
    );
    let mcp_release = app
        .call(mcp_tool_call_as(
            7,
            "a3s_cloud_human_tasks_release",
            json!({
                "humanTaskId": mcp_ready_id,
                "expectedVersion": 3,
                "idempotencyKey": "human-task-mcp-release"
            }),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    let mcp_release = response_json(&mcp_release)?;
    assert_eq!(mcp_release["result"]["structuredContent"]["code"], 200);
    assert_eq!(
        mcp_release["result"]["structuredContent"]["data"]["humanTask"]["status"],
        "ready"
    );
    let mcp_denied_list = app
        .call(mcp_tool_call_as(
            3,
            "a3s_cloud_human_tasks_list",
            json!({"projectId": environment_only_project}),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    assert_eq!(
        response_json(&mcp_denied_list)?["result"]["structuredContent"]["code"],
        404
    );
    let mcp_denied = app
        .call(mcp_tool_call_as(
            4,
            "a3s_cloud_human_tasks_get",
            json!({"humanTaskId": denied_id}),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    let mcp_missing = app
        .call(mcp_tool_call_as(
            5,
            "a3s_cloud_human_tasks_get",
            json!({"humanTaskId": Uuid::now_v7()}),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    assert_mcp_not_found_equivalent(&mcp_denied, &mcp_missing)?;
    Ok(())
}

fn human_task_read_fixture(
    organization: &str,
    project: &str,
    claimant: Option<PrincipalId>,
    message: &str,
) -> Result<HumanTaskRecord> {
    human_task_read_fixture_with_form_release(organization, project, claimant, message, None)
}

fn human_task_read_fixture_with_form_release(
    organization: &str,
    project: &str,
    claimant: Option<PrincipalId>,
    message: &str,
    form_release: Option<FormReleaseRef>,
) -> Result<HumanTaskRecord> {
    let organization_id = OrganizationId::from_uuid(
        Uuid::parse_str(organization)
            .map_err(|error| BootError::Internal(format!("invalid organization ID: {error}")))?,
    );
    let project_id = ProjectId::from_uuid(
        Uuid::parse_str(project)
            .map_err(|error| BootError::Internal(format!("invalid project ID: {error}")))?,
    );
    let (mut task, _) = crate::modules::workflow::test_support::pending_task();
    task.organization_id = organization_id;
    task.project_id = project_id;
    task.form_release.organization_id = organization_id.to_string();
    task.form_release.project_id = project_id.to_string();
    if let Some(form_release) = form_release {
        task.form_release = form_release;
    }
    task.assignment_policy = AssignmentPolicyRef::workflow_organization_member_exclusive()
        .map_err(BootError::Internal)?;
    task.due_at = None;
    task.expires_at = None;
    let interaction = HumanTaskInteractionSpec::approval(
        message,
        Some("Approve or reject the change".into()),
        None,
    )
    .map_err(BootError::Internal)?;
    let mut record = HumanTaskRecord::create(task, interaction, 1, Uuid::now_v7())
        .map_err(BootError::Internal)?;
    record
        .activate(1, crate::modules::workflow::test_support::timestamp(8, 1))
        .map_err(BootError::Internal)?;
    if let Some(claimant) = claimant {
        record
            .claim(
                2,
                claimant,
                crate::modules::workflow::test_support::timestamp(8, 2),
            )
            .map_err(BootError::Internal)?;
    }
    Ok(record)
}

fn form_submission(
    request: &FormInteractionRequest,
    principal_id: PrincipalId,
) -> Result<FormInteractionSubmission> {
    let value = parse_json(br#"{"approved":true}"#)
        .map_err(|error| BootError::Internal(format!("invalid Form value: {error}")))?;
    let value_digest = digest_interaction_value(&value)
        .map_err(|error| BootError::Internal(format!("Form value digest failed: {error}")))?;
    Ok(FormInteractionSubmission {
        api_version: FORM_INTERACTION_SUBMISSION_API_VERSION.into(),
        submission_id: Uuid::now_v7().to_string(),
        request_id: request.request_id.clone(),
        request_digest: request.digest.clone(),
        identity: request.identity.clone(),
        form: request.form.clone(),
        assignment: FormInteractionSubmissionAssignment {
            policy_id: request.assignment.policy_id.clone(),
            policy_revision: request.assignment.policy_revision,
            policy_digest: request.assignment.policy_digest.clone(),
        },
        task_version: request.task.version,
        principal_id: principal_id.to_string(),
        outcome: FormInteractionOutcome::Approve,
        idempotency_key: format!("human-task-submit-{}", request.identity.human_task_id),
        submitted_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        value,
        value_digest,
    })
}

fn post_empty_as(path: impl Into<String>, idempotency_key: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path.into())
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
}

#[tokio::test]
async fn workflow_definition_goal_and_plan_are_versioned_idempotent_and_exact() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization_id = bootstrap_organization(&app, "workflow-bootstrap", "Acme").await?;
    let project_id =
        create_project(&app, &organization_id, "workflow-project", "Automation").await?;

    let ontology = app
        .call(post_acl(
            format!("/api/v1/organizations/{organization_id}/projects/{project_id}/ontologies"),
            "workflow-ontology",
            ONTOLOGY_ACL.as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(ontology.status(), 201);
    let ontology = response_json(&ontology)?;

    let fixture = workflow_fixture("Version one").map_err(BootError::Internal)?;
    let collection = format!(
        "/api/v1/organizations/{organization_id}/projects/{project_id}/workflow-definitions"
    );
    let create_request = || {
        post_json(
            &collection,
            "workflow-definition-create",
            fixture.transport.clone(),
        )
    };
    let created = app.call(create_request()).await?;
    let replayed = app.call(create_request()).await?;
    assert_eq!(created.status(), 201);
    assert_eq!(replayed.status(), 200);
    let created = response_json(&created)?;
    let replayed = response_json(&replayed)?;
    assert_eq!(
        created["data"]["workflowDefinition"]["id"],
        replayed["data"]["workflowDefinition"]["id"]
    );
    assert_eq!(replayed["data"]["replayed"], true);
    assert_eq!(
        created["data"]["revision"]["payloadSetDigest"],
        fixture.payload_set_digest
    );
    assert_eq!(created["data"]["revision"]["payloadCount"], 4);

    let definition_id = required_string(
        &created["data"]["workflowDefinition"]["id"],
        "WorkflowDefinition ID",
    )?;
    let definition_root =
        format!("/api/v1/organizations/{organization_id}/workflow-definitions/{definition_id}");
    let listed = app.call(get_as(&collection, ADMIN_TOKEN)).await?;
    assert_eq!(
        response_json(&listed)?["data"].as_array().map(Vec::len),
        Some(1)
    );
    let fetched = app.call(get_as(&definition_root, ADMIN_TOKEN)).await?;
    assert_eq!(response_json(&fetched)?["data"]["currentRevisionNumber"], 1);

    let revision_two = workflow_fixture("Version two").map_err(BootError::Internal)?;
    let revised = app
        .call(
            post_json(
                format!("{definition_root}/revisions"),
                "workflow-definition-revise",
                revision_two.transport.clone(),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(revised.status(), 201);
    let revised = response_json(&revised)?;
    let revised_replay = app
        .call(
            post_json(
                format!("{definition_root}/revisions"),
                "workflow-definition-revise",
                revision_two.transport.clone(),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(revised_replay.status(), 200);
    let revised_replay = response_json(&revised_replay)?;
    assert_eq!(revised_replay["data"]["replayed"], true);
    assert_eq!(
        revised["data"]["revision"]["id"],
        revised_replay["data"]["revision"]["id"]
    );
    assert_eq!(
        revised_replay["data"]["workflowDefinition"]["currentRevisionNumber"],
        2
    );
    assert_eq!(revised["data"]["revision"]["revisionNumber"], 2);
    let revision_id = required_string(&revised["data"]["revision"]["id"], "Workflow revision ID")?;
    let workflow_digest = required_string(
        &revised["data"]["revision"]["contentDigest"],
        "Workflow revision digest",
    )?;

    let revisions = app
        .call(get_as(format!("{definition_root}/revisions"), ADMIN_TOKEN))
        .await?;
    assert_eq!(
        response_json(&revisions)?["data"]
            .as_array()
            .map(|values| values
                .iter()
                .filter_map(|value| value["revisionNumber"].as_u64())
                .collect::<Vec<_>>()),
        Some(vec![2, 1])
    );
    let revision = app
        .call(get_as(
            format!("{definition_root}/revisions/{revision_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(revision.status(), 200);
    assert!(response_json(&revision)?["data"]["canonicalDefinitionAcl"]
        .as_str()
        .is_some_and(|acl| acl.contains("Version two")));

    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "Resolve support request".into(),
        workflow_definition_id: WorkflowDefinitionId::from_uuid(
            Uuid::parse_str(&definition_id)
                .map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        workflow_revision_id: WorkflowRevisionId::from_uuid(
            Uuid::parse_str(&revision_id)
                .map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        workflow_digest: Sha256Digest::parse(workflow_digest).map_err(BootError::Internal)?,
        ontology_id: OntologyId::from_uuid(required_uuid(
            &ontology["data"]["ontology"]["id"],
            "Ontology ID",
        )?),
        ontology_revision_id: OntologyRevisionId::from_uuid(required_uuid(
            &ontology["data"]["revision"]["id"],
            "Ontology revision ID",
        )?),
        ontology_digest: Sha256Digest::parse(required_string(
            &ontology["data"]["revision"]["contentDigest"],
            "Ontology digest",
        )?)
        .map_err(BootError::Internal)?,
        environment_id: None,
        input: json!({"ticketId": "T-42", "priority": "high"}),
    })
    .map_err(BootError::Internal)?;
    let goal_collection =
        format!("/api/v1/organizations/{organization_id}/projects/{project_id}/workflow-goals");
    let create_goal = || {
        post_acl(
            &goal_collection,
            "workflow-goal-create",
            goal_contract.canonical_acl().as_bytes().to_vec(),
        )
    };
    let goal = app.call(create_goal()).await?;
    let goal_replay = app.call(create_goal()).await?;
    assert_eq!(goal.status(), 201);
    assert_eq!(goal_replay.status(), 200);
    let goal = response_json(&goal)?;
    let goal_replay = response_json(&goal_replay)?;
    assert_eq!(
        goal["data"]["goal"]["id"],
        goal_replay["data"]["goal"]["id"]
    );
    assert_eq!(
        goal["data"]["planRevision"]["id"],
        goal_replay["data"]["planRevision"]["id"]
    );
    assert_eq!(goal_replay["data"]["replayed"], true);
    assert_eq!(
        goal["data"]["planRevision"]["compilerRevision"],
        "cloud.workflow.plan-compiler.v1"
    );
    assert_eq!(
        goal["data"]["planRevision"]["plan"]["steps"]
            .as_array()
            .map(|steps| steps
                .iter()
                .filter_map(|step| step["id"].as_str())
                .collect::<Vec<_>>()),
        Some(vec!["input", "triage", "output"])
    );

    let equivalent_goal = app
        .call(post_acl(
            &goal_collection,
            "workflow-goal-equivalent",
            goal_contract.canonical_acl().as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(equivalent_goal.status(), 201);
    let equivalent_goal = response_json(&equivalent_goal)?;
    assert_ne!(
        goal["data"]["goal"]["id"],
        equivalent_goal["data"]["goal"]["id"]
    );
    assert_ne!(
        goal["data"]["planRevision"]["id"],
        equivalent_goal["data"]["planRevision"]["id"]
    );
    assert_eq!(
        goal["data"]["planRevision"]["digest"],
        equivalent_goal["data"]["planRevision"]["digest"]
    );
    assert_eq!(
        goal["data"]["planRevision"]["canonicalPlan"],
        equivalent_goal["data"]["planRevision"]["canonicalPlan"]
    );

    let goal_id = required_string(&goal["data"]["goal"]["id"], "WorkflowGoal ID")?;
    let plan_revision_id =
        required_string(&goal["data"]["planRevision"]["id"], "Plan revision ID")?;
    let goal_root = format!("/api/v1/organizations/{organization_id}/workflow-goals/{goal_id}");
    let listed_goals = app.call(get_as(&goal_collection, ADMIN_TOKEN)).await?;
    assert_eq!(
        response_json(&listed_goals)?["data"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let fetched_goal = app.call(get_as(&goal_root, ADMIN_TOKEN)).await?;
    assert_eq!(
        response_json(&fetched_goal)?["data"]["planRevisionId"],
        plan_revision_id
    );
    let fetched_plan = app
        .call(get_as(
            format!("{goal_root}/plan-revisions/{plan_revision_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(fetched_plan.status(), 200);
    assert_eq!(
        response_json(&fetched_plan)?["data"]["digest"],
        goal["data"]["goal"]["planDigest"]
    );

    let run_collection =
        format!("/api/v1/organizations/{organization_id}/projects/{project_id}/workflow-runs");
    let start_body = json!({
        "workflowGoalId": goal_id,
        "planRevisionId": plan_revision_id,
        "timeoutSeconds": 60
    });
    let start_run = || post_json(&run_collection, "workflow-run-start", start_body.clone());
    let started = app.call(start_run()).await?;
    let replayed_start = app.call(start_run()).await?;
    assert_eq!(started.status(), 202);
    assert_eq!(replayed_start.status(), 200);
    let started = response_json(&started)?;
    let replayed_start = response_json(&replayed_start)?;
    let run_id = required_string(&started["data"]["workflowRun"]["id"], "WorkflowRun ID")?;
    assert_eq!(
        started["data"]["workflowRun"]["operationId"],
        started["data"]["workflowRun"]["id"]
    );
    assert_eq!(started["data"]["workflowRun"]["status"], "pending");
    assert_eq!(
        started["data"]["workflowRun"]["steps"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(replayed_start["data"]["replayed"], true);
    assert_eq!(replayed_start["data"]["workflowRun"]["id"], run_id);

    let conflicting_start = app
        .call(post_json(
            &run_collection,
            "workflow-run-start",
            json!({
                "workflowGoalId": goal_id,
                "planRevisionId": plan_revision_id,
                "timeoutSeconds": 61
            }),
        ))
        .await?;
    assert_eq!(conflicting_start.status(), 409);
    let missing_plan = app
        .call(post_json(
            &run_collection,
            "workflow-run-missing-plan",
            json!({
                "workflowGoalId": goal_id,
                "planRevisionId": Uuid::now_v7(),
                "timeoutSeconds": 60
            }),
        ))
        .await?;
    assert_eq!(missing_plan.status(), 404);

    let run_root = format!("/api/v1/organizations/{organization_id}/workflow-runs/{run_id}");
    let listed_runs = app
        .call(get_as(format!("{run_collection}?limit=1"), ADMIN_TOKEN))
        .await?;
    assert_eq!(listed_runs.status(), 200);
    assert_eq!(response_json(&listed_runs)?["data"][0]["id"], run_id);
    let invalid_list = app
        .call(get_as(format!("{run_collection}?limit=201"), ADMIN_TOKEN))
        .await?;
    assert_eq!(invalid_list.status(), 422);
    let fetched_run = app.call(get_as(&run_root, ADMIN_TOKEN)).await?;
    assert_eq!(response_json(&fetched_run)?["data"]["id"], run_id);
    let waited_run = app
        .call(get_as(
            format!("{run_root}/wait?timeoutSeconds=0"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&waited_run)?["data"]["status"], "pending");
    let pending_history = app
        .call(get_as(
            format!("{run_root}/history?afterSequence=0&limit=10"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(pending_history.status(), 200);
    assert_eq!(
        response_json(&pending_history)?["data"]["events"],
        json!([])
    );
    let pending_diagnostics = app
        .call(get_as(format!("{run_root}/diagnostics"), ADMIN_TOKEN))
        .await?;
    assert_eq!(pending_diagnostics.status(), 200);
    let pending_diagnostics = response_json(&pending_diagnostics)?;
    assert_eq!(
        pending_diagnostics["data"]["schema"],
        "cloud.workflow-run.diagnostics.v1"
    );
    assert_eq!(pending_diagnostics["data"]["observedFlowStatus"], "missing");
    assert_eq!(
        pending_diagnostics["data"]["diagnostics"][0]["code"],
        "flow_history_missing"
    );
    let pending_output = app
        .call(get_as(format!("{run_root}/output"), ADMIN_TOKEN))
        .await?;
    assert_eq!(pending_output.status(), 409);

    let cancel_body = json!({"reason": "operator request"});
    let cancel_run = || {
        post_json(
            format!("{run_root}/cancel"),
            "workflow-run-cancel",
            cancel_body.clone(),
        )
    };
    let cancelled = app.call(cancel_run()).await?;
    let cancelled_replay = app.call(cancel_run()).await?;
    assert_eq!(cancelled.status(), 202);
    assert_eq!(cancelled_replay.status(), 200);
    let cancelled = response_json(&cancelled)?;
    assert_eq!(cancelled["data"]["workflowRun"]["status"], "cancelling");
    assert!(cancelled["data"]["workflowRun"]["cancellationRequestedBy"].is_string());
    assert_eq!(
        cancelled["data"]["workflowRun"]["cancellationReason"],
        "operator request"
    );
    assert_eq!(response_json(&cancelled_replay)?["data"]["replayed"], true);
    let conflicting_cancel = app
        .call(post_json(
            format!("{run_root}/cancel"),
            "workflow-run-cancel",
            json!({"reason": "different reason"}),
        ))
        .await?;
    assert_eq!(conflicting_cancel.status(), 409);
    Ok(())
}

#[tokio::test]
async fn user_authored_legacy_provider_publication_fails_closed_for_create_and_revise() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "workflow-provider-admission", "Provider admission").await?;
    let project = create_project(
        &app,
        &organization,
        "workflow-provider-admission-project",
        "Provider admission",
    )
    .await?;
    let collection =
        format!("/api/v1/organizations/{organization}/projects/{project}/workflow-definitions");
    let provider =
        legacy_provider_workflow_transport(WorkflowStepKind::Agent, CapabilityType::AgentRelease)
            .map_err(BootError::Internal)?;

    let rejected_create = app
        .call(post_json(
            &collection,
            "workflow-provider-create-rejected",
            provider.clone(),
        ))
        .await?;
    assert_eq!(rejected_create.status(), 422);

    let valid = app
        .call(post_json(
            &collection,
            "workflow-provider-valid-base",
            workflow_fixture("Provider admission base")
                .map_err(BootError::Internal)?
                .transport,
        ))
        .await?;
    assert_eq!(valid.status(), 201);
    let valid = response_json(&valid)?;
    let definition_id = required_string(
        &valid["data"]["workflowDefinition"]["id"],
        "WorkflowDefinition ID",
    )?;
    let definition_root =
        format!("/api/v1/organizations/{organization}/workflow-definitions/{definition_id}");

    let rejected_revise = app
        .call(
            post_json(
                format!("{definition_root}/revisions"),
                "workflow-provider-revise-rejected",
                provider,
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(rejected_revise.status(), 422);
    let unchanged = app.call(get_as(definition_root, ADMIN_TOKEN)).await?;
    assert_eq!(
        response_json(&unchanged)?["data"]["currentRevisionNumber"],
        1
    );
    Ok(())
}

#[tokio::test]
async fn workflow_semantic_contracts_publish_restore_compile_and_create_v2_runs() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "workflow-semantics", "Semantic workflow").await?;
    let project = create_project(
        &app,
        &organization,
        "workflow-semantics-project",
        "Semantic workflow",
    )
    .await?;
    let ontology = app
        .call(post_acl(
            format!("/api/v1/organizations/{organization}/projects/{project}/ontologies"),
            "workflow-semantics-ontology",
            ONTOLOGY_ACL.as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(ontology.status(), 201);
    let ontology = response_json(&ontology)?;

    let fixture = semantic_workflow_fixture("Semantic Plan v2").map_err(BootError::Internal)?;
    let collection =
        format!("/api/v1/organizations/{organization}/projects/{project}/workflow-definitions");
    let created = app
        .call(post_json(
            &collection,
            "workflow-semantics-create",
            fixture.transport.clone(),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    let revision = &created["data"]["revision"];
    assert_eq!(revision["compilerSchemaVersion"], 2);
    assert_eq!(revision["semanticContractCount"], 4);
    let semantic_contract_set_digest = fixture
        .semantic_contract_set_digest
        .as_deref()
        .ok_or_else(|| BootError::Internal("semantic fixture has no digest".into()))?;
    assert_eq!(
        revision["semanticContractSetDigest"],
        semantic_contract_set_digest
    );
    assert_eq!(
        revision["semanticContracts"]
            .as_array()
            .map(|contracts| contracts
                .iter()
                .filter_map(|contract| contract["kind"].as_str())
                .collect::<Vec<_>>()),
        Some(vec![
            "descriptor_bindings",
            "descriptor_registry",
            "variable_contract",
            "variable_defaults"
        ])
    );
    let definition_id = required_string(
        &created["data"]["workflowDefinition"]["id"],
        "WorkflowDefinition ID",
    )?;
    let revision_id = required_string(&revision["id"], "Workflow revision ID")?;
    let workflow_digest = required_string(&revision["contentDigest"], "Workflow digest")?;
    let fetched = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/workflow-definitions/{definition_id}/revisions/{revision_id}"
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(response_json(&fetched)?["data"]["semanticContractCount"], 4);

    let ontology_id = OntologyId::from_uuid(required_uuid(
        &ontology["data"]["ontology"]["id"],
        "Ontology ID",
    )?);
    let ontology_revision_id = OntologyRevisionId::from_uuid(required_uuid(
        &ontology["data"]["revision"]["id"],
        "Ontology revision ID",
    )?);
    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: "Compile semantic plan".into(),
        workflow_definition_id: WorkflowDefinitionId::from_uuid(required_uuid(
            &created["data"]["workflowDefinition"]["id"],
            "WorkflowDefinition ID",
        )?),
        workflow_revision_id: WorkflowRevisionId::from_uuid(required_uuid(
            &revision["id"],
            "Workflow revision ID",
        )?),
        workflow_digest: Sha256Digest::parse(workflow_digest).map_err(BootError::Internal)?,
        ontology_id,
        ontology_revision_id,
        ontology_digest: Sha256Digest::parse(required_string(
            &ontology["data"]["revision"]["contentDigest"],
            "Ontology digest",
        )?)
        .map_err(BootError::Internal)?,
        environment_id: None,
        input: json!({"ticketId": "T-42"}),
    })
    .map_err(BootError::Internal)?;
    let goal_collection =
        format!("/api/v1/organizations/{organization}/projects/{project}/workflow-goals");
    let goal = app
        .call(post_acl(
            &goal_collection,
            "workflow-semantics-goal",
            goal_contract.canonical_acl().as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(goal.status(), 201);
    let goal = response_json(&goal)?;
    let plan = &goal["data"]["planRevision"];
    assert_eq!(plan["schema"], "cloud.workflow.plan.v2");
    assert_eq!(plan["compilerRevision"], "cloud.workflow.plan-compiler.v2");
    assert_eq!(
        plan["plan"]["semanticContractSetDigest"],
        semantic_contract_set_digest
    );
    assert!(plan["plan"]["variableContractDigest"].is_string());
    assert!(plan["plan"]["steps"]
        .as_array()
        .is_some_and(|steps| steps.iter().all(|step| step["descriptor"].is_object())));

    let run = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/workflow-runs"),
            "workflow-semantics-run",
            json!({
                "workflowGoalId": goal["data"]["goal"]["id"],
                "planRevisionId": plan["id"],
                "timeoutSeconds": 60
            }),
        ))
        .await?;
    assert_eq!(run.status(), 202);
    let run = response_json(&run)?;
    assert_eq!(run["data"]["workflowRun"]["status"], "pending");
    assert_eq!(run["data"]["workflowRun"]["planRevisionId"], plan["id"]);
    let workflow_run_id = required_string(&run["data"]["workflowRun"]["id"], "WorkflowRun ID")?;
    let variables = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization}/workflow-runs/{workflow_run_id}/variables"
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(variables.status(), 200);
    let variables = response_json(&variables)?;
    assert_eq!(
        variables["data"]["schema"],
        "cloud.workflow-run.variable-inspection.v1"
    );
    assert_eq!(variables["data"]["workflowRunId"], workflow_run_id);
    assert_eq!(variables["data"]["planRevisionId"], plan["id"]);
    assert_eq!(
        variables["data"]["variableContractDigest"],
        plan["plan"]["variableContractDigest"]
    );
    assert_eq!(variables["data"]["lastFlowSequence"], 0);
    let projected = variables["data"]["variables"]
        .as_array()
        .ok_or_else(|| BootError::Internal("Workflow variables are not an array".into()))?;
    let request = projected
        .iter()
        .find(|variable| variable["name"] == "request")
        .ok_or_else(|| BootError::Internal("request variable is missing".into()))?;
    assert_eq!(request["state"], "materialized");
    assert_eq!(request["value"], json!({"ticketId": "T-42"}));
    assert!(request["valueDigest"].is_string());
    let fallback = projected
        .iter()
        .find(|variable| variable["name"] == "fallback")
        .ok_or_else(|| BootError::Internal("fallback variable is missing".into()))?;
    assert_eq!(fallback["state"], "materialized");
    assert_eq!(fallback["value"], "normal");
    assert!(fallback["valueDigest"].is_string());
    let mcp_variables = app
        .call(mcp_tool_call_as(
            901,
            "a3s_cloud_workflow_run_variables_get",
            json!({"workflowRunId": workflow_run_id}),
            ADMIN_TOKEN,
        ))
        .await?;
    let mcp_variables = response_json(&mcp_variables)?;
    assert_eq!(
        mcp_variables["result"]["structuredContent"]["data"],
        variables["data"]
    );

    let downgrade = app
        .call(
            post_json(
                format!(
                    "/api/v1/organizations/{organization}/workflow-definitions/{definition_id}/revisions"
                ),
                "workflow-semantics-downgrade",
                workflow_fixture("Attempted downgrade")
                    .map_err(BootError::Internal)?
                    .transport,
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(downgrade.status(), 422);
    Ok(())
}

#[tokio::test]
async fn restricted_workflow_access_resolves_project_before_reads_mutations_and_replay(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "workflow-grants", "Workflow grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "workflow-grants-membership",
            json!({"name": "Restricted Workflow operator", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Workflow membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Workflow principal has no ID".into()))?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "workflow-grants-token",
            json!({
                "name": "Restricted Workflow operator",
                "token": RESTRICTED_WORKFLOW_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::WORKFLOW_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "workflow-granted-project", "Granted").await?;
    let environment_only_project = create_project(
        &app,
        &organization,
        "workflow-environment-project",
        "Environment only",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{environment_only_project}/environments"
            ),
            "workflow-environment",
            json!({"name": "Environment only"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment_id = response_id(&environment)?;

    let granted =
        create_workflow_access_fixture(&app, &organization, &granted_project, "workflow-granted")
            .await?;
    let denied = create_workflow_access_fixture(
        &app,
        &organization,
        &environment_only_project,
        "workflow-environment",
    )
    .await?;

    let resource_grants =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    let project_grant = app
        .call(post_json(
            &resource_grants,
            "workflow-grants-create-project",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(project_grant.status(), 201);
    let project_grant_id = response_json(&project_grant)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Workflow Resource Grant has no ID".into()))?
        .to_owned();
    let environment_grant = app
        .call(post_json(
            &resource_grants,
            "workflow-grants-create-environment",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": environment_only_project,
                    "environmentId": environment_id
                }
            }),
        ))
        .await?;
    assert_eq!(environment_grant.status(), 201);

    let granted_collections = [
        format!(
            "/api/v1/organizations/{organization}/projects/{granted_project}/workflow-node-catalog"
        ),
        format!(
            "/api/v1/organizations/{organization}/projects/{granted_project}/workflow-definitions"
        ),
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/workflow-goals"),
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/workflow-runs"),
    ];
    let denied_collections = [
        format!(
            "/api/v1/organizations/{organization}/projects/{environment_only_project}/workflow-node-catalog"
        ),
        format!(
            "/api/v1/organizations/{organization}/projects/{environment_only_project}/workflow-definitions"
        ),
        format!(
            "/api/v1/organizations/{organization}/projects/{environment_only_project}/workflow-goals"
        ),
        format!(
            "/api/v1/organizations/{organization}/projects/{environment_only_project}/workflow-runs"
        ),
    ];
    for path in granted_collections {
        assert_eq!(
            app.call(get_as(path, RESTRICTED_WORKFLOW_TOKEN))
                .await?
                .status(),
            200
        );
    }
    for path in denied_collections {
        assert_eq!(
            app.call(get_as(path, RESTRICTED_WORKFLOW_TOKEN))
                .await?
                .status(),
            403
        );
    }

    let granted_definition_root = format!(
        "/api/v1/organizations/{organization}/workflow-definitions/{}",
        granted.definition_id
    );
    let granted_goal_root = format!(
        "/api/v1/organizations/{organization}/workflow-goals/{}",
        granted.goal_id
    );
    let granted_run_root = format!(
        "/api/v1/organizations/{organization}/workflow-runs/{}",
        granted.run_id
    );
    for path in [
        granted_definition_root.clone(),
        format!("{granted_definition_root}/revisions"),
        format!(
            "{granted_definition_root}/revisions/{}",
            granted.revision_id
        ),
        granted_goal_root.clone(),
        format!(
            "{granted_goal_root}/plan-revisions/{}",
            granted.plan_revision_id
        ),
        granted_run_root.clone(),
        format!("{granted_run_root}/wait?timeoutSeconds=0"),
        format!("{granted_run_root}/diagnostics"),
        format!("{granted_run_root}/history?limit=10"),
    ] {
        assert_eq!(
            app.call(get_as(path, RESTRICTED_WORKFLOW_TOKEN))
                .await?
                .status(),
            200
        );
    }
    assert_eq!(
        app.call(get_as(
            format!("{granted_run_root}/output"),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?
        .status(),
        409
    );
    assert_eq!(
        app.call(get_as(
            format!("{granted_run_root}/variables"),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?
        .status(),
        409
    );

    let denied_definition_root = format!(
        "/api/v1/organizations/{organization}/workflow-definitions/{}",
        denied.definition_id
    );
    let denied_goal_root = format!(
        "/api/v1/organizations/{organization}/workflow-goals/{}",
        denied.goal_id
    );
    let denied_run_root = format!(
        "/api/v1/organizations/{organization}/workflow-runs/{}",
        denied.run_id
    );
    let missing_definition_root = format!(
        "/api/v1/organizations/{organization}/workflow-definitions/{}",
        Uuid::now_v7()
    );
    let missing_goal_root = format!(
        "/api/v1/organizations/{organization}/workflow-goals/{}",
        Uuid::now_v7()
    );
    let missing_run_root = format!(
        "/api/v1/organizations/{organization}/workflow-runs/{}",
        Uuid::now_v7()
    );
    for (denied_path, missing_path) in [
        (
            denied_definition_root.clone(),
            missing_definition_root.clone(),
        ),
        (
            format!("{denied_definition_root}/revisions"),
            format!("{missing_definition_root}/revisions"),
        ),
        (
            format!("{denied_definition_root}/revisions/{}", denied.revision_id),
            format!("{missing_definition_root}/revisions/{}", Uuid::now_v7()),
        ),
        (denied_goal_root.clone(), missing_goal_root.clone()),
        (
            format!(
                "{denied_goal_root}/plan-revisions/{}",
                denied.plan_revision_id
            ),
            format!("{missing_goal_root}/plan-revisions/{}", Uuid::now_v7()),
        ),
        (denied_run_root.clone(), missing_run_root.clone()),
        (
            format!("{denied_run_root}/wait?timeoutSeconds=0"),
            format!("{missing_run_root}/wait?timeoutSeconds=0"),
        ),
        (
            format!("{denied_run_root}/output"),
            format!("{missing_run_root}/output"),
        ),
        (
            format!("{denied_run_root}/diagnostics"),
            format!("{missing_run_root}/diagnostics"),
        ),
        (
            format!("{denied_run_root}/history?limit=10"),
            format!("{missing_run_root}/history?limit=10"),
        ),
        (
            format!("{denied_run_root}/variables"),
            format!("{missing_run_root}/variables"),
        ),
    ] {
        assert_resource_not_found_equivalent(
            &app,
            get_as(denied_path, RESTRICTED_WORKFLOW_TOKEN),
            get_as(missing_path, RESTRICTED_WORKFLOW_TOKEN),
        )
        .await?;
    }

    for (id, name, arguments, expected_code) in [
        (
            20,
            "a3s_cloud_workflow_definitions_get",
            json!({"workflowDefinitionId": granted.definition_id}),
            200,
        ),
        (
            21,
            "a3s_cloud_workflow_revisions_list",
            json!({"workflowDefinitionId": granted.definition_id}),
            200,
        ),
        (
            22,
            "a3s_cloud_workflow_revisions_get",
            json!({
                "workflowDefinitionId": granted.definition_id,
                "workflowRevisionId": granted.revision_id
            }),
            200,
        ),
        (
            23,
            "a3s_cloud_workflow_goals_get",
            json!({"workflowGoalId": granted.goal_id}),
            200,
        ),
        (
            24,
            "a3s_cloud_workflow_plan_revisions_get",
            json!({
                "workflowGoalId": granted.goal_id,
                "planRevisionId": granted.plan_revision_id
            }),
            200,
        ),
        (
            25,
            "a3s_cloud_workflow_runs_get",
            json!({"workflowRunId": granted.run_id}),
            200,
        ),
        (
            26,
            "a3s_cloud_workflow_runs_wait",
            json!({"workflowRunId": granted.run_id, "timeoutSeconds": 0}),
            200,
        ),
        (
            27,
            "a3s_cloud_workflow_run_history_get",
            json!({"workflowRunId": granted.run_id, "limit": 10}),
            200,
        ),
        (
            28,
            "a3s_cloud_workflow_run_output_get",
            json!({"workflowRunId": granted.run_id}),
            409,
        ),
        (
            30,
            "a3s_cloud_workflow_run_diagnostics_get",
            json!({"workflowRunId": granted.run_id}),
            200,
        ),
        (
            29,
            "a3s_cloud_workflow_run_variables_get",
            json!({"workflowRunId": granted.run_id}),
            409,
        ),
    ] {
        let response = app
            .call(mcp_tool_call_as(
                id,
                name,
                arguments,
                RESTRICTED_WORKFLOW_TOKEN,
            ))
            .await?;
        let response = response_json(&response)?;
        assert_eq!(
            response["result"]["structuredContent"]["code"], expected_code,
            "{name}"
        );
        assert_eq!(response["result"]["isError"], expected_code != 200);
    }
    for (id, name, denied_arguments, missing_arguments) in [
        (
            40,
            "a3s_cloud_workflow_definitions_get",
            json!({"workflowDefinitionId": denied.definition_id}),
            json!({"workflowDefinitionId": Uuid::now_v7()}),
        ),
        (
            42,
            "a3s_cloud_workflow_revisions_list",
            json!({"workflowDefinitionId": denied.definition_id}),
            json!({"workflowDefinitionId": Uuid::now_v7()}),
        ),
        (
            44,
            "a3s_cloud_workflow_revisions_get",
            json!({
                "workflowDefinitionId": denied.definition_id,
                "workflowRevisionId": denied.revision_id
            }),
            json!({
                "workflowDefinitionId": Uuid::now_v7(),
                "workflowRevisionId": Uuid::now_v7()
            }),
        ),
        (
            46,
            "a3s_cloud_workflow_goals_get",
            json!({"workflowGoalId": denied.goal_id}),
            json!({"workflowGoalId": Uuid::now_v7()}),
        ),
        (
            48,
            "a3s_cloud_workflow_plan_revisions_get",
            json!({
                "workflowGoalId": denied.goal_id,
                "planRevisionId": denied.plan_revision_id
            }),
            json!({
                "workflowGoalId": Uuid::now_v7(),
                "planRevisionId": Uuid::now_v7()
            }),
        ),
        (
            50,
            "a3s_cloud_workflow_runs_get",
            json!({"workflowRunId": denied.run_id}),
            json!({"workflowRunId": Uuid::now_v7()}),
        ),
        (
            52,
            "a3s_cloud_workflow_runs_wait",
            json!({"workflowRunId": denied.run_id, "timeoutSeconds": 0}),
            json!({"workflowRunId": Uuid::now_v7(), "timeoutSeconds": 0}),
        ),
        (
            54,
            "a3s_cloud_workflow_run_output_get",
            json!({"workflowRunId": denied.run_id}),
            json!({"workflowRunId": Uuid::now_v7()}),
        ),
        (
            60,
            "a3s_cloud_workflow_run_diagnostics_get",
            json!({"workflowRunId": denied.run_id}),
            json!({"workflowRunId": Uuid::now_v7()}),
        ),
        (
            56,
            "a3s_cloud_workflow_run_history_get",
            json!({"workflowRunId": denied.run_id, "limit": 10}),
            json!({"workflowRunId": Uuid::now_v7(), "limit": 10}),
        ),
        (
            58,
            "a3s_cloud_workflow_run_variables_get",
            json!({"workflowRunId": denied.run_id}),
            json!({"workflowRunId": Uuid::now_v7()}),
        ),
    ] {
        let denied_response = app
            .call(mcp_tool_call_as(
                id,
                name,
                denied_arguments,
                RESTRICTED_WORKFLOW_TOKEN,
            ))
            .await?;
        let missing_response = app
            .call(mcp_tool_call_as(
                id + 1,
                name,
                missing_arguments,
                RESTRICTED_WORKFLOW_TOKEN,
            ))
            .await?;
        assert_mcp_not_found_equivalent(&denied_response, &missing_response)?;
    }

    let denied_revision = workflow_fixture("Denied Workflow revision")
        .map_err(BootError::Internal)?
        .transport;
    let denied_revision_mcp = app
        .call(mcp_tool_call_as(
            60,
            "a3s_cloud_workflow_definitions_revise",
            {
                let mut arguments = denied_revision.clone();
                let object = arguments.as_object_mut().ok_or_else(|| {
                    BootError::Internal("Workflow revision fixture is not an object".into())
                })?;
                object.insert("workflowDefinitionId".into(), json!(denied.definition_id));
                object.insert("expectedVersion".into(), json!(1));
                object.insert("idempotencyKey".into(), json!("workflow-mcp-revise-denied"));
                arguments
            },
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    let missing_revision_mcp = app
        .call(mcp_tool_call_as(
            61,
            "a3s_cloud_workflow_definitions_revise",
            {
                let mut arguments = denied_revision.clone();
                let object = arguments.as_object_mut().ok_or_else(|| {
                    BootError::Internal("Workflow revision fixture is not an object".into())
                })?;
                object.insert("workflowDefinitionId".into(), json!(Uuid::now_v7()));
                object.insert("expectedVersion".into(), json!(1));
                object.insert(
                    "idempotencyKey".into(),
                    json!("workflow-mcp-revise-missing"),
                );
                arguments
            },
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    assert_mcp_not_found_equivalent(&denied_revision_mcp, &missing_revision_mcp)?;
    let denied_cancel_mcp = app
        .call(mcp_tool_call_as(
            62,
            "a3s_cloud_workflow_runs_cancel",
            json!({
                "workflowRunId": denied.run_id,
                "reason": "Must not cancel",
                "idempotencyKey": "workflow-mcp-cancel-denied"
            }),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    let missing_cancel_mcp = app
        .call(mcp_tool_call_as(
            63,
            "a3s_cloud_workflow_runs_cancel",
            json!({
                "workflowRunId": Uuid::now_v7(),
                "reason": "Must not cancel",
                "idempotencyKey": "workflow-mcp-cancel-missing"
            }),
            RESTRICTED_WORKFLOW_TOKEN,
        ))
        .await?;
    assert_mcp_not_found_equivalent(&denied_cancel_mcp, &missing_cancel_mcp)?;
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("{denied_definition_root}/revisions"),
            "workflow-revise-denied",
            denied_revision.clone(),
            RESTRICTED_WORKFLOW_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1"),
        post_json_as(
            format!("{missing_definition_root}/revisions"),
            "workflow-revise-missing",
            denied_revision,
            RESTRICTED_WORKFLOW_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1"),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("{denied_run_root}/cancel"),
            "workflow-cancel-denied",
            json!({"reason": "Must not cancel"}),
            RESTRICTED_WORKFLOW_TOKEN,
        ),
        post_json_as(
            format!("{missing_run_root}/cancel"),
            "workflow-cancel-missing",
            json!({"reason": "Must not cancel"}),
            RESTRICTED_WORKFLOW_TOKEN,
        ),
    )
    .await?;

    let granted_revision = workflow_fixture("Granted Workflow revision")
        .map_err(BootError::Internal)?
        .transport;
    let revise_granted = || {
        post_json_as(
            format!("{granted_definition_root}/revisions"),
            "workflow-revise-granted",
            granted_revision.clone(),
            RESTRICTED_WORKFLOW_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1")
    };
    assert_eq!(app.call(revise_granted()).await?.status(), 201);
    let revise_replay = app.call(revise_granted()).await?;
    assert_eq!(revise_replay.status(), 200);
    assert_eq!(response_json(&revise_replay)?["data"]["replayed"], true);

    let cancel_granted = || {
        post_json_as(
            format!("{granted_run_root}/cancel"),
            "workflow-cancel-granted",
            json!({"reason": "Authorized cancellation"}),
            RESTRICTED_WORKFLOW_TOKEN,
        )
    };
    assert_eq!(app.call(cancel_granted()).await?.status(), 202);
    let cancel_replay = app.call(cancel_granted()).await?;
    assert_eq!(cancel_replay.status(), 200);
    assert_eq!(response_json(&cancel_replay)?["data"]["replayed"], true);

    let revoked = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{project_grant_id}/revocation"
            ),
            "workflow-grants-revoke",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_resource_not_found_equivalent(
        &app,
        revise_granted(),
        post_json_as(
            format!("{missing_definition_root}/revisions"),
            "workflow-revise-missing-after-revoke",
            granted_revision,
            RESTRICTED_WORKFLOW_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1"),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        cancel_granted(),
        post_json_as(
            format!("{missing_run_root}/cancel"),
            "workflow-cancel-missing-after-revoke",
            json!({"reason": "Authorized cancellation"}),
            RESTRICTED_WORKFLOW_TOKEN,
        ),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        get_as(&granted_definition_root, RESTRICTED_WORKFLOW_TOKEN),
        get_as(&missing_definition_root, RESTRICTED_WORKFLOW_TOKEN),
    )
    .await?;
    Ok(())
}

struct WorkflowAccessFixture {
    definition_id: String,
    revision_id: String,
    goal_id: String,
    plan_revision_id: String,
    run_id: String,
}

async fn create_workflow_access_fixture(
    app: &BootApplication,
    organization: &str,
    project: &str,
    key: &str,
) -> Result<WorkflowAccessFixture> {
    let ontology = app
        .call(post_acl(
            format!("/api/v1/organizations/{organization}/projects/{project}/ontologies"),
            &format!("{key}-ontology"),
            ONTOLOGY_ACL.as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(ontology.status(), 201);
    let ontology = response_json(&ontology)?;

    let fixture = workflow_fixture(key).map_err(BootError::Internal)?;
    let definition = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/workflow-definitions"),
            &format!("{key}-definition"),
            fixture.transport,
        ))
        .await?;
    assert_eq!(definition.status(), 201);
    let definition = response_json(&definition)?;
    let definition_id = required_string(
        &definition["data"]["workflowDefinition"]["id"],
        "WorkflowDefinition ID",
    )?;
    let revision_id = required_string(
        &definition["data"]["revision"]["id"],
        "Workflow revision ID",
    )?;
    let workflow_digest = required_string(
        &definition["data"]["revision"]["contentDigest"],
        "Workflow revision digest",
    )?;

    let goal_contract = WorkflowGoalContract::from_spec(WorkflowGoalSpec {
        name: format!("{key} goal"),
        workflow_definition_id: WorkflowDefinitionId::from_uuid(
            Uuid::parse_str(&definition_id)
                .map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        workflow_revision_id: WorkflowRevisionId::from_uuid(
            Uuid::parse_str(&revision_id)
                .map_err(|error| BootError::Internal(error.to_string()))?,
        ),
        workflow_digest: Sha256Digest::parse(workflow_digest).map_err(BootError::Internal)?,
        ontology_id: OntologyId::from_uuid(required_uuid(
            &ontology["data"]["ontology"]["id"],
            "Ontology ID",
        )?),
        ontology_revision_id: OntologyRevisionId::from_uuid(required_uuid(
            &ontology["data"]["revision"]["id"],
            "Ontology revision ID",
        )?),
        ontology_digest: Sha256Digest::parse(required_string(
            &ontology["data"]["revision"]["contentDigest"],
            "Ontology digest",
        )?)
        .map_err(BootError::Internal)?,
        environment_id: None,
        input: json!({"key": key}),
    })
    .map_err(BootError::Internal)?;
    let goal = app
        .call(post_acl(
            format!("/api/v1/organizations/{organization}/projects/{project}/workflow-goals"),
            &format!("{key}-goal"),
            goal_contract.canonical_acl().as_bytes().to_vec(),
        ))
        .await?;
    assert_eq!(goal.status(), 201);
    let goal = response_json(&goal)?;
    let goal_id = required_string(&goal["data"]["goal"]["id"], "WorkflowGoal ID")?;
    let plan_revision_id =
        required_string(&goal["data"]["planRevision"]["id"], "Plan revision ID")?;

    let run = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/workflow-runs"),
            &format!("{key}-run"),
            json!({
                "workflowGoalId": goal_id,
                "planRevisionId": plan_revision_id,
                "timeoutSeconds": 60
            }),
        ))
        .await?;
    assert_eq!(run.status(), 202);
    let run = response_json(&run)?;
    let run_id = required_string(&run["data"]["workflowRun"]["id"], "WorkflowRun ID")?;

    Ok(WorkflowAccessFixture {
        definition_id,
        revision_id,
        goal_id,
        plan_revision_id,
        run_id,
    })
}

pub(super) struct WorkflowFixture {
    pub(super) transport: Value,
    pub(super) payload_set_digest: String,
    pub(super) semantic_contract_set_digest: Option<String>,
    pub(super) schema_digest: String,
}

pub(super) fn workflow_fixture(description: &str) -> std::result::Result<WorkflowFixture, String> {
    workflow_fixture_with_semantics(description, false)
}

pub(super) fn semantic_workflow_fixture(
    description: &str,
) -> std::result::Result<WorkflowFixture, String> {
    workflow_fixture_with_semantics(description, true)
}

fn legacy_provider_workflow_transport(
    kind: WorkflowStepKind,
    capability_type: CapabilityType,
) -> std::result::Result<Value, String> {
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
        ))?;
    let provider_configuration = WorkflowPayload::from_content(
        WorkflowPayloadContent::Configuration(WorkflowStepConfiguration::empty(kind)),
    )?;
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))?;
    let payloads = vec![
        input_configuration,
        provider_configuration,
        output_configuration,
        data_schema,
    ];
    let schema_digest = payloads[3].digest().clone();
    let mut provider = workflow_step("provider", kind, &payloads[1], &schema_digest);
    provider.capability = Some(CapabilityReference {
        owner: capability_type.owner(),
        capability_type,
        resource_id: Uuid::now_v7(),
        revision: "release-1".into(),
        digest: Sha256Digest::parse(format!("sha256:{}", "e".repeat(64)))?,
        capability: format!("{}.invoke", kind.as_str()),
    });
    let contract = WorkflowContract::from_spec(WorkflowSpec {
        name: "Legacy provider".into(),
        description: "Structurally valid legacy provider graph".into(),
        steps: vec![
            workflow_step(
                "input",
                WorkflowStepKind::Input,
                &payloads[0],
                &schema_digest,
            ),
            provider,
            workflow_step(
                "output",
                WorkflowStepKind::Output,
                &payloads[2],
                &schema_digest,
            ),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-provider".into(),
                source: "input".into(),
                target: "provider".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "provider-output".into(),
                source: "provider".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    })?;
    crate::modules::workflow::WorkflowRevision::initial(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowDefinitionId::new(),
        WorkflowRevisionId::new(),
        contract.clone(),
        payloads.clone(),
        PrincipalId::new(),
        Utc::now(),
    )?;
    Ok(json!({
        "definitionAcl": contract.canonical_acl(),
        "payloads": payloads.iter().map(|payload| json!({
            "kind": payload.kind().as_str(),
            "acl": payload.canonical_acl(),
        })).collect::<Vec<_>>(),
    }))
}

fn workflow_fixture_with_semantics(
    description: &str,
    include_semantics: bool,
) -> std::result::Result<WorkflowFixture, String> {
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
        ))?;
    let mut transform = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    transform.template = Some("{{ input }}".into());
    let transform_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(transform))?;
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))?;
    let payloads = vec![
        input_configuration,
        transform_configuration,
        output_configuration,
        data_schema,
    ];
    let schema_digest = payloads[3].digest().clone();
    let contract = WorkflowContract::from_spec(WorkflowSpec {
        name: "Support triage".into(),
        description: description.into(),
        steps: vec![
            workflow_step(
                "input",
                WorkflowStepKind::Input,
                &payloads[0],
                &schema_digest,
            ),
            workflow_step(
                "triage",
                WorkflowStepKind::Transform,
                &payloads[1],
                &schema_digest,
            ),
            workflow_step(
                "output",
                WorkflowStepKind::Output,
                &payloads[2],
                &schema_digest,
            ),
        ],
        edges: vec![
            WorkflowEdgeSpec {
                id: "input-triage".into(),
                source: "input".into(),
                target: "triage".into(),
                source_handle: None,
            },
            WorkflowEdgeSpec {
                id: "triage-output".into(),
                source: "triage".into(),
                target: "output".into(),
                source_handle: None,
            },
        ],
    })?;
    let revision = crate::modules::workflow::WorkflowRevision::initial(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowDefinitionId::new(),
        WorkflowRevisionId::new(),
        contract.clone(),
        payloads.clone(),
        crate::modules::shared_kernel::domain::PrincipalId::new(),
        Utc::now(),
    )?;
    let mut transport = json!({
        "definitionAcl": contract.canonical_acl(),
        "payloads": payloads.iter().map(|payload| json!({
            "kind": payload.kind().as_str(),
            "acl": payload.canonical_acl(),
        })).collect::<Vec<_>>(),
    });
    let semantic_contract_set_digest = if include_semantics {
        let semantic_contracts = semantic_contracts(contract.spec(), &schema_digest)?;
        let variable_defaults = semantic_contracts
            .variable_defaults()
            .ok_or_else(|| "semantic fixture has no variable defaults".to_owned())?;
        transport
            .as_object_mut()
            .ok_or_else(|| "Workflow transport is not an object".to_owned())?
            .insert(
                "semanticContracts".into(),
                json!({
                    "descriptorBindingsAcl": semantic_contracts
                        .descriptor_bindings()
                        .canonical_acl(),
                    "descriptorRegistryAcl": semantic_contracts
                        .descriptor_registry()
                        .canonical_acl(),
                    "variableContractAcl": semantic_contracts.variable_contract().canonical_acl(),
                    "variableDefaultsAcl": variable_defaults.canonical_acl(),
                }),
            );
        Some(semantic_contracts.digest().to_string())
    } else {
        None
    };
    Ok(WorkflowFixture {
        transport,
        payload_set_digest: revision.payload_set_digest.to_string(),
        semantic_contract_set_digest,
        schema_digest: schema_digest.to_string(),
    })
}

fn semantic_contracts(
    workflow: &WorkflowSpec,
    schema_digest: &Sha256Digest,
) -> std::result::Result<WorkflowRevisionSemanticContracts, String> {
    let descriptor_specs = workflow
        .steps
        .iter()
        .map(|step| workflow_local_descriptor(step.kind, &step.configuration_digest))
        .collect::<Vec<_>>();
    let registry = WorkflowStepDescriptorRegistry::from_spec(WorkflowStepDescriptorRegistrySpec {
        id: "support.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        descriptors: descriptor_specs,
    })?;
    let bindings = WorkflowStepDescriptorBindings::from_spec(WorkflowStepDescriptorBindingsSpec {
        id: "support.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        bindings: workflow
            .steps
            .iter()
            .map(|step| {
                let descriptor_id = format!("workflow.{}", step.kind.as_str());
                let descriptor = registry
                    .resolve(&descriptor_id, "1.0.0")
                    .ok_or_else(|| format!("missing test descriptor {descriptor_id:?}"))?;
                Ok(WorkflowStepDescriptorBinding {
                    step_id: step.id.clone(),
                    descriptor_id,
                    descriptor_revision: "1.0.0".into(),
                    semantic_digest: descriptor.semantic_digest().clone(),
                })
            })
            .collect::<std::result::Result<Vec<_>, String>>()?,
    })?;
    let default = WorkflowVariableDefault::new("fallback", json!("normal"))?;
    let variables = WorkflowVariableContract::from_spec(WorkflowVariableContractSpec {
        id: "support.workflow".into(),
        revision: "1.0.0".into(),
        compiler_schema_version: 2,
        declarations: vec![
            WorkflowVariableDeclaration {
                name: "request".into(),
                scope: WorkflowVariableScope::InvocationInput,
                value_type: WorkflowDataType::Any,
                value_schema_digest: schema_digest.clone(),
                source_schema_digest: Some(schema_digest.clone()),
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Immutable,
                required: true,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: None,
            },
            WorkflowVariableDeclaration {
                name: "fallback".into(),
                scope: WorkflowVariableScope::Run,
                value_type: WorkflowDataType::String,
                value_schema_digest: schema_digest.clone(),
                source_schema_digest: None,
                storage_class: WorkflowVariableStorageClass::Inline,
                mutation_mode: WorkflowVariableMutationMode::Deterministic,
                required: false,
                source_step_id: None,
                source_path: Vec::new(),
                region_id: None,
                default_value_digest: Some(default.digest.clone()),
            },
        ],
        reads: vec![WorkflowVariableRead {
            id: "output-request".into(),
            variable: "request".into(),
            consumer_step_id: "output".into(),
            consumer_region_id: None,
            target_port: "result".into(),
            path: Vec::new(),
            expected_type: WorkflowDataType::Any,
            expected_schema_digest: schema_digest.clone(),
            required: true,
            mode: WorkflowVariableReadMode::DirectValue,
        }],
        assignments: Vec::new(),
        exports: Vec::new(),
    })?;
    let defaults = WorkflowVariableDefaults::from_spec(WorkflowVariableDefaultsSpec {
        id: variables.id().into(),
        revision: variables.revision().into(),
        values: vec![default],
    })?;
    WorkflowRevisionSemanticContracts::create_with_defaults(
        workflow,
        bindings,
        registry,
        variables,
        Some(defaults),
    )
}

fn workflow_local_descriptor(
    kind: WorkflowStepKind,
    configuration_schema_digest: &Sha256Digest,
) -> WorkflowStepDescriptorSpec {
    let (input, output) = match kind {
        WorkflowStepKind::Input => ("invocation", "value"),
        WorkflowStepKind::Output => ("result", "value"),
        _ => ("input", "value"),
    };
    let id = format!("workflow.{}", kind.as_str());
    WorkflowStepDescriptorSpec {
        id: id.clone(),
        revision: "1.0.0".into(),
        owner: WorkflowStepOwner::Workflow,
        kind: Some(kind),
        semantic_profile: id.clone(),
        execution_class: WorkflowStepExecutionClass::WorkflowLocal,
        input_ports: vec![workflow_port(input)],
        output_ports: vec![workflow_port(output)],
        configuration_schema_digest: configuration_schema_digest.clone(),
        default_policy_digest: None,
        required_bindings: Vec::new(),
        allowed_capability_types: Vec::new(),
        failure: WorkflowStepFailureContract {
            error_output: None,
            retry_classification: WorkflowStepRetryClassification::NotRetryable,
            fallback: WorkflowStepFallbackMode::Unsupported,
            failure_branch: false,
        },
        minimum_compiler_schema_version: 2,
        maximum_compiler_schema_version: 2,
        admission: WorkflowStepDescriptorAdmission::Admitted,
        unavailable_reason: None,
        presentation: WorkflowStepPresentationSpec {
            label: kind.as_str().into(),
            summary: format!("{} test descriptor", kind.as_str()),
            icon_key: id,
        },
    }
}

fn workflow_port(name: &str) -> WorkflowStepPort {
    WorkflowStepPort {
        name: name.into(),
        value_type: WorkflowDataType::Any,
        cardinality: WorkflowStepPortCardinality::Single,
        required: true,
        dynamic: false,
    }
}

fn workflow_step(
    id: &str,
    kind: WorkflowStepKind,
    configuration: &WorkflowPayload,
    schema_digest: &Sha256Digest,
) -> WorkflowStepSpec {
    WorkflowStepSpec {
        id: id.into(),
        label: id.into(),
        kind,
        configuration_digest: configuration.digest().clone(),
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest.clone(),
        policy_digest: None,
        capability: None,
    }
}

fn required_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("{label} is missing")))
}

fn required_uuid(value: &Value, label: &str) -> Result<Uuid> {
    Uuid::parse_str(&required_string(value, label)?)
        .map_err(|error| BootError::Internal(format!("{label} is invalid: {error}")))
}
