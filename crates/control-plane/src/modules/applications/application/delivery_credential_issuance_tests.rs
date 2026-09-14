use super::{
    ApplicationAccess, ApplicationAccessScope, ApplicationDeliveryCredentialMaterial,
    IApplicationDeliveryCredentialMaterialPort, IssueApplicationDeliveryCredential,
    IssueApplicationDeliveryCredentialHandler, OpenAnonymousApplicationSession,
    OpenAnonymousApplicationSessionHandler,
};
use crate::modules::applications::domain::{
    Application, ApplicationAudience, ApplicationDeliveryCredentialStatus,
    ApplicationDeliveryPolicy, ApplicationExperience, ApplicationInteractionMode,
    ApplicationRecord, ApplicationRelease, ApplicationReleaseContract,
    ApplicationReleaseContractSpec, ApplicationReleasePublished, ApplicationResponseMode,
    ApplicationWorkflowBinding, CreateApplicationWrite, IApplicationRepository,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationDeliveryCredentialRepository, InMemoryApplicationRepository,
    InMemoryApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationReleaseId, ApplicationSessionId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, ResourceName, SecretId,
    SecretVersionReference, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

struct Fixture {
    applications: Arc<InMemoryApplicationRepository>,
    sessions: Arc<InMemoryApplicationSessionRepository>,
    credentials: Arc<InMemoryApplicationDeliveryCredentialRepository>,
    material: Arc<RecordingMaterialPort>,
    release: ApplicationRelease,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
    access: ApplicationAccess,
}

#[derive(Default)]
struct RecordingMaterialPort {
    mint_count: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl IApplicationDeliveryCredentialMaterialPort for RecordingMaterialPort {
    async fn mint(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        credential_id: ApplicationDeliveryCredentialId,
        _actor_principal_id: PrincipalId,
    ) -> ApplicationResult<ApplicationDeliveryCredentialMaterial> {
        self.mint_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let namespace = Uuid::NAMESPACE_OID;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"cloud.application.delivery-credential.material.v1");
        bytes.extend_from_slice(organization_id.as_uuid().as_bytes());
        bytes.extend_from_slice(project_id.as_uuid().as_bytes());
        bytes.extend_from_slice(application_id.as_uuid().as_bytes());
        bytes.extend_from_slice(credential_id.as_uuid().as_bytes());
        let secret_id = SecretId::from_uuid(Uuid::new_v5(&namespace, &bytes));
        let lookup_key = format!("lk{:x}", credential_id.as_uuid().as_u128());
        let mut plaintext = Zeroizing::new(String::from("a3s_"));
        plaintext.push_str(&"ab".repeat(32));
        Ok(ApplicationDeliveryCredentialMaterial {
            lookup_key,
            secret: SecretVersionReference::new(secret_id, 1).expect("secret"),
            plaintext_secret: plaintext,
        })
    }
}

impl Fixture {
    fn issue(
        &self,
        credential_id: ApplicationDeliveryCredentialId,
    ) -> IssueApplicationDeliveryCredential {
        IssueApplicationDeliveryCredential {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            application_release_id: self.release.id,
            credential_id,
            actor_principal_id: self.actor,
            access: self.access.clone(),
            created_at: self.created_at,
        }
    }

    fn issue_handler(&self) -> IssueApplicationDeliveryCredentialHandler {
        IssueApplicationDeliveryCredentialHandler::new(
            self.applications.clone(),
            self.credentials.clone(),
            self.material.clone(),
        )
    }

    fn open_handler(&self) -> OpenAnonymousApplicationSessionHandler {
        OpenAnonymousApplicationSessionHandler::new(
            self.applications.clone(),
            self.sessions.clone(),
            self.credentials.clone(),
        )
    }
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

async fn anonymous_fixture() -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let application_id = ApplicationId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 9, 14, 14, 0, 0)
        .single()
        .expect("timestamp");
    let workflow = ApplicationWorkflowBinding {
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_contract_digest: digest('a'),
        workflow_payload_set_digest: digest('b'),
        workflow_semantic_contract_set_digest: digest('c'),
        input_schema_digest: digest('d'),
        output_schema_digest: digest('e'),
    };
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::Anonymous,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![ApplicationResponseMode::Streaming],
        },
        workflow,
        presentation_digest: digest('f'),
    })
    .expect("Application release contract");
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        contract,
        actor,
        created_at,
    )
    .expect("Application release");
    let application = Application::create(
        application_id,
        ResourceName::parse("Delivery credential issuance").expect("Application name"),
        "APP0.2-C30 commands".into(),
        &release,
    )
    .expect("Application");
    let applications = Arc::new(InMemoryApplicationRepository::new());
    let record = ApplicationRecord::new(application.clone(), release.clone()).expect("record");
    let request_id = Uuid::now_v7();
    applications
        .create(CreateApplicationWrite {
            event: ApplicationReleasePublished::published(&application, &release, request_id)
                .expect("Application event"),
            record,
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "application-delivery-credential-issuance-test",
                "application",
                release.contract.canonical_acl().as_bytes(),
            )
            .expect("Application idempotency"),
        })
        .await
        .expect("persist Application");
    Fixture {
        applications,
        sessions: Arc::new(InMemoryApplicationSessionRepository::new()),
        credentials: Arc::new(InMemoryApplicationDeliveryCredentialRepository::default()),
        material: Arc::new(RecordingMaterialPort::default()),
        release,
        actor,
        created_at,
        access: ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]),
    }
}

