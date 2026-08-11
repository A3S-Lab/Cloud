use super::*;
use crate::modules::shared_kernel::domain::{
    OntologyId, OntologyRevisionId, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::{
    WorkflowContract, WorkflowDataSchema, WorkflowDataType, WorkflowEdgeSpec, WorkflowGoalContract,
    WorkflowGoalSpec, WorkflowPayload, WorkflowPayloadContent, WorkflowSpec,
    WorkflowStepConfiguration, WorkflowStepKind, WorkflowStepSpec,
};

const ONTOLOGY_ACL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/w0.1/ontology.acl"
));

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
