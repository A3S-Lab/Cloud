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
        "invalid Cloud config: events.provider memory is allowed only for the development all-in-one process; production and split process roles require nats"
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
        "security_providers(",
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
