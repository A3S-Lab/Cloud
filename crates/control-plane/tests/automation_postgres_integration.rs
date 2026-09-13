//! Focused AUT0 PostgreSQL recovery gates.
//!
//! Kept as a separate integration binary so webhook/schedule persistence
//! certification does not depend on unrelated postgres_integration support
//! modules that currently drift from production types.

use a3s_cloud_contracts::{
    AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationRevisionV1,
    AutomationWebhookEndpointV1, AutomationWebhookRequestV1, AutomationWebhookSecretReferenceV1,
    AutomationWebhookSignatureAlgorithmV1, AutomationWebhookSignatureV1,
};
use a3s_cloud_control_plane::infrastructure::{
    connect_postgres, migrate_postgres, PostgresBootstrapError, PostgresMigrationReport,
};
use a3s_cloud_control_plane::modules::automations::{
    AdmitAutomationWebhookDelivery, AutomationScheduleState, AutomationScheduleStateKey,
    AutomationWebhookAdmissionService, AutomationWebhookEndpointQueryService,
    AutomationWebhookEndpointScope, AutomationWebhookInvocationFactory,
    AutomationWebhookInvocationRequest, ChangeAutomationWebhookEndpoint,
    CommitAutomationScheduleCursor, CreateAutomationWebhookEndpoint, EndpointLifecycleAction,
    IAutomationScheduleStateRepository, IAutomationWebhookRepository,
    IAutomationWebhookSchemaValidator, IAutomationWebhookSignatureVerifier,
    PostgresAutomationScheduleStateRepository, PostgresAutomationWebhookRepository,
    ReleaseAutomationScheduleLease, ReserveAutomationScheduleLease,
    ResolveAutomationWebhookEndpoint,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, Sha256Digest,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use uuid::Uuid;

struct AcceptAutomationWebhookPorts;

#[async_trait]
impl IAutomationWebhookSignatureVerifier for AcceptAutomationWebhookPorts {
    async fn verify(
        &self,
        _endpoint: &AutomationWebhookEndpointV1,
        _request: &AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait]
impl IAutomationWebhookSchemaValidator for AcceptAutomationWebhookPorts {
    async fn validate(
        &self,
        _endpoint: &AutomationWebhookEndpointV1,
        _request: &AutomationWebhookRequestV1,
    ) -> Result<(), String> {
        Ok(())
    }
}


const AUTOMATION_WEBHOOK_DEFINITION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/aut0.1/automation-definition-webhook.acl"
));

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn automation_webhook_postgres_scope_replay_and_reconnect_are_durable() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_automation_webhook_postgres)
        .await
        .expect("Automation webhook PostgreSQL recovery gate");
}

