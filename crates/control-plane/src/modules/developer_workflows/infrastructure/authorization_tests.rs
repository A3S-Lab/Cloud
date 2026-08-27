use super::IdentityProjectsDeveloperWorkflowAuthorizationAdapter;
use crate::modules::developer_workflows::application::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    IDeveloperWorkflowAuthorizationPort,
};
use crate::modules::identity::domain::entities::{Membership, ResourceGrant};
use crate::modules::identity::domain::repositories::{
    ChangeMembershipRoleWrite, CreateMembershipWrite, CreateResourceGrantWrite,
    IMembershipRepository, IResourceGrantRepository, MembershipRecord, RevokeMembershipWrite,
    RevokeResourceGrantWrite,
};
use crate::modules::identity::domain::value_objects::{MembershipRole, ResourceGrantScope};
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, MembershipId, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, ResourceGrantId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[tokio::test]
async fn organization_wide_membership_requires_the_exact_existing_environment() {
    let fixture = Fixture::new();
    let owners = Arc::new(StubOwnerRepositories::new(
        Some(fixture.membership(MembershipRole::Member)),
        Vec::new(),
        Some(fixture.environment()),
    ));
    let adapter = authorization(Arc::clone(&owners));

    for action in [
        DeveloperWorkflowAction::DetectBuildPlan,
        DeveloperWorkflowAction::ReadBuildPlan,
        DeveloperWorkflowAction::AcceptBuildPlan,
        DeveloperWorkflowAction::ReadWorkloadProfile,
        DeveloperWorkflowAction::AcceptWorkloadProfile,
        DeveloperWorkflowAction::AcceptPullRequestPreviewPolicy,
    ] {
        assert!(adapter
            .is_environment_action_allowed(fixture.access(action))
            .await
            .expect("owner authorization"));
    }
    assert_eq!(owners.membership_calls(), 6);
    assert_eq!(owners.grant_calls(), 0);
    assert_eq!(owners.environment_calls(), 6);

    let missing_owners = Arc::new(StubOwnerRepositories::new(
        Some(fixture.membership(MembershipRole::Member)),
        Vec::new(),
        None,
    ));
    let missing = authorization(missing_owners);
    assert!(!missing
        .is_environment_action_allowed(fixture.access(DeveloperWorkflowAction::AcceptBuildPlan))
        .await
        .expect("concealed missing environment"));
}

#[tokio::test]
async fn restricted_membership_requires_an_exact_grant_before_projects_lookup() {
    let fixture = Fixture::new();
    let denied_owners = Arc::new(StubOwnerRepositories::new(
        Some(fixture.membership(MembershipRole::Restricted)),
        vec![fixture.project_grant(ProjectId::new())],
        Some(fixture.environment()),
    ));
    let denied = authorization(Arc::clone(&denied_owners));
    assert!(!denied
        .is_environment_action_allowed(fixture.access(DeveloperWorkflowAction::AcceptBuildPlan))
        .await
        .expect("restricted denial"));
    assert_eq!(denied_owners.grant_calls(), 1);
    assert_eq!(
        denied_owners.environment_calls(),
        0,
        "Projects must not be queried before Identity grants admit the exact scope"
    );

    let allowed_owners = Arc::new(StubOwnerRepositories::new(
        Some(fixture.membership(MembershipRole::Restricted)),
        vec![fixture.project_grant(fixture.project_id)],
        Some(fixture.environment()),
    ));
    let allowed = authorization(Arc::clone(&allowed_owners));
    assert!(allowed
        .is_environment_action_allowed(fixture.access(DeveloperWorkflowAction::AcceptBuildPlan))
        .await
        .expect("project descendant authorization"));
    assert_eq!(allowed_owners.grant_calls(), 1);
    assert_eq!(allowed_owners.environment_calls(), 1);
}

