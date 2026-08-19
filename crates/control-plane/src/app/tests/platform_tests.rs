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
        "connect_postgres(",
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
fn postgres_repositories_have_one_typed_composition_boundary() {
    let composition = include_str!("../../app.rs");
    let adapters = include_str!("../postgres_adapters.rs");
    let repositories = [
        "PostgresIdentityRepository",
        "PostgresProjectsRepository",
        "PostgresOntologyRepository",
        "PostgresWorkflowDefinitionRepository",
        "PostgresWorkflowGoalRepository",
        "PostgresWorkflowRunRepository",
        "PostgresFormRepository",
        "PostgresHumanTaskRepository",
        "PostgresSearchRepository",
        "PostgresAuditRecordRepository",
        "PostgresNotificationRepository",
        "PostgresPluginRegistryRepository",
        "PostgresNodeRepository",
        "PostgresBuildRunRepository",
        "PostgresExecutionRepository",
        "PostgresExecutionTemplateRepository",
        "PostgresAgentRepository",
        "PostgresWorkloadRepository",
        "PostgresResourceClaimRepository",
        "PostgresEdgeRepository",
        "PostgresAssetRepository",
        "PostgresSecretRepository",
        "PostgresConnectorProfileRepository",
        "PostgresDurableCellApplicationRepository",
        "PostgresDurableCellDeploymentRepository",
        "PostgresConnectorExecutionAttemptRepository",
        "PostgresSourceRevisionRepository",
        "PostgresSourceSubscriptionRepository",
        "PostgresGithubConnectionRepository",
        "PostgresOperationRepository",
        "PostgresOutboxRepository",
    ];

    for line in composition.lines() {
        assert!(
            !(line.contains("Postgres") && line.contains("Repository::new(")),
            "the process composition root bypassed the typed PostgreSQL adapter factory: {line}"
        );
    }
    assert_eq!(
        composition.matches("PostgresAdapterFactory::new(").count(),
        2,
        "API/Worker and dedicated Relay must be the only PostgreSQL adapter selection roots"
    );
    assert_eq!(composition.matches(".api_worker()").count(), 1);
    assert_eq!(composition.matches(".relay()").count(), 1);
    assert_eq!(composition.matches(".connector_attempts()").count(), 1);
    assert_eq!(composition.matches(".outbox()").count(), 1);

    for repository in repositories {
        assert_eq!(
            adapters.matches(&format!("{repository}::new(")).count(),
            1,
            "{repository} must have one constructor rule in the sole adapter boundary"
        );
    }

    fn collect_rust_sources(root: &std::path::Path, sources: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(root).expect("read Rust source directory") {
            let path = entry.expect("read Rust source entry").path();
            if path.is_dir() {
                collect_rust_sources(&path, sources);
            } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("rs") {
                sources.push(path);
            }
        }
    }

    let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let adapter_path = source_root.join("app").join("postgres_adapters.rs");
    let mut sources = Vec::new();
    collect_rust_sources(&source_root, &mut sources);
    for path in sources {
        if path == adapter_path {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read Rust source");
        for repository in repositories {
            assert!(
                !source.contains(&format!("{repository}::new(")),
                "{} bypassed the sole PostgreSQL adapter constructor boundary with {repository}",
                path.display()
            );
        }
    }

    for family in [
        "IdentityPostgresAdapters::new",
        "ProjectPostgresAdapters::new",
        "WorkflowPostgresAdapters::new",
        "NotificationPostgresAdapters::new",
        "PluginPostgresAdapters::new",
        "FleetPostgresAdapters::new",
        "WorkloadPostgresAdapters::new",
        "EdgePostgresAdapters::new",
        "AssetPostgresAdapters::new",
        "SourcePostgresAdapters::new",
    ] {
        assert!(
            adapters.contains(family),
            "the bounded PostgreSQL adapter family {family} disappeared"
        );
    }
    for forbidden in [
        "connect_and_migrate(",
        "connect_postgres(",
        "migrate_postgres(",
        "Database::new(",
        "sql_query!(",
        ".await",
        "tokio::",
    ] {
        assert!(
            !adapters.contains(forbidden),
            "the adapter factory acquired I/O or persistence behavior: {forbidden}"
        );
    }
}

#[test]
fn postgres_schema_mutation_has_one_non_serving_process_root() {
    let application = include_str!("../../app.rs");
    let server = include_str!("../../main.rs");
    let migrator = include_str!("../../bin/a3s-cloud-migrate.rs");
    let development = include_str!("../../../../../tools/dev/run_cloud.sh");
    let persistence = include_str!("../../infrastructure/postgres.rs");

    assert_eq!(
        application.matches("connect_postgres(").count(),
        2,
        "API/Worker and Relay must share the read-only schema-admitting connection path"
    );
    for (name, source) in [
        ("serving application", application),
        ("HTTP server", server),
    ] {
        for forbidden in ["migrate_postgres(", "Migrator::new("] {
            assert!(
                !source.contains(forbidden),
                "{name} acquired schema mutation authority through {forbidden}"
            );
        }
    }

    for required in [
        "migrate_postgres(&postgres_url",
        "report.is_up_to_date()",
        "report.applied.join",
    ] {
        assert!(
            migrator.contains(required),
            "one-shot migrator lost required behavior {required}"
        );
    }
    for forbidden in [
        "build_application(",
        "AxumAdapter",
        ".serve",
        "connect_postgres(",
    ] {
        assert!(
            !migrator.contains(forbidden),
            "one-shot migrator became a serving process through {forbidden}"
        );
    }
    assert_eq!(
        persistence.matches("Migrator::new(").count(),
        1,
        "Cloud must delegate schema execution to exactly one A3S ORM migrator"
    );
    let migration_position = development
        .find("\"$migration_bin\" config/cloud.acl")
        .expect("development launcher must run the one-shot migration");
    let serving_position = development
        .find("exec \"$api_bin\" config/cloud.acl")
        .expect("development launcher must start the serving process");
    assert!(
        migration_position < serving_position,
        "development startup must migrate before it starts serving"
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