async fn exercise_automation_webhook_postgres(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000201")?;
    let project_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000202")?;
    let environment_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000404")?;
    seed_automation_webhook_scope(&database, organization_id, project_id, environment_id).await?;

    let definition = AutomationDefinitionV1::parse_acl(AUTOMATION_WEBHOOK_DEFINITION)
        .map_err(std::io::Error::other)?;
    let revision = AutomationRevisionV1::from_definition(
        Uuid::from_u128(0x018f0000000070008000000000000408),
        1,
        None,
        definition.spec().clone(),
    )
    .map_err(std::io::Error::other)?;
    let repository = Arc::new(PostgresAutomationWebhookRepository::new(executor.clone()));
    let admission = AutomationWebhookAdmissionService::new(
        repository.clone(),
        Arc::new(AcceptAutomationWebhookPorts),
        Arc::new(AcceptAutomationWebhookPorts),
    );
    let created = admission
        .create_endpoint(CreateAutomationWebhookEndpoint {
            endpoint_id: Uuid::from_u128(0x018f0000000070008000000000000409),
            endpoint_key: "release-hook".into(),
            signing_secret: AutomationWebhookSecretReferenceV1 {
                secret_id: Uuid::from_u128(0x018f000000007000800000000000040a),
                version: 4,
            },
            max_body_bytes: 4_096,
            revision: revision.clone(),
            created_at: automation_timestamp(1_000),
        })
        .await?;
    let scope = AutomationWebhookEndpointScope {
        organization_id,
        project_id,
        environment_id,
    };
    let query = AutomationWebhookEndpointQueryService::new(repository.clone());
    assert_eq!(
        query
            .resolve(ResolveAutomationWebhookEndpoint {
                scope,
                endpoint_key: "release-hook".into(),
            })
            .await?
            .expect("scoped endpoint")
            .endpoint,
        created.endpoint
    );

    let endpoint = created.endpoint.clone();
    let received_at = automation_timestamp(1_010);
    let request = AutomationWebhookRequestV1::from_json(
        &endpoint,
        Uuid::from_u128(0x018f0000000070008000000000000410),
        AutomationWebhookSignatureV1 {
            algorithm: AutomationWebhookSignatureAlgorithmV1::HmacSha256,
            key_version: endpoint.signing_secret.version,
            value: format!("hmac-sha256:{}", "a".repeat(64)),
        },
        "application/json",
        br#"{"release":"stable"}"#,
        received_at,
    )
    .map_err(std::io::Error::other)?;
    let invocation = AutomationWebhookInvocationFactory::build(AutomationWebhookInvocationRequest {
        endpoint: &endpoint,
        revision: &revision,
        request: &request,
        invocation_id: Uuid::from_u128(0x018f0000000070008000000000000411),
        requested_at: received_at,
        authorization: AutomationInvocationAuthorizationV1 {
            policy_digest: revision
                .spec()
                .definition
                .authorization
                .policy_digest
                .clone(),
            grant_snapshot_digest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .into(),
            principal_id: None,
        },
        correlation_id: Uuid::from_u128(0x018f0000000070008000000000000412),
        causation_id: None,
    })
    .map_err(std::io::Error::other)?;
    let first = admission
        .admit(AdmitAutomationWebhookDelivery {
            request: request.clone(),
            invocation: Some(invocation.clone()),
            receipt_id: Uuid::from_u128(0x018f0000000070008000000000000413),
            recorded_at: received_at,
        })
        .await?;
    assert!(!first.replayed);

    let replay = admission
        .admit(AdmitAutomationWebhookDelivery {
            request: request.clone(),
            invocation: Some(invocation),
            receipt_id: Uuid::from_u128(0x018f0000000070008000000000000414),
            recorded_at: received_at + chrono::Duration::seconds(1),
        })
        .await?;
    assert!(replay.replayed);
    assert_eq!(
        repository
            .find_delivery(endpoint.endpoint_id, request.delivery_id)
            .await?
            .expect("durable delivery")
            .request
            .body_digest,
        request.body_digest
    );

    let changed_at = received_at + chrono::Duration::seconds(2);
    let first_lifecycle = admission.clone();
    let second_lifecycle = admission.clone();
    let (first_transition, second_transition) = tokio::join!(
        first_lifecycle.change_endpoint(ChangeAutomationWebhookEndpoint {
            endpoint_id: endpoint.endpoint_id,
            expected_generation: 1,
            action: EndpointLifecycleAction::Disable,
            changed_at,
        }),
        second_lifecycle.change_endpoint(ChangeAutomationWebhookEndpoint {
            endpoint_id: endpoint.endpoint_id,
            expected_generation: 1,
            action: EndpointLifecycleAction::Disable,
            changed_at,
        }),
    );
    let transitions = [first_transition, second_transition];
    assert_eq!(
        transitions.iter().filter(|result| result.is_ok()).count(),
        1,
        "exactly one lifecycle writer may win the generation fence"
    );
    assert_eq!(
        transitions
            .iter()
            .filter(|result| matches!(result, Err(error) if matches!(
                error,
                a3s_cloud_control_plane::modules::shared_kernel::application::ApplicationError::Conflict(_)
            )))
            .count(),
        1,
        "the stale lifecycle writer must receive a conflict"
    );

    drop(query);
    drop(admission);
    drop(repository);
    drop(database);
    drop(executor);

    let recovered_executor = connect_postgres(&url, 8).await?;
    let recovered = PostgresAutomationWebhookRepository::new(recovered_executor);
    let recovered_endpoint = recovered
        .find_endpoint_by_key(organization_id, project_id, environment_id, "release-hook")
        .await?
        .expect("endpoint after reconnect");
    assert_eq!(recovered_endpoint.endpoint.endpoint_id, endpoint.endpoint_id);
    assert_eq!(recovered_endpoint.endpoint.endpoint_key, endpoint.endpoint_key);
    assert_eq!(recovered_endpoint.endpoint.revision_id, endpoint.revision_id);
    assert_eq!(
        recovered_endpoint.endpoint.state,
        a3s_cloud_contracts::AutomationWebhookEndpointStateV1::Disabled
    );
    assert_eq!(recovered_endpoint.endpoint.generation, 2);
    let recovered_delivery = recovered
        .find_delivery(endpoint.endpoint_id, request.delivery_id)
        .await?
        .expect("delivery after reconnect");
    assert_eq!(recovered_delivery.request.body_digest, request.body_digest);
    assert!(recovered
        .find_endpoint_by_key(
            organization_id,
            project_id,
            Uuid::from_u128(environment_id.as_u128() ^ 1),
            "release-hook",
        )
        .await?
        .is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn automation_schedule_state_postgres_fences_and_recovers() {
    let Some(admin_url) = std::env::var("A3S_CLOUD_TEST_POSTGRES_URL").ok() else {
        return;
    };
    run_isolated_postgres(&admin_url, exercise_automation_schedule_state_postgres)
        .await
        .expect("Automation schedule-state PostgreSQL recovery gate");
}

async fn exercise_automation_schedule_state_postgres(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000221")?;
    let project_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000222")?;
    let environment_id = Uuid::parse_str("018f0000-0000-7000-8000-000000000223")?;
    seed_automation_webhook_scope(&database, organization_id, project_id, environment_id).await?;

    let key = AutomationScheduleStateKey::new(
        OrganizationId::from_uuid(organization_id),
        ProjectId::from_uuid(project_id),
        EnvironmentId::from_uuid(environment_id),
        Uuid::from_u128(0x018f0000000070008000000000000421),
    );
    let state = AutomationScheduleState::new(
        key,
        Uuid::from_u128(0x018f0000000070008000000000000422),
        Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))?,
        automation_timestamp(1_000),
        automation_timestamp(900),
    )?;
    let repository = Arc::new(PostgresAutomationScheduleStateRepository::new(
        executor.clone(),
    ));
    repository.create(state.clone()).await?;

    let first_reservation = ReserveAutomationScheduleLease {
        key,
        owner_id: Uuid::from_u128(0x018f0000000070008000000000000423),
        lease_id: Uuid::from_u128(0x018f0000000070008000000000000424),
        reserved_at: automation_timestamp(1_001),
        lease_expires_at: automation_timestamp(1_101),
    };
    let second_reservation = ReserveAutomationScheduleLease {
        key,
        owner_id: Uuid::from_u128(0x018f0000000070008000000000000425),
        lease_id: Uuid::from_u128(0x018f0000000070008000000000000426),
        reserved_at: automation_timestamp(1_001),
        lease_expires_at: automation_timestamp(1_101),
    };
    let (first, second) = tokio::join!(
        repository.reserve(first_reservation.clone()),
        repository.reserve(second_reservation),
    );
    let reserved = match (first, second) {
        (Ok(state), Err(_)) | (Err(_), Ok(state)) => state,
        (first, second) => {
            return Err(format!(
                "exactly one schedule lease reservation must win: {first:?}; {second:?}"
            )
            .into())
        }
    };
    assert_eq!(reserved.lease_generation(), 1);
    let lease = reserved.lease().expect("reserved lease");
    let commit = repository
        .commit_cursor(CommitAutomationScheduleCursor {
            key,
            owner_id: lease.owner_id(),
            lease_id: lease.lease_id(),
            lease_generation: reserved.lease_generation(),
            evaluated_through: automation_timestamp(1_050),
            committed_at: automation_timestamp(1_060),
        })
        .await?;
    assert_eq!(commit.cursor_at(), automation_timestamp(1_050));
    assert!(commit.lease().is_none());

    drop(repository);
    drop(database);
    drop(executor);

    let recovered_executor = connect_postgres(&url, 8).await?;
    let recovered = PostgresAutomationScheduleStateRepository::new(recovered_executor.clone());
    let recovered_state = recovered.find(key).await?.expect("state after reconnect");
    assert_eq!(recovered_state.cursor_at(), automation_timestamp(1_050));
    assert_eq!(recovered_state.lease_generation(), 1);
    assert!(recovered_state.lease().is_none());

    let reserved_again = recovered
        .reserve(ReserveAutomationScheduleLease {
            key,
            owner_id: Uuid::from_u128(0x018f0000000070008000000000000427),
            lease_id: Uuid::from_u128(0x018f0000000070008000000000000428),
            reserved_at: automation_timestamp(1_061),
            lease_expires_at: automation_timestamp(1_161),
        })
        .await?;
    let lease_again = reserved_again.lease().expect("second lease");
    let released = recovered
        .release_lease(ReleaseAutomationScheduleLease {
            key,
            owner_id: lease_again.owner_id(),
            lease_id: lease_again.lease_id(),
            lease_generation: reserved_again.lease_generation(),
            released_at: automation_timestamp(1_062),
        })
        .await?;
    assert_eq!(released.lease_generation(), 2);
    assert_eq!(released.cursor_at(), automation_timestamp(1_050));
    assert!(released.lease().is_none());

    drop(recovered);
    drop(recovered_executor);
    let final_executor = connect_postgres(&url, 8).await?;
    let final_repository = PostgresAutomationScheduleStateRepository::new(final_executor);
    let final_state = final_repository
        .find(key)
        .await?
        .expect("released state after reconnect");
    assert_eq!(final_state.lease_generation(), 2);
    assert!(final_state.lease().is_none());
    assert!(final_repository
        .find(AutomationScheduleStateKey::new(
            key.organization_id,
            key.project_id,
            key.environment_id,
            Uuid::from_u128(0x018f0000000070008000000000000429),
        ))
        .await?
        .is_none());
    Ok(())
}


