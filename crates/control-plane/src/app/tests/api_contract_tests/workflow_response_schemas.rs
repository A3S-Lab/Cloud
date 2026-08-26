use super::*;

#[test]
fn workflow_definition_and_revision_responses_are_closed_and_typed() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let schemas = &document["components"]["schemas"];

    assert_eq!(
        schemas["WorkflowPayloadSchema"]["enum"],
        json!([
            "cloud.workflow.configuration.v1",
            "cloud.workflow.configuration.list-operator.v1",
            "cloud.workflow.configuration.variable-aggregate.v1",
            "cloud.workflow.data-schema.v1",
            "cloud.workflow.policy.v1",
            "cloud.workflow.policy.v2",
            "cloud.workflow.policy.v3",
            "cloud.workflow.policy.v4"
        ])
    );
    assert_eq!(schemas["WorkflowRevision"]["additionalProperties"], false);
    assert_eq!(
        schemas["WorkflowRevision"]["properties"]["payloads"]["items"]["$ref"],
        "#/components/schemas/WorkflowPayload"
    );
    assert_eq!(
        schemas["WorkflowRevision"]["properties"]["semanticContracts"]["items"]["$ref"],
        "#/components/schemas/WorkflowSemanticContract"
    );
    assert_eq!(
        schemas["WorkflowSemanticContractSchema"]["enum"],
        json!([
            "cloud.workflow.composite-regions.v1",
            "cloud.workflow.step-descriptor-bindings.v1",
            "cloud.workflow.step-descriptor-registry.v1",
            "cloud.workflow.variable-contract.v1",
            "cloud.workflow.variable-defaults.v1"
        ])
    );

    let definition_collection = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/workflow-definitions"];
    assert_eq!(
        definition_collection["get"]["responses"]["200"]["$ref"],
        "#/components/responses/WorkflowDefinitionListSuccess200"
    );
    for status in ["200", "201"] {
        assert_eq!(
            definition_collection["post"]["responses"][status]["$ref"],
            format!("#/components/responses/WorkflowDefinitionMutationSuccess{status}")
        );
    }

    let definition = &document["paths"]
        ["/organizations/{organization_id}/workflow-definitions/{workflow_definition_id}"]["get"];
    assert_eq!(
        definition["responses"]["200"]["$ref"],
        "#/components/responses/WorkflowDefinitionSuccess200"
    );
    let revision_collection = &document["paths"]
        ["/organizations/{organization_id}/workflow-definitions/{workflow_definition_id}/revisions"];
    assert_eq!(
        revision_collection["get"]["responses"]["200"]["$ref"],
        "#/components/responses/WorkflowRevisionSummaryListSuccess200"
    );
    for status in ["200", "201"] {
        assert_eq!(
            revision_collection["post"]["responses"][status]["$ref"],
            format!("#/components/responses/WorkflowDefinitionMutationSuccess{status}")
        );
    }
    let revision = &document["paths"]
        ["/organizations/{organization_id}/workflow-definitions/{workflow_definition_id}/revisions/{workflow_revision_id}"]["get"];
    assert_eq!(
        revision["responses"]["200"]["$ref"],
        "#/components/responses/WorkflowRevisionSuccess200"
    );
    Ok(())
}