#[tokio::test]
async fn missing_or_inconsistent_owner_evidence_fails_closed_before_resource_access() {
    let fixture = Fixture::new();
    let missing_owners = Arc::new(StubOwnerRepositories::new(
        None,
        Vec::new(),
        Some(fixture.environment()),
    ));
    let missing = authorization(Arc::clone(&missing_owners));
    assert!(!missing
        .is_environment_action_allowed(fixture.access(DeveloperWorkflowAction::AcceptBuildPlan))
        .await
        .expect("missing membership is concealed"));
    assert_eq!(missing_owners.grant_calls(), 0);
    assert_eq!(missing_owners.environment_calls(), 0);

    let inconsistent_owners = Arc::new(StubOwnerRepositories::new(
        Some(Membership::create(
            fixture.membership_id,
            fixture.organization_id,
            PrincipalId::new(),
            MembershipRole::Member,
            Utc::now(),
        )),
        Vec::new(),
        Some(fixture.environment()),
    ));
    let inconsistent = authorization(Arc::clone(&inconsistent_owners));
    assert!(matches!(
        inconsistent
            .is_environment_action_allowed(fixture.access(DeveloperWorkflowAction::AcceptBuildPlan))
            .await,
        Err(RepositoryError::Storage(_))
    ));
    assert_eq!(inconsistent_owners.grant_calls(), 0);
    assert_eq!(inconsistent_owners.environment_calls(), 0);

    let inconsistent_grant_owners = Arc::new(StubOwnerRepositories::new(
        Some(fixture.membership(MembershipRole::Restricted)),
        vec![ResourceGrant::create(
            ResourceGrantId::new(),
            fixture.organization_id,
            MembershipId::new(),
            ResourceGrantScope::Project {
                project_id: fixture.project_id,
            },
            Utc::now(),
        )],
        Some(fixture.environment()),
    ));
    let inconsistent_grant = authorization(Arc::clone(&inconsistent_grant_owners));
    assert!(matches!(
        inconsistent_grant
            .is_environment_action_allowed(fixture.access(DeveloperWorkflowAction::AcceptBuildPlan))
            .await,
        Err(RepositoryError::Storage(_))
    ));
    assert_eq!(inconsistent_grant_owners.grant_calls(), 1);
    assert_eq!(inconsistent_grant_owners.environment_calls(), 0);

    let inconsistent_environment_owners = Arc::new(StubOwnerRepositories::new(
        Some(fixture.membership(MembershipRole::Member)),
        Vec::new(),
        Some(Environment::create(
            fixture.organization_id,
            ProjectId::new(),
            fixture.environment_id,
            EnvironmentName::parse("production").expect("environment name"),
            Utc::now(),
        )),
    ));
    let inconsistent_environment = authorization(Arc::clone(&inconsistent_environment_owners));
    assert!(matches!(
        inconsistent_environment
            .is_environment_action_allowed(fixture.access(DeveloperWorkflowAction::AcceptBuildPlan))
            .await,
        Err(RepositoryError::Storage(_))
    ));
    assert_eq!(inconsistent_environment_owners.grant_calls(), 0);
    assert_eq!(inconsistent_environment_owners.environment_calls(), 1);
}

#[test]
fn authorization_adapter_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IdentityProjectsDeveloperWorkflowAuthorizationAdapter>();
}

fn authorization(
    owners: Arc<StubOwnerRepositories>,
) -> IdentityProjectsDeveloperWorkflowAuthorizationAdapter {
    let memberships: Arc<dyn IMembershipRepository> = owners.clone();
    let resource_grants: Arc<dyn IResourceGrantRepository> = owners.clone();
    let environments: Arc<dyn IEnvironmentRepository> = owners;
    IdentityProjectsDeveloperWorkflowAuthorizationAdapter::new(
        memberships,
        resource_grants,
        environments,
    )
}

struct Fixture {
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    membership_id: MembershipId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
}

impl Fixture {
    fn new() -> Self {
        Self {
            organization_id: OrganizationId::new(),
            principal_id: PrincipalId::new(),
            membership_id: MembershipId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
        }
    }

    fn access(&self, action: DeveloperWorkflowAction) -> DeveloperWorkflowEnvironmentAccess {
        DeveloperWorkflowEnvironmentAccess {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            principal_id: self.principal_id,
            action,
        }
    }

