use super::ProjectsDeveloperWorkflowEnvironmentAdapter;
use crate::modules::developer_workflows::application::{
    DeveloperWorkflowEnvironmentScope, IDeveloperWorkflowEnvironmentPort,
};
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[tokio::test]
async fn exact_existing_environment_is_reported_once() {
    let fixture = Fixture::new();
    let repository = Arc::new(StubEnvironmentRepository::new(Some(fixture.environment())));
    let adapter = environment_adapter(Arc::clone(&repository));

    assert!(adapter
        .environment_exists(fixture.scope())
        .await
        .expect("Projects environment evidence"));
    assert_eq!(repository.find_calls(), 1);
}

#[tokio::test]
async fn missing_environment_is_concealed_and_inconsistent_evidence_fails_closed() {
    let fixture = Fixture::new();
    let missing = environment_adapter(Arc::new(StubEnvironmentRepository::new(None)));
    assert!(!missing
        .environment_exists(fixture.scope())
        .await
        .expect("missing environment"));

    let inconsistent = environment_adapter(Arc::new(StubEnvironmentRepository::new(Some(
        Environment::create(
            fixture.organization_id,
            ProjectId::new(),
            fixture.environment_id,
            EnvironmentName::parse("production").expect("environment name"),
            Utc::now(),
        ),
    ))));
    assert!(matches!(
        inconsistent.environment_exists(fixture.scope()).await,
        Err(RepositoryError::Storage(_))
    ));
}

#[tokio::test]
async fn invalid_scope_is_rejected_before_projects_lookup() {
    let fixture = Fixture::new();
    let repository = Arc::new(StubEnvironmentRepository::new(Some(fixture.environment())));
    let adapter = environment_adapter(Arc::clone(&repository));
    let invalid = DeveloperWorkflowEnvironmentScope {
        organization_id: OrganizationId::from_uuid(uuid::Uuid::nil()),
        ..fixture.scope()
    };

    assert!(matches!(
        adapter.environment_exists(invalid).await,
        Err(RepositoryError::Forbidden(_))
    ));
    assert_eq!(repository.find_calls(), 0);
}

#[test]
fn environment_adapter_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ProjectsDeveloperWorkflowEnvironmentAdapter>();
}

fn environment_adapter(
    repository: Arc<StubEnvironmentRepository>,
) -> ProjectsDeveloperWorkflowEnvironmentAdapter {
    let environments: Arc<dyn IEnvironmentRepository> = repository;
    ProjectsDeveloperWorkflowEnvironmentAdapter::new(environments)
}

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
}

impl Fixture {
    fn new() -> Self {
        Self {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
        }
    }

    fn scope(&self) -> DeveloperWorkflowEnvironmentScope {
        DeveloperWorkflowEnvironmentScope {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
        }
    }

    fn environment(&self) -> Environment {
        Environment::create(
            self.organization_id,
            self.project_id,
            self.environment_id,
            EnvironmentName::parse("production").expect("environment name"),
            Utc::now(),
        )
    }
}

struct StubEnvironmentRepository {
    environment: Option<Environment>,
    find_calls: AtomicUsize,
}

impl StubEnvironmentRepository {
    fn new(environment: Option<Environment>) -> Self {
        Self {
            environment,
            find_calls: AtomicUsize::new(0),
        }
    }

    fn find_calls(&self) -> usize {
        self.find_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IEnvironmentRepository for StubEnvironmentRepository {
    async fn create(
        &self,
        _environment: Environment,
        _event: DomainEventEnvelope,
        _idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        unreachable!("environment access adapter never creates environments")
    }

    async fn find(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        self.find_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.environment.clone())
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        unreachable!("environment access adapter never lists environments")
    }
}
