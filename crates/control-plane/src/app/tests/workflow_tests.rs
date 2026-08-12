use super::*;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::{
    AssignmentPolicyRef, HumanTaskInteractionSpec, WorkflowContract, WorkflowDataSchema,
    WorkflowDataType, WorkflowEdgeSpec, WorkflowGoalContract, WorkflowGoalSpec, WorkflowPayload,
    WorkflowPayloadContent, WorkflowSpec, WorkflowStepConfiguration, WorkflowStepKind,
    WorkflowStepSpec,
};

const ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));
const RESTRICTED_WORKFLOW_TOKEN: &str =
    "a3s_8888888888888888888888888888888888888888888888888888888888888888";

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
                "scopes": [ApiTokenScope::CLOUD_READ],
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
    task.assignment_policy = AssignmentPolicyRef::workflow_organization_member_exclusive()
        .map_err(BootError::Internal)?;
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
            "/api/v1/organizations/{organization}/projects/{granted_project}/workflow-definitions"
        ),
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/workflow-goals"),
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/workflow-runs"),
    ];
    let denied_collections = [
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
            format!("{denied_run_root}/history?limit=10"),
            format!("{missing_run_root}/history?limit=10"),
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
            56,
            "a3s_cloud_workflow_run_history_get",
            json!({"workflowRunId": denied.run_id, "limit": 10}),
            json!({"workflowRunId": Uuid::now_v7(), "limit": 10}),
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
}

pub(super) fn workflow_fixture(description: &str) -> std::result::Result<WorkflowFixture, String> {
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
    Ok(WorkflowFixture {
        transport: json!({
            "definitionAcl": contract.canonical_acl(),
            "payloads": payloads.into_iter().map(|payload| json!({
                "kind": payload.kind().as_str(),
                "acl": payload.canonical_acl(),
            })).collect::<Vec<_>>(),
        }),
        payload_set_digest: revision.payload_set_digest.to_string(),
    })
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