#[tokio::test]
async fn issues_material_registers_binding_and_admits_anonymous_session() {
    let fixture = anonymous_fixture().await;
    let credential_id = ApplicationDeliveryCredentialId::new();
    let issued = fixture
        .issue_handler()
        .execute(fixture.issue(credential_id), context())
        .await
        .expect("command framework")
        .expect("issue");
    assert!(!issued.replayed);
    assert!(issued.plaintext_secret.is_some());
    assert_eq!(
        issued.credential.status,
        ApplicationDeliveryCredentialStatus::Active
    );
    assert_eq!(
        fixture
            .material
            .mint_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    let session_id = ApplicationSessionId::new();
    let opened = fixture
        .open_handler()
        .execute(
            OpenAnonymousApplicationSession {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                application_release_id: fixture.release.id,
                session_id,
                credential_lookup_key: issued.lookup_key.clone(),
                initial_variables: json!({"locale": "en-US"}),
                opened_at: fixture.created_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("open anonymous session");
    assert!(!opened.replayed);
    assert_eq!(opened.session.id, session_id);
    assert_eq!(opened.end_user.audience, ApplicationAudience::Anonymous);
}

#[tokio::test]
async fn exact_credential_identity_replays_without_reminting_or_plaintext() {
    let fixture = anonymous_fixture().await;
    let credential_id = ApplicationDeliveryCredentialId::new();
    let first = fixture
        .issue_handler()
        .execute(fixture.issue(credential_id), context())
        .await
        .expect("command framework")
        .expect("issue");
    let second = fixture
        .issue_handler()
        .execute(fixture.issue(credential_id), context())
        .await
        .expect("command framework")
        .expect("replay");
    assert!(second.replayed);
    assert!(second.plaintext_secret.is_none());
    assert_eq!(second.credential.id, first.credential.id);
    assert_eq!(second.lookup_key, first.lookup_key);
    assert_eq!(
        fixture
            .material
            .mint_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn unauthorized_project_fails_closed_without_minting() {
    let fixture = anonymous_fixture().await;
    let mut command = fixture.issue(ApplicationDeliveryCredentialId::new());
    command.access = ApplicationAccess::restricted([ApplicationAccessScope::Project {
        project_id: ProjectId::new(),
    }]);
    let error = fixture
        .issue_handler()
        .execute(command, context())
        .await
        .expect("command framework")
        .expect_err("unauthorized");
    assert!(matches!(error, ApplicationError::NotFound(_)));
    assert_eq!(
        fixture
            .material
            .mint_count
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