fn automation_timestamp(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("canonical timestamp")
}

async fn seed_automation_webhook_scope(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", 'Automation integration organization', 'automation-integration-organization', 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", ")
            .bind(project_id)
            .append(", 'Automation integration project', 'automation-integration-project', 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id)
            .append(", ")
            .bind(project_id)
            .append(", ")
            .bind(environment_id)
            .append(", 'Automation integration environment', 'automation-integration-environment', 1, ")
            .bind(now)
            .append(")"),
        )
        .await?;
    Ok(())
}

async fn migrate_and_connect_for_test(
    url: &str,
    max_connections: usize,
) -> Result<PostgresExecutor, PostgresBootstrapError> {
    migrate_for_test(url, max_connections).await?;
    connect_postgres(url, max_connections).await
}

async fn migrate_for_test(
    url: &str,
    max_connections: usize,
) -> Result<PostgresMigrationReport, PostgresBootstrapError> {
    let serving_role = serving_role_for_url(url)?;
    migrate_postgres(url, max_connections, &serving_role).await
}

fn serving_role_for_url(database_url: &str) -> Result<String, PostgresBootstrapError> {
    let parsed = url::Url::parse(database_url).map_err(|error| {
        PostgresBootstrapError::ComponentConfiguration(format!(
            "invalid isolated PostgreSQL URL: {error}"
        ))
    })?;
    let database_name = parsed.path().trim_start_matches('/');
    if database_name.is_empty() || database_name.contains('/') {
        return Err(PostgresBootstrapError::ComponentConfiguration(
            "isolated PostgreSQL URL must name exactly one database".into(),
        ));
    }
    Ok(format!("{database_name}_serving"))
}