#[test]
fn workflow_goal_catalog_run_and_observation_responses_are_closed_and_typed() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let schemas = &document["components"]["schemas"];

    for name in [
        "WorkflowNodeCatalog",
        "WorkflowNodeCatalogEntry",
        "WorkflowGoal",
        "WorkflowGoalMutation",
        "WorkflowPlanRevision",
        "WorkflowPlan",
        "WorkflowPlanStep",
        "WorkflowRun",
        "WorkflowRunMutation",
        "WorkflowStepProjection",
        "WorkflowRunOutput",
        "WorkflowRunVariableInspection",
        "WorkflowRunVariable",
        "WorkflowRunDiagnostics",
        "WorkflowRunHistoryPage",
        "WorkflowRunHistoryEvent",
    ] {
        assert_eq!(
            schemas[name]["additionalProperties"], false,
            "{name} must reject undocumented fields"
        );
    }
    assert_eq!(
        schemas["WorkflowNodeCatalog"]["properties"]["nodes"]["items"]["$ref"],
        "#/components/schemas/WorkflowNodeCatalogEntry"
    );
    assert_eq!(
        schemas["WorkflowPlan"]["properties"]["steps"]["items"]["$ref"],
        "#/components/schemas/WorkflowPlanStep"
    );
    assert!(schemas["WorkflowPlan"]["properties"]["schema"]["enum"]
        .as_array()
        .is_some_and(|values| values.contains(&json!("cloud.workflow.plan.v12"))));
    assert!(
        schemas["WorkflowPlan"]["properties"]["compilerRevision"]["enum"]
            .as_array()
            .is_some_and(|values| values.contains(&json!("cloud.workflow.plan-compiler.v12")))
    );
    assert!(
        schemas["WorkflowStepFailureOutput"]["properties"]["schema"]["enum"]
            .as_array()
            .is_some_and(|values| values.contains(&json!("cloud.workflow.step-failure.v9")))
    );
    for classification in [
        "agent_dispatch_rejected",
        "agent_execution_failed",
        "agent_execution_cancelled",
    ] {
        assert!(
            schemas["WorkflowStepFailureOutput"]["properties"]["classification"]["enum"]
                .as_array()
                .is_some_and(|values| values.contains(&json!(classification)))
        );
    }
    assert_eq!(
        schemas["WorkflowRun"]["properties"]["steps"]["items"]["$ref"],
        "#/components/schemas/WorkflowStepProjection"
    );
    assert_eq!(
        schemas["WorkflowRunDiagnostics"]["properties"]["diagnostics"]["items"]["$ref"],
        "#/components/schemas/WorkflowRunDiagnostic"
    );
    assert_eq!(
        schemas["WorkflowRunVariableInspection"]["properties"]["variables"]["items"]["$ref"],
        "#/components/schemas/WorkflowRunVariable"
    );
    assert_eq!(
        schemas["WorkflowRunHistoryPage"]["properties"]["events"]["items"]["$ref"],
        "#/components/schemas/WorkflowRunHistoryEvent"
    );

    for (success_schema, data_schema) in [
        ("WorkflowNodeCatalogSuccessResponse", "WorkflowNodeCatalog"),
        ("WorkflowGoalSuccessResponse", "WorkflowGoal"),
        ("WorkflowGoalListSuccessResponse", "WorkflowGoalList"),
        (
            "WorkflowGoalMutationSuccessResponse",
            "WorkflowGoalMutation",
        ),
        (
            "WorkflowPlanRevisionSuccessResponse",
            "WorkflowPlanRevision",
        ),
        ("WorkflowRunSuccessResponse", "WorkflowRun"),
        ("WorkflowRunListSuccessResponse", "WorkflowRunList"),
        ("WorkflowRunMutationSuccessResponse", "WorkflowRunMutation"),
        ("WorkflowRunOutputSuccessResponse", "WorkflowRunOutput"),
        (
            "WorkflowRunVariableInspectionSuccessResponse",
            "WorkflowRunVariableInspection",
        ),
        (
            "WorkflowRunDiagnosticsSuccessResponse",
            "WorkflowRunDiagnostics",
        ),
        (
            "WorkflowRunHistoryPageSuccessResponse",
            "WorkflowRunHistoryPage",
        ),
    ] {
        assert_eq!(
            schemas[success_schema]["allOf"][0]["properties"]["data"]["$ref"],
            format!("#/components/schemas/{data_schema}")
        );
    }

    for (path, response) in [
        (
            "/organizations/{organization_id}/projects/{project_id}/workflow-node-catalog",
            "WorkflowNodeCatalogSuccess200",
        ),
        (
            "/organizations/{organization_id}/projects/{project_id}/workflow-goals",
            "WorkflowGoalListSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-goals/{workflow_goal_id}",
            "WorkflowGoalSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-goals/{workflow_goal_id}/plan-revisions/{plan_revision_id}",
            "WorkflowPlanRevisionSuccess200",
        ),
        (
            "/organizations/{organization_id}/projects/{project_id}/workflow-runs",
            "WorkflowRunListSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-runs/{workflow_run_id}",
            "WorkflowRunSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-runs/{workflow_run_id}/wait",
            "WorkflowRunSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-runs/{workflow_run_id}/output",
            "WorkflowRunOutputSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-runs/{workflow_run_id}/variables",
            "WorkflowRunVariableInspectionSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-runs/{workflow_run_id}/diagnostics",
            "WorkflowRunDiagnosticsSuccess200",
        ),
        (
            "/organizations/{organization_id}/workflow-runs/{workflow_run_id}/history",
            "WorkflowRunHistoryPageSuccess200",
        ),
    ] {
        assert_eq!(
            document["paths"][path]["get"]["responses"]["200"]["$ref"],
            format!("#/components/responses/{response}"),
            "GET {path} must use its exact success response"
        );
    }

    let goal_collection =
        &document["paths"]["/organizations/{organization_id}/projects/{project_id}/workflow-goals"];
    for status in ["200", "201"] {
        assert_eq!(
            goal_collection["post"]["responses"][status]["$ref"],
            format!("#/components/responses/WorkflowGoalMutationSuccess{status}")
        );
    }
    let run_collection =
        &document["paths"]["/organizations/{organization_id}/projects/{project_id}/workflow-runs"];
    let run_cancellation = &document["paths"]
        ["/organizations/{organization_id}/workflow-runs/{workflow_run_id}/cancel"];
    for status in ["200", "202"] {
        let expected = format!("#/components/responses/WorkflowRunMutationSuccess{status}");
        assert_eq!(
            run_collection["post"]["responses"][status]["$ref"],
            expected
        );
        assert_eq!(
            run_cancellation["post"]["responses"][status]["$ref"],
            expected
        );
    }
    Ok(())
}

