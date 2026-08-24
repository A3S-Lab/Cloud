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
            "cloud.workflow.policy.v3"
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
