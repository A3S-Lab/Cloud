use super::{
    BuildPlanDetectionService, BuildPlanSourceLayoutError, BuildPlanSourceLayoutRequest,
    DetectBuildPlanProposals, DetectBuildPlanProposalsHandler, DeveloperWorkflowAction,
    DeveloperWorkflowEnvironmentAccess, IBuildPlanSourceLayoutPort,
    IDeveloperWorkflowAuthorizationPort,
};
use crate::modules::developer_workflows::domain::{
    BuildPlanDetectionDiagnosticCode, BuildPlanDetectorKind, SourceLayoutEntry,
    SourceLayoutIdentity, SourceLayoutSnapshot,
};
use crate::modules::developer_workflows::infrastructure::{
    AssetAclBuildPlanDetector, DockerfileBuildPlanDetector,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GitCommitSha, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    Sha256Digest, SourceRevisionId,
};
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn exact_asset_acl_is_authoritative_through_the_production_detector_set() {
    let manifest = concat!(
        "asset {\n",
        "  kind = \"agent\"\n",
        "  schema = \"a3s.cloud.asset.v1\"\n",
        "  build {\n",
        "    context = \".\"\n",
        "    file = \"Dockerfile\"\n",
        "    platforms = [\"linux/amd64\", \"linux/arm64\"]\n",
        "    target = \"release\"\n",
        "  }\n",
        "}\n",
    );
    let layout = layout(
        'a',
        vec![
            inspected("services/ignored/Dockerfile", b"FROM scratch\n"),
            inspected(".a3s/asset.acl", manifest.as_bytes()),
            inspected("Dockerfile", b"FROM scratch\n"),
        ],
    );
    let source = layout.identity().clone();
    let fixture = Fixture::new(Some(layout), true);

    let detection = fixture
        .handler()
        .execute(fixture.query(), context())
        .await
        .expect("CQRS result")
        .expect("BuildPlan detection");

    assert_eq!(detection.source, source);
    assert_eq!(detection.proposals.len(), 1);
    assert_eq!(
        detection.proposals[0].spec().detector,
        BuildPlanDetectorKind::AssetAcl
    );
    assert_eq!(
        detection.proposals[0].spec().recipe.target(),
        Some("release")
    );
    assert!(detection.diagnostics.is_empty());
    assert_eq!(fixture.layouts.requests(), vec![fixture.layout_request()]);
    assert_eq!(fixture.authorization.calls(), 1);
    assert_eq!(
        fixture.authorization.accesses(),
        vec![fixture.access(DeveloperWorkflowAction::DetectBuildPlan)]
    );
}

#[tokio::test]
async fn dockerfile_fallback_and_unsupported_layout_are_closed_and_canonical() {
    let detected_fixture = Fixture::new(
        Some(layout(
            'b',
            vec![
                inspected("worker/Dockerfile", b"FROM scratch\n"),
                inspected("Dockerfile", b"FROM scratch\n"),
            ],
        )),
        true,
    );
    let detected = detected_fixture
        .handler()
        .execute(detected_fixture.query(), context())
        .await
        .expect("CQRS result")
        .expect("Dockerfile detection");
    assert_eq!(detected.proposals.len(), 2);
    assert_eq!(detected.proposals[0].spec().project_root, ".");
    assert_eq!(detected.proposals[1].spec().project_root, "worker");
    assert!(detected
        .proposals
        .iter()
        .all(|proposal| { proposal.spec().detector == BuildPlanDetectorKind::Dockerfile }));

    let unsupported_fixture = Fixture::new(
        Some(layout('c', vec![inspected("README.md", b"example\n")])),
        true,
    );
    let unsupported = unsupported_fixture
        .handler()
        .execute(unsupported_fixture.query(), context())
        .await
        .expect("CQRS result")
        .expect("closed unsupported-layout result");
    assert!(unsupported.proposals.is_empty());
    assert_eq!(unsupported.diagnostics.len(), 1);
    assert_eq!(
        unsupported.diagnostics[0].code,
        BuildPlanDetectionDiagnosticCode::NoSupportedLayout
    );
}

#[tokio::test]
async fn invalid_authoritative_acl_fails_closed_without_heuristic_fallback() {
    let fixture = Fixture::new(
        Some(layout(
            'd',
            vec![
                inspected(".a3s/asset.acl", b"asset { kind = \"agent\" }\n"),
                inspected("Dockerfile", b"FROM scratch\n"),
            ],
        )),
        true,
    );
    let error = fixture
        .handler()
        .execute(fixture.query(), context())
        .await
        .expect("CQRS result")
        .expect_err("invalid explicit Asset ACL must fail closed");
    let ApplicationError::Invalid(message) = error else {
        panic!("invalid explicit Asset ACL must remain an input error");
    };
    assert!(message.contains("explicit Asset ACL is invalid"));
}