struct IsolatedPostgresDatabase {
    admin_url: String,
    database_name: String,
    database_url: String,
    serving_role: String,
}

impl IsolatedPostgresDatabase {
    async fn create(admin_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let database_name = format!("a3s_cloud_test_{}", Uuid::new_v4().simple());
        let mut database_url = url::Url::parse(admin_url)?;
        database_url.set_path(&format!("/{database_name}"));
        let serving_role = format!("{database_name}_serving");

        let admin = PostgresExecutor::connect_no_tls(admin_url, 2)?;
        let connection = admin.pool().get().await?;
        connection
            .batch_execute(&format!("create database \"{database_name}\""))
            .await?;
        if let Err(source) = connection
            .batch_execute(&format!(
                "create role \"{serving_role}\" nologin nosuperuser nocreatedb nocreaterole noreplication"
            ))
            .await
        {
            let cleanup = connection
                .batch_execute(&format!(
                    "drop database if exists \"{database_name}\" with (force)"
                ))
                .await;
            return match cleanup {
                Ok(()) => Err(source.into()),
                Err(cleanup_error) => Err(std::io::Error::other(format!(
                    "could not create isolated serving role: {source}; database cleanup also failed: {cleanup_error}"
                ))
                .into()),
            };
        }

        Ok(Self {
            admin_url: admin_url.to_owned(),
            database_name,
            database_url: database_url.to_string(),
            serving_role,
        })
    }

