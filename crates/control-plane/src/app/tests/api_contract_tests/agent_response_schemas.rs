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
        "HarnessAgentReleaseBinding",
        "HarnessProviderBinding",
        "HarnessWorkspaceBinding",
        "HarnessSkillBinding",
        "HarnessMcpBinding",
        "HarnessModelBinding",
        "HarnessSecretReference",
        "HarnessToolBinding",
        "HarnessInvocationProfile",
        "AgentToolPayloadIdentity",
        "AgentModelOutputEventContent",
        "AgentToolRequestEventContent",
        "AgentToolResultEventContent",
        "AgentExecutionFailureEventContent",
        "AgentExecutionRequestedEvent",
        "AgentModelOutputEvent",
        "AgentToolRequestEvent",
        "AgentToolResultEvent",
        "AgentExecutionFailedEvent",
        "AgentExecutionCompletedEvent",
        "AgentExecutionCancelledEvent",
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
        schemas["AgentExecution"]["properties"]["invocationProfile"]["allOf"][0]["$ref"],
        "#/components/schemas/HarnessInvocationProfile"
    );
    assert_eq!(
        schemas["HarnessInvocationProfile"]["properties"]["schema"]["enum"],
        json!(["a3s.cloud.harness-invocation-profile.v1"])
    );
    assert_eq!(
        schemas["HarnessProviderBinding"]["properties"]["kind"]["enum"],
        json!(["a3s.code", "reference.echo"])
    );
    assert_eq!(
        schemas["HarnessInvocationProfile"]["x-a3s-max-canonical-bytes"],
        a3s_cloud_contracts::HARNESS_INVOCATION_PROFILE_MAX_BYTES
    );
    for binding in ["skills", "mcpServers", "models", "secrets", "tools"] {
        assert_eq!(
            schemas["HarnessInvocationProfile"]["properties"][binding]["uniqueItems"], true,
            "{binding} must document immutable unique bindings"
        );
    }
    for (binding, order) in [
        ("skills", json!(["assetId", "assetReleaseId"])),
        ("mcpServers", json!(["assetId", "assetReleaseId"])),
        ("models", json!(["modelId", "modelRevisionId"])),
        ("secrets", json!(["name"])),
        ("tools", json!(["name", "revision"])),
    ] {
        assert_eq!(
            schemas["HarnessInvocationProfile"]["properties"][binding]["x-a3s-canonical-order"],
            order,
            "{binding} must document its canonical digest order"
        );
    }
    assert_eq!(
        schemas["HarnessInvocationProfile"]["properties"]["requiredCapabilities"]
            ["x-a3s-canonical-order"],
        "lexical-wire-value"
    );
    assert_eq!(
        schemas["AgentExecutionEvent"]["discriminator"]["propertyName"],
        "kind"
    );
    assert_eq!(
        schemas["AgentExecutionEvent"]["oneOf"]
            .as_array()
            .map(Vec::len),
        Some(7),
        "AgentExecutionEvent must expose exactly the documented event variants"
    );
    for (kind, variant) in [
        ("execution_requested", "AgentExecutionRequestedEvent"),
        ("model_output", "AgentModelOutputEvent"),
        ("tool_request", "AgentToolRequestEvent"),
        ("tool_result", "AgentToolResultEvent"),
        ("execution_failed", "AgentExecutionFailedEvent"),
        ("execution_completed", "AgentExecutionCompletedEvent"),
        ("execution_cancelled", "AgentExecutionCancelledEvent"),
    ] {
        let expected_reference = format!("#/components/schemas/{variant}");
        assert_eq!(
            schemas["AgentExecutionEvent"]["discriminator"]["mapping"][kind].as_str(),
            Some(expected_reference.as_str())
        );
        assert!(
            schemas["AgentExecutionEvent"]["oneOf"]
                .as_array()
                .expect("AgentExecutionEvent oneOf variants")
                .iter()
                .any(|schema| schema["$ref"] == expected_reference),
            "AgentExecutionEvent must include the {kind} variant"
        );
        assert_eq!(schemas[variant]["additionalProperties"], false);
        assert_eq!(
            schemas[variant]["properties"]["kind"]["enum"],
            json!([kind])
        );
    }
    assert_eq!(
        schemas["AgentToolRequestEvent"]["properties"]["content"]["$ref"],
        "#/components/schemas/AgentToolRequestEventContent"
    );
    assert_eq!(
        schemas["AgentToolResultEvent"]["properties"]["content"]["$ref"],
        "#/components/schemas/AgentToolResultEventContent"
    );
    assert_eq!(
        schemas["AgentToolRequestEventContent"]["properties"]["request"]["$ref"],
        "#/components/schemas/AgentToolPayloadIdentity"
    );
    assert_eq!(
        schemas["AgentToolResultEventContent"]["properties"]["result"]["$ref"],
        "#/components/schemas/AgentToolPayloadIdentity"
    );
    for forbidden in ["payload", "value", "body", "secretMaterial"] {
        assert!(
            schemas["AgentToolPayloadIdentity"]["properties"]
                .get(forbidden)
                .is_none(),
            "Tool payload identity exposed {forbidden}"
        );
    }
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