#[test]
fn workflow_ontology_and_human_task_responses_are_closed_and_typed() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let schemas = &document["components"]["schemas"];

    for name in [
        "Ontology",
        "OntologyMigrationPolicy",
        "OntologyRevisionSummary",
        "OntologyRevision",
        "OntologyChange",
        "OntologyDiff",
        "OntologyRevisionDiff",
        "OntologyMutation",
        "FormReleaseRef",
        "FormInteractionOutputMappingIdentity",
        "FormInteractionOutputMappingRegistry",
        "WorkflowInteractionIdentity",
        "FormInteractionAssignment",
        "FormInteractionTaskBinding",
        "FormInteractionRequest",
        "HumanTaskAssignmentPolicy",
        "HumanTaskSummary",
        "HumanTask",
        "HumanTaskMutation",
    ] {
        assert_eq!(
            schemas[name]["additionalProperties"], false,
            "{name} must reject undocumented fields"
        );
    }
    assert_eq!(
        schemas["OntologyRevision"]["properties"]["migrationPolicy"]["$ref"],
        "#/components/schemas/OntologyMigrationPolicy"
    );
    assert_eq!(
        schemas["OntologyMutation"]["properties"]["diff"]["allOf"][0]["$ref"],
        "#/components/schemas/OntologyDiff"
    );
    assert_eq!(
        schemas["HumanTask"]["properties"]["formRelease"]["$ref"],
        "#/components/schemas/FormReleaseRef"
    );
    assert_eq!(
        schemas["HumanTask"]["properties"]["interactionRequest"]["allOf"][0]["$ref"],
        "#/components/schemas/FormInteractionRequest"
    );
    assert_eq!(
        schemas["FormInteractionOutputMapping"]["discriminator"]["propertyName"],
        "kind"
    );

    for (success_schema, data_schema) in [
        ("OntologySuccessResponse", "Ontology"),
        ("OntologyListSuccessResponse", "OntologyList"),
        (
            "OntologyRevisionSummaryListSuccessResponse",
            "OntologyRevisionSummaryList",
        ),
        ("OntologyRevisionSuccessResponse", "OntologyRevision"),
        ("OntologyMutationSuccessResponse", "OntologyMutation"),
        ("OntologyDiffSuccessResponse", "OntologyRevisionDiff"),
        ("HumanTaskSuccessResponse", "HumanTask"),
        ("HumanTaskListSuccessResponse", "HumanTaskList"),
        ("HumanTaskMutationSuccessResponse", "HumanTaskMutation"),
    ] {
        assert_eq!(
            schemas[success_schema]["allOf"][0]["properties"]["data"]["$ref"],
            format!("#/components/schemas/{data_schema}")
        );
    }

    for (path, response) in [
        (
            "/organizations/{organization_id}/projects/{project_id}/ontologies",
            "OntologyListSuccess200",
        ),
        (
            "/organizations/{organization_id}/ontologies/{ontology_id}",
            "OntologySuccess200",
        ),
        (
            "/organizations/{organization_id}/ontologies/{ontology_id}/revisions",
            "OntologyRevisionSummaryListSuccess200",
        ),
        (
            "/organizations/{organization_id}/ontologies/{ontology_id}/revisions/{revision_id}",
            "OntologyRevisionSuccess200",
        ),
        (
            "/organizations/{organization_id}/ontologies/{ontology_id}/revisions/{from_revision_id}/diff/{to_revision_id}",
            "OntologyDiffSuccess200",
        ),
        (
            "/organizations/{organization_id}/projects/{project_id}/human-tasks",
            "HumanTaskListSuccess200",
        ),
        (
            "/organizations/{organization_id}/human-tasks/{human_task_id}",
            "HumanTaskSuccess200",
        ),
    ] {
        assert_eq!(
            document["paths"][path]["get"]["responses"]["200"]["$ref"],
            format!("#/components/responses/{response}"),
            "GET {path} must use its exact success response"
        );
    }

    for path in [
        "/organizations/{organization_id}/projects/{project_id}/ontologies",
        "/organizations/{organization_id}/ontologies/{ontology_id}/revisions",
    ] {
        for status in ["200", "201"] {
            assert_eq!(
                document["paths"][path]["post"]["responses"][status]["$ref"],
                format!("#/components/responses/OntologyMutationSuccess{status}")
            );
        }
    }
    for path in [
        "/organizations/{organization_id}/human-tasks/{human_task_id}/claim",
        "/organizations/{organization_id}/human-tasks/{human_task_id}/release",
        "/organizations/{organization_id}/human-tasks/{human_task_id}/submission",
    ] {
        assert_eq!(
            document["paths"][path]["post"]["responses"]["200"]["$ref"],
            "#/components/responses/HumanTaskMutationSuccess200"
        );
    }

    for path in document["paths"]
        .as_object()
        .expect("OpenAPI paths")
        .values()
    {
        for operation in path.as_object().expect("OpenAPI path item").values() {
            let is_workflow = operation["tags"]
                .as_array()
                .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some("Workflow")));
            if !is_workflow {
                continue;
            }
            for (status, response) in operation["responses"]
                .as_object()
                .expect("OpenAPI responses")
            {
                if status.starts_with('2') {
                    assert_ne!(
                        response["$ref"],
                        format!("#/components/responses/Success{status}"),
                        "Workflow success response {status} must be operation-specific"
                    );
                }
            }
        }
    }
    Ok(())
}