    fn url(&self) -> &str {
        &self.database_url
    }

    async fn cleanup(&self) -> Result<(), Box<dyn std::error::Error>> {
        let admin = PostgresExecutor::connect_no_tls(&self.admin_url, 2)?;
        let connection = admin.pool().get().await?;
        let database_cleanup = connection
            .batch_execute(&format!(
                "drop database if exists \"{}\" with (force)",
                self.database_name
            ))
            .await;
        let role_cleanup = connection
            .batch_execute(&format!("drop role if exists \"{}\"", self.serving_role))
            .await;
        match (database_cleanup, role_cleanup) {
            (Ok(()), Ok(())) => {}
            (Err(database_error), Ok(())) => return Err(database_error.into()),
            (Ok(()), Err(role_error)) => return Err(role_error.into()),
            (Err(database_error), Err(role_error)) => {
                return Err(std::io::Error::other(format!(
                    "isolated database cleanup failed: {database_error}; role cleanup also failed: {role_error}"
                ))
                .into());
            }
        }
        let row = connection
            .query_one(
                "select exists(select 1 from pg_database where datname = $1), exists(select 1 from pg_roles where rolname = $2)",
                &[&self.database_name, &self.serving_role],
            )
            .await?;
        let database_still_exists: bool = row.get(0);
        let role_still_exists: bool = row.get(1);
        if database_still_exists || role_still_exists {
            return Err(std::io::Error::other(format!(
                "isolated PostgreSQL resources still exist after cleanup (database={}, role={})",
                self.database_name, self.serving_role
            ))
            .into());
        }
        Ok(())
    }
}

async fn run_isolated_postgres<F, Fut>(
    admin_url: &str,
    exercise: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    let isolated = IsolatedPostgresDatabase::create(admin_url).await?;
    let result = AssertUnwindSafe(Box::pin(exercise(isolated.url().to_owned())))
        .catch_unwind()
        .await;
    let cleanup = isolated.cleanup().await;

    match result {
        Ok(Ok(())) => cleanup,
        Ok(Err(test_error)) => {
            if let Err(cleanup_error) = cleanup {
                return Err(std::io::Error::other(format!(
                    "PostgreSQL integration test failed: {test_error}; isolated database cleanup also failed: {cleanup_error}"
                ))
                .into());
            }
            Err(test_error)
        }
        Err(panic_payload) => {
            if let Err(cleanup_error) = cleanup {
                eprintln!(
                    "isolated PostgreSQL database cleanup failed after test panic: {cleanup_error}"
                );
            }
            std::panic::resume_unwind(panic_payload)
        }
    }
}
