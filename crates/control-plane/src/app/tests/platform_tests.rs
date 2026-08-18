use super::*;

#[tokio::test]
async fn boot_shell_exposes_wrapped_platform_and_health_responses() -> Result<()> {
    let organizations = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(organizations, projects)?;
    let platform = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/platform")
                .with_header("accept", "application/json")
                .with_header("x-request-id", "018f3f56-8d4a-7c2a-9f13-5ab3d245d701"),
        )
        .await?;
    let body = response_json(&platform)?;
    assert_eq!(platform.status(), 200);
    assert_eq!(body["code"], 200);
    assert_eq!(body["data"]["name"], "a3s-cloud");
    assert_eq!(body["requestId"], "018f3f56-8d4a-7c2a-9f13-5ab3d245d701");

    let health = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/health/live")
                .with_header("accept", "application/json"),
        )
        .await?;
    let body = response_json(&health)?;
    assert_eq!(body["data"]["status"], "up");

    let readiness = app
        .call(
            BootRequest::new(HttpMethod::Get, "/api/v1/health/ready")
                .with_header("accept", "application/json"),
        )
        .await?;
    let body = response_json(&readiness)?;
    assert_eq!(body["data"]["status"], "up");
    Ok(())
}

#[tokio::test]
async fn worker_and_relay_roles_expose_only_process_status_routes() -> Result<()> {
    for role in [ProcessRole::Worker, ProcessRole::Relay] {
        let mut process_config = config();
        process_config.server.role = role;
        let app = build_process_status_application(
            &process_config,
            HealthModule::new("readiness")
                .with_route("/health/ready")
                .indicator("fixture", || async { Ok(HealthIndicatorResult::up()) }),
        )?;

        for path in [
            "/api/v1/platform",
            "/api/v1/health/live",
            "/api/v1/health/ready",
        ] {
            let response = app
                .call(
                    BootRequest::new(HttpMethod::Get, path)
                        .with_header("accept", "application/json"),
                )
                .await?;
            assert_eq!(response.status(), 200, "{role:?} must expose {path}");
        }

        for path in ["/api/v1/openapi.json", "/api/v1/organizations"] {
            let response = app
                .call(
                    BootRequest::new(HttpMethod::Get, path)
                        .with_header("accept", "application/json"),
                )
                .await?;
            assert_eq!(
                response.status(),
                404,
                "{role:?} must not expose management route {path}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn production_composition_revalidates_the_role_contract_before_io() -> Result<()> {
    let mut invalid = config();
    invalid.server.role = ProcessRole::Worker;
    let oidc_provider: Arc<dyn IOidcProviderService> =
        Arc::new(OpenIdConnectProviderService::new(&[]).map_err(BootError::Internal)?);

    let error = match build_application_with_source_resolver_and_oidc_provider(
        invalid,
        Arc::new(TestSourceResolver),
        oidc_provider,
    )
    .await
    {
        Ok(_) => {
            return Err(BootError::Internal(
                "invalid role contract was accepted".into(),
            ))
        }
        Err(error) => error,
    };
    assert_eq!(
        error.to_string(),
        "invalid Cloud config: events.provider memory is allowed only for the development all-in-one process or an API process that does not own event transport; every event-owning production or split role requires nats"
    );
    Ok(())
}

#[test]
fn relay_composition_has_one_closed_dependency_set() {
    let production = include_str!("../../app.rs");
    let relay = production
        .split_once("async fn build_relay_application(")
        .and_then(|(_, tail)| tail.split_once("\nfn build_outbox_relay("))
        .map(|(body, _)| body)
        .expect("relay composition root");

    for required in [
        "connect_and_migrate(",
        "event_publisher(",
        "build_outbox_relay(",
        "relay_readiness(",
        "ControlPlaneWorkers::relay(",
    ] {
        assert!(
            relay.contains(required),
            "relay composition lost required authority {required}"
        );
    }
    for forbidden in [
        "connect_flow(",
        "certificate_authority_provider(",
        "key_encryption_provider(",
        "build_evidence_signer(",
        "gateway_certificate_authority(",
        "log_chunk_store(",
        "bootstrap_token(",
        "GithubSourceResolver::new(",
        "OpenIdConnectProviderService::new(",
        "OutboxRelay::new(",
    ] {
        assert!(
            !relay.contains(forbidden),
            "relay composition acquired unrelated dependency {forbidden}"
        );
    }
    assert_eq!(
        production.matches("OutboxRelay::new(").count(),
        1,
        "all and relay roles must share one Outbox construction mechanism"
    );
    assert_eq!(
        production.matches("OutboxRelayConfig {").count(),
        1,
        "all and relay roles must share one Outbox timing projection"
    );
}

#[test]
fn api_and_worker_flow_capabilities_have_distinct_composition_roots() {
    let application = include_str!("../../app.rs");
    let worker_flow = application
        .split_once("let flow = if run_operations {")
        .and_then(|(_, tail)| tail.split_once("\n    let management_flow_reader ="))
        .map(|(body, _)| body)
        .expect("worker Flow composition root");
    for required in [
        "GitSourceCheckout::new(",
        "OciBuildOutputValidator::new(",
        "build_evidence_signer(",
        "FlowRuntimeRouter::new(",
        "connect_flow(",
    ] {
        assert!(
            worker_flow.contains(required),
            "worker Flow composition lost required capability {required}"
        );
    }
    assert!(
        application.contains(
            "if config.server.role.owns_event_transport() {\n        Some(event_publisher(&config).await?)"
        ),
        "API must not acquire an event publisher"
    );
    assert!(
        application.contains("Some(FlowReadInfrastructure::connect(&postgres_url).await?)"),
        "API must acquire the query-only Flow boundary"
    );

    let flow = include_str!("../../infrastructure/flow.rs");
    let reader = flow
        .split_once("impl FlowReadInfrastructure {")
        .and_then(|(_, tail)| {
            tail.split_once("\n}\n\npub(crate) fn cloud_runtime_build_compatibility")
        })
        .map(|(body, _)| body)
        .expect("Flow read infrastructure implementation");
    assert!(reader.contains("PostgresEventStore::connect("));
    for forbidden in [
        "PostgresQueueBackend::connect(",
        "BootFlowTaskManager::new(",
        "retire_incompatible_build_workflows(",
    ] {
        assert!(
            !reader.contains(forbidden),
            "API Flow reader acquired worker capability {forbidden}"
        );
    }
}

#[test]
fn process_roles_have_one_closed_capability_matrix() {
    for (role, management, workers, relay, events) in [
        (ProcessRole::All, true, true, true, true),
        (ProcessRole::Api, true, false, false, false),
        (ProcessRole::Worker, false, true, false, true),
        (ProcessRole::Relay, false, false, true, true),
    ] {
        assert_eq!(role.serves_management_api(), management, "{role:?}");
        assert_eq!(role.runs_workers(), workers, "{role:?}");
        assert_eq!(role.runs_relay(), relay, "{role:?}");
        assert_eq!(role.owns_event_transport(), events, "{role:?}");
    }
}