#[tokio::test]
async fn authorization_precedes_source_revision_and_provider_access() {
    let fixture = Fixture::new(Some(layout('e', Vec::new())), false);

    assert!(matches!(
        fixture
            .handler()
            .execute(fixture.query(), context())
            .await
            .expect("CQRS result"),
        Err(ApplicationError::NotFound(_))
    ));
    assert_eq!(fixture.authorization.calls(), 1);
    assert!(fixture.layouts.requests().is_empty());
}

#[tokio::test]
async fn missing_exact_source_revision_is_concealed_after_authorization() {
    let fixture = Fixture::new(None, true);

    assert!(matches!(
        fixture
            .handler()
            .execute(fixture.query(), context())
            .await
            .expect("CQRS result"),
        Err(ApplicationError::NotFound(_))
    ));
    assert_eq!(fixture.layouts.requests(), vec![fixture.layout_request()]);
}

fn production_detection() -> Arc<BuildPlanDetectionService> {
    Arc::new(
        BuildPlanDetectionService::new(vec![
            Arc::new(AssetAclBuildPlanDetector),
            Arc::new(DockerfileBuildPlanDetector),
        ])
        .expect("production detector set"),
    )
}

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    principal_id: PrincipalId,
    layouts: Arc<StaticLayouts>,
    authorization: Arc<StaticAuthorization>,
}

impl Fixture {
    fn new(layout: Option<SourceLayoutSnapshot>, allowed: bool) -> Self {
        Self {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            source_revision_id: SourceRevisionId::new(),
            principal_id: PrincipalId::new(),
            layouts: Arc::new(StaticLayouts {
                layout,
                requests: Mutex::new(Vec::new()),
            }),
            authorization: Arc::new(StaticAuthorization {
                allowed,
                accesses: Mutex::new(Vec::new()),
                calls: AtomicUsize::new(0),
            }),
        }
    }

    fn handler(&self) -> DetectBuildPlanProposalsHandler {
        DetectBuildPlanProposalsHandler::new(
            production_detection(),
            self.layouts.clone(),
            self.authorization.clone(),
        )
    }

    fn query(&self) -> DetectBuildPlanProposals {
        DetectBuildPlanProposals {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            source_revision_id: self.source_revision_id,
            principal_id: self.principal_id,
        }
    }

    fn layout_request(&self) -> BuildPlanSourceLayoutRequest {
        BuildPlanSourceLayoutRequest {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            source_revision_id: self.source_revision_id,
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
}

struct StaticLayouts {
    layout: Option<SourceLayoutSnapshot>,
    requests: Mutex<Vec<BuildPlanSourceLayoutRequest>>,
}

impl StaticLayouts {
    fn requests(&self) -> Vec<BuildPlanSourceLayoutRequest> {
        self.requests.lock().expect("layout requests").clone()
    }
}

#[async_trait]
impl IBuildPlanSourceLayoutPort for StaticLayouts {
    async fn acquire(
        &self,
        request: BuildPlanSourceLayoutRequest,
    ) -> Result<Option<SourceLayoutSnapshot>, BuildPlanSourceLayoutError> {
        self.requests.lock().expect("layout requests").push(request);
        Ok(self.layout.clone())
    }
}

struct StaticAuthorization {
    allowed: bool,
    accesses: Mutex<Vec<DeveloperWorkflowEnvironmentAccess>>,
    calls: AtomicUsize,
}

impl StaticAuthorization {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn accesses(&self) -> Vec<DeveloperWorkflowEnvironmentAccess> {
        self.accesses
            .lock()
            .expect("authorization accesses")
            .clone()
    }
}

#[async_trait]
impl IDeveloperWorkflowAuthorizationPort for StaticAuthorization {
    async fn is_environment_action_allowed(
        &self,
        access: DeveloperWorkflowEnvironmentAccess,
    ) -> Result<bool, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.accesses
            .lock()
            .expect("authorization accesses")
            .push(access);
        Ok(self.allowed)
    }
}

fn layout(seed: char, entries: Vec<SourceLayoutEntry>) -> SourceLayoutSnapshot {
    SourceLayoutSnapshot::new(
        SourceLayoutIdentity::new(
            digest(seed),
            GitCommitSha::parse(seed.to_string().repeat(40)).expect("commit SHA"),
            digest(seed),
        )
        .expect("source identity"),
        entries,
    )
    .expect("source layout")
}

fn inspected(path: &str, content: &[u8]) -> SourceLayoutEntry {
    SourceLayoutEntry::inspected_regular(path, content).expect("source entry")
}

fn digest(seed: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", seed.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