    fn membership(&self, role: MembershipRole) -> Membership {
        Membership::create(
            self.membership_id,
            self.organization_id,
            self.principal_id,
            role,
            Utc::now(),
        )
    }

    fn project_grant(&self, project_id: ProjectId) -> ResourceGrant {
        ResourceGrant::create(
            ResourceGrantId::new(),
            self.organization_id,
            self.membership_id,
            ResourceGrantScope::Project { project_id },
            Utc::now(),
        )
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

struct StubOwnerRepositories {
    membership: Option<Membership>,
    grants: Vec<ResourceGrant>,
    environment: Option<Environment>,
    membership_calls: AtomicUsize,
    grant_calls: AtomicUsize,
    environment_calls: AtomicUsize,
}

impl StubOwnerRepositories {
    fn new(
        membership: Option<Membership>,
        grants: Vec<ResourceGrant>,
        environment: Option<Environment>,
    ) -> Self {
        Self {
            membership,
            grants,
            environment,
            membership_calls: AtomicUsize::new(0),
            grant_calls: AtomicUsize::new(0),
            environment_calls: AtomicUsize::new(0),
        }
    }

    fn membership_calls(&self) -> usize {
        self.membership_calls.load(Ordering::SeqCst)
    }

    fn grant_calls(&self) -> usize {
        self.grant_calls.load(Ordering::SeqCst)
    }

    fn environment_calls(&self) -> usize {
        self.environment_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IMembershipRepository for StubOwnerRepositories {
    async fn create_membership(
        &self,
        _write: CreateMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        unreachable!("authorization adapter never creates memberships")
    }

    async fn find_membership(
        &self,
        _organization_id: OrganizationId,
        _membership_id: MembershipId,
    ) -> Result<Option<MembershipRecord>, RepositoryError> {
        unreachable!("authorization adapter never finds membership records by ID")
    }

    async fn list_memberships(
        &self,
        _organization_id: OrganizationId,
    ) -> Result<Vec<MembershipRecord>, RepositoryError> {
        unreachable!("authorization adapter never lists memberships")
    }

    async fn find_active_membership_by_principal(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<Option<Membership>, RepositoryError> {
        self.membership_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.membership.clone())
    }

    async fn change_membership_role(
        &self,
        _write: ChangeMembershipRoleWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        unreachable!("authorization adapter never changes memberships")
    }

    async fn revoke_membership(
        &self,
        _write: RevokeMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        unreachable!("authorization adapter never revokes memberships")
    }
}

#[async_trait]
impl IResourceGrantRepository for StubOwnerRepositories {
    async fn create_resource_grant(
        &self,
        _write: CreateResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError> {
        unreachable!("authorization adapter never creates Resource Grants")
    }

    async fn find_resource_grant(
        &self,
        _organization_id: OrganizationId,
        _resource_grant_id: ResourceGrantId,
    ) -> Result<Option<ResourceGrant>, RepositoryError> {
        unreachable!("authorization adapter never finds one Resource Grant")
    }

    async fn list_resource_grants(
        &self,
        _organization_id: OrganizationId,
        _membership_id: Option<MembershipId>,
    ) -> Result<Vec<ResourceGrant>, RepositoryError> {
        unreachable!("authorization adapter never lists inactive Resource Grants")
    }

    async fn list_active_resource_grants_for_membership(
        &self,
        _organization_id: OrganizationId,
        _membership_id: MembershipId,
    ) -> Result<Vec<ResourceGrant>, RepositoryError> {
        self.grant_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.grants.clone())
    }

    async fn revoke_resource_grant(
        &self,
        _write: RevokeResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError> {
        unreachable!("authorization adapter never revokes Resource Grants")
    }
}

#[async_trait]
impl IEnvironmentRepository for StubOwnerRepositories {
    async fn create(
        &self,
        _environment: Environment,
        _event: DomainEventEnvelope,
        _idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        unreachable!("authorization adapter never creates environments")
    }

    async fn find(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        self.environment_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.environment.clone())
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        unreachable!("authorization adapter never lists environments")
    }
}
