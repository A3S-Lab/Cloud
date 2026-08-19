use super::*;

#[test]
fn production_box_acl_enforces_migrate_then_serve_secret_boundaries() {
    use a3s_box_core::compose::{normalize_compose, ComposeSourceFormat};

    let source = include_str!("../../../../../deploy/production/compose.acl");
    let environment = std::collections::HashMap::from([
        (
            "A3S_CLOUD_IMAGE".to_string(),
            format!("ghcr.io/a3s-lab/cloud@sha256:{}", "a".repeat(64)),
        ),
        (
            "A3S_CLOUD_VAULT_ADDR".to_string(),
            "https://vault.example.invalid".to_string(),
        ),
    ]);
    let normalized = normalize_compose(source, ComposeSourceFormat::Acl, &environment)
        .expect("production Box ACL must normalize through Box itself");

    assert_eq!(
        normalized.services.keys().cloned().collect::<Vec<_>>(),
        ["api", "migrate", "nats", "postgres", "relay", "worker"]
    );
    let migrator = &normalized.services["migrate"];
    assert_eq!(
        migrator.secret_environment,
        std::collections::BTreeMap::from([(
            "A3S_CLOUD_POSTGRES_MIGRATION_URL".to_string(),
            "A3S_CLOUD_POSTGRES_MIGRATION_URL".to_string(),
        )])
    );
    assert_eq!(migrator.depends_on["postgres"].condition, "service_healthy");
    let postgres = &normalized.services["postgres"];
    assert_eq!(
        postgres
            .environment
            .get("POSTGRES_USER")
            .map(String::as_str),
        Some("a3s_cloud_bootstrap")
    );
    assert_eq!(
        postgres.secret_environment,
        std::collections::BTreeMap::from([
            (
                "A3S_CLOUD_POSTGRES_MIGRATION_PASSWORD".to_string(),
                "A3S_CLOUD_POSTGRES_MIGRATION_PASSWORD".to_string(),
            ),
            (
                "A3S_CLOUD_POSTGRES_SERVING_PASSWORD".to_string(),
                "A3S_CLOUD_POSTGRES_SERVING_PASSWORD".to_string(),
            ),
            (
                "POSTGRES_PASSWORD".to_string(),
                "A3S_CLOUD_POSTGRES_BOOTSTRAP_PASSWORD".to_string(),
            ),
        ])
    );
    assert_eq!(normalized.services["api"].ports, ["8080:8080", "8443:8443"]);

    for (service_name, role) in [("api", "api"), ("worker", "worker"), ("relay", "relay")] {
        let service = &normalized.services[service_name];
        assert_eq!(
            service.command.as_deref(),
            Some(
                ["/etc/a3s-cloud/cloud.acl", "--role", role]
                    .map(str::to_string)
                    .as_slice()
            )
        );
        assert_eq!(
            service.depends_on["migrate"].condition,
            "service_completed_successfully"
        );
        assert_eq!(
            service
                .secret_environment
                .get("A3S_CLOUD_POSTGRES_URL")
                .map(String::as_str),
            Some("A3S_CLOUD_POSTGRES_URL")
        );
        assert!(!service
            .secret_environment
            .values()
            .any(|source| source == "A3S_CLOUD_POSTGRES_MIGRATION_URL"));
    }

    let secret_names = |service_name: &str| {
        normalized.services[service_name]
            .secret_environment
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        secret_names("api"),
        [
            "A3S_CLOUD_BOOTSTRAP_TOKEN",
            "A3S_CLOUD_GITHUB_WEBHOOK_SECRET",
            "A3S_CLOUD_POSTGRES_URL",
            "A3S_CLOUD_S3_ACCESS_KEY_ID",
            "A3S_CLOUD_S3_SECRET_ACCESS_KEY",
            "A3S_CLOUD_VAULT_TOKEN",
        ],
        "API Secret projection widened"
    );
    assert_eq!(
        secret_names("worker"),
        [
            "A3S_CLOUD_NATS_URL",
            "A3S_CLOUD_POSTGRES_URL",
            "A3S_CLOUD_REGISTRY_CREDENTIAL",
            "A3S_CLOUD_S3_ACCESS_KEY_ID",
            "A3S_CLOUD_S3_SECRET_ACCESS_KEY",
            "A3S_CLOUD_VAULT_TOKEN",
        ],
        "Worker Secret projection widened"
    );
    assert_eq!(
        secret_names("relay"),
        ["A3S_CLOUD_NATS_URL", "A3S_CLOUD_POSTGRES_URL"],
        "Relay Secret projection widened"
    );

    let canonical = normalized
        .to_canonical_json()
        .expect("canonical production Box ACL");
    assert!(!canonical.contains("postgres://"));
    assert!(!canonical.contains("serving password"));

    let postgres_init = include_str!("../../../../../deploy/production/postgres-init.sh");
    for required in [
        "GRANT USAGE ON SCHEMAS TO a3s_cloud_serving",
        "GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO a3s_cloud_serving",
        "GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO a3s_cloud_serving",
        "ALTER ROLE a3s_cloud_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION",
        "ALTER DATABASE %I OWNER TO a3s_cloud_migrator",
        "ALTER ROLE %I NOLOGIN",
    ] {
        assert!(
            postgres_init.contains(required),
            "PostgreSQL bootstrap lost the least-privilege boundary {required}"
        );
    }
    assert!(
        !postgres_init.contains("IN SCHEMA public"),
        "serving grants must cover Cloud-owned component schemas, not only public"
    );
    for forbidden in [
        "GRANT CREATE",
        "GRANT ALL",
        "a3s_cloud_serving CREATEDB",
        "a3s_cloud_serving CREATEROLE",
        "a3s_cloud_serving SUPERUSER",
    ] {
        assert!(
            !postgres_init.contains(forbidden),
            "serving database authority widened through {forbidden}"
        );
    }
}

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
    let c0_conformance =
        include_str!("../../../../../tools/c0-conformance/run_cross_surface_gate.sh");
    let persistence = include_str!("../../infrastructure/postgres.rs");

    assert_eq!(
        application.matches("connect_postgres(").count(),
        2,
        "API/Worker and Relay must share the read-only schema-admitting connection path"
    );
    assert_eq!(
        application.matches("serving_postgres_url()").count(),
        2,
        "each serving PostgreSQL composition root must resolve only the serving credential"
    );
    assert!(
        !application.contains("migration_postgres_url()"),
        "a serving composition root acquired the migration credential"
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
        "migration_postgres_url()",
        "migrate_postgres(&migration_postgres_url",
        "report.is_up_to_date()",
        "report.applied.join",
    ] {
        assert!(
            migrator.contains(required),
            "one-shot migrator lost required behavior {required}"
        );
    }
    for forbidden in [
        "serving_postgres_url()",
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
        persistence.matches(".run(cloud_migrations())").count(),
        1,
        "Cloud must own exactly one migration manifest"
    );
    assert_eq!(
        persistence
            .matches(".verify_required(cloud_migrations())")
            .count(),
        1,
        "Cloud serving admission must reuse the ORM manifest verifier"
    );
    for required in [
        "migrate_postgres_flow(&flow_executor)",
        "migrate_postgres_queue(&boot_executor)",
    ] {
        assert!(
            persistence.contains(required),
            "the Cloud migration root stopped delegating component schema ownership through {required}"
        );
    }
    for forbidden in [
        "verify_schema_manifest",
        "expected_schema_manifest",
        "migration_manifest_query",
        "MigrationRecords",
    ] {
        assert!(
            !persistence.contains(forbidden),
            "Cloud duplicated the ORM schema-admission mechanism through {forbidden}"
        );
    }
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
    for required in [
        "A3S_CLOUD_POSTGRES_MIGRATION_URL=\"$migration_postgres_url\"",
        "unset A3S_CLOUD_POSTGRES_MIGRATION_URL",
    ] {
        assert!(
            development.contains(required),
            "development startup lost PostgreSQL credential isolation: {required}"
        );
    }

    let c0_migration_position = c0_conformance
        .find("\"$migration_binary\" \"$cloud_root/config/cloud.acl\"")
        .expect("C0 conformance must run the one-shot migration");
    let c0_serving_position = c0_conformance
        .find("\"$api_binary\" \"$cloud_root/config/cloud.acl\"")
        .expect("C0 conformance must start the serving process");
    assert!(
        c0_migration_position < c0_serving_position,
        "C0 conformance must migrate before it starts serving"
    );
    assert!(
        c0_conformance.contains("A3S_CLOUD_POSTGRES_MIGRATION_URL=\"$postgres_url\""),
        "C0 migrator must receive the migration credential"
    );
    assert!(
        c0_conformance.contains("-u A3S_CLOUD_POSTGRES_MIGRATION_URL"),
        "C0 serving process must explicitly discard the migration credential"
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
        application.contains("Some(FlowReadInfrastructure::connect(&serving_postgres_url).await?)"),
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
    assert!(reader.contains("PostgresEventStore::connect_verified("));
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
    assert_eq!(
        flow.matches("PostgresEventStore::connect_verified(")
            .count(),
        2,
        "API and worker Flow stores must use the same read-only schema admission"
    );
    assert_eq!(
        flow.matches("PostgresQueueBackend::connect_verified(")
            .count(),
        1,
        "only the worker must admit the Boot queue schema"
    );
    for forbidden in [
        "PostgresEventStore::connect(",
        "PostgresQueueBackend::connect(",
    ] {
        assert!(
            !flow.contains(forbidden),
            "a serving Flow composition root retained schema mutation through {forbidden}"
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
