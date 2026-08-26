use super::*;

#[test]
fn agent_requests_and_responses_are_closed_typed_and_provider_bound() -> Result<()> {
    let app = contract_test_application()?;
    let document = generate_openapi_contract(&app)?;
    let schemas = &document["components"]["schemas"];

    for name in [
        "AgentConversation",
        "AgentConversationMutation",
        "AgentReleaseBinding",
        "AgentProviderProfile",
        "AgentExecution",
        "AgentExecutionMutation",
        "AgentCodeRunIdentity",
        "AgentCodeChangeSet",
        "AgentExecutionChangeSet",
        "AgentExecutionEvent",
        "AgentExecutionEventPage",
    ] {
        assert_eq!(
            schemas[name]["additionalProperties"], false,
            "{name} must reject undocumented fields"
        );
    }
    assert_eq!(
        schemas["AgentProviderProfile"]["properties"]["kind"]["enum"],
        json!(["a3s.code", "reference.echo"])
    );
    assert_eq!(
        schemas["AgentProviderProfile"]["properties"]["protocol"]["enum"],
        json!(["a3s.cloud.agent-provider.v1"])
    );
    assert_eq!(
        schemas["AgentExecution"]["properties"]["provider"]["$ref"],
        "#/components/schemas/AgentProviderProfile"
    );
    assert_eq!(
        schemas["AgentCodeChangeSet"]["properties"]["identity"]["$ref"],
        "#/components/schemas/AgentCodeRunIdentity"
    );
    assert_eq!(
        schemas["AgentExecutionEventPage"]["properties"]["records"]["items"]["$ref"],
        "#/components/schemas/AgentExecutionEvent"
    );

    let conversation_collection = &document["paths"]
        ["/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/agent-conversations"];
    assert_eq!(
        conversation_collection["get"]["responses"]["200"]["$ref"],
        "#/components/responses/AgentConversationListSuccess200"
    );
    for status in ["200", "201"] {
        assert_eq!(
            conversation_collection["post"]["responses"][status]["$ref"],
            format!("#/components/responses/AgentConversationMutationSuccess{status}")
        );
    }

    let execution_collection = &document["paths"]
        ["/organizations/{organization_id}/agent-conversations/{conversation_id}/executions"];
    assert_eq!(
        execution_collection["get"]["responses"]["200"]["$ref"],
        "#/components/responses/AgentExecutionListSuccess200"
    );
    for status in ["200", "202"] {
        assert_eq!(
            execution_collection["post"]["responses"][status]["$ref"],
            format!("#/components/responses/AgentExecutionMutationSuccess{status}")
        );
    }
    let request =
        &execution_collection["post"]["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(
        request["properties"]["providerKind"]["enum"],
        json!(["a3s.code", "reference.echo"])
    );
    assert_eq!(request["properties"]["providerKind"]["default"], "a3s.code");
    assert!(!request["required"]
        .as_array()
        .expect("Agent start required fields")
        .contains(&json!("providerKind")));

    let execution = &document["paths"]
        ["/organizations/{organization_id}/agent-executions/{execution_id}"]["get"];
    assert_eq!(
        execution["responses"]["200"]["$ref"],
        "#/components/responses/AgentExecutionSuccess200"
    );
    let cancel = &document["paths"]
        ["/organizations/{organization_id}/agent-executions/{execution_id}/cancel"]["post"];
    for status in ["200", "202"] {
        assert_eq!(
            cancel["responses"][status]["$ref"],
            format!("#/components/responses/AgentExecutionMutationSuccess{status}")
        );
    }
    let change_set = &document["paths"]
        ["/organizations/{organization_id}/agent-executions/{execution_id}/changes"]["get"];
    assert_eq!(
        change_set["responses"]["200"]["$ref"],
        "#/components/responses/AgentExecutionChangeSetSuccess200"
    );
    let events = &document["paths"]
        ["/organizations/{organization_id}/agent-conversations/{conversation_id}/events"]["get"];
    assert_eq!(
        events["responses"]["200"]["$ref"],
        "#/components/responses/AgentExecutionEventPageSuccess200"
    );
    Ok(())
}
