use super::{
    OpenAnonymousApplicationSession, OpenAnonymousApplicationSessionHandler,
    OpenApplicationSession, OpenApplicationSessionHandler,
};
use crate::modules::applications::ApplicationAccess;
use crate::modules::applications::domain::{
    Application, ApplicationAudience, ApplicationDeliveryCredential, ApplicationDeliveryPolicy,
    ApplicationExperience, ApplicationInteractionMode, ApplicationRecord, ApplicationRelease,
    ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationReleasePublished,
    ApplicationResponseMode, ApplicationWorkflowBinding, CreateApplicationWrite,
    IApplicationDeliveryCredentialRepository, IApplicationRepository,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationDeliveryCredentialRepository, InMemoryApplicationRepository,
    InMemoryApplicationSessionRepository,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationReleaseId, ApplicationSessionId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, ResourceName, SecretId,
    SecretVersionReference, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

struct Fixture {
    applications: Arc<InMemoryApplicationRepository>,
    sessions: Arc<InMemoryApplicationSessionRepository>,
    credentials: Arc<InMemoryApplicationDeliveryCredentialRepository>,
    release: ApplicationRelease,
    credential: ApplicationDeliveryCredential,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
}

impl Fixture {
    fn open(&self, session_id: ApplicationSessionId) -> OpenAnonymousApplicationSession {
        OpenAnonymousApplicationSession {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            application_release_id: self.release.id,
            session_id,
            credential_lookup_key: self.credential.lookup_key.clone(),
            initial_variables: json!({"locale": "en-US"}),
            opened_at: self.created_at + Duration::seconds(1),
        }
    }

    fn handler(&self) -> OpenAnonymousApplicationSessionHandler {
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
        .with_ymd_and_hms(2026, 9, 14, 12, 0, 0)
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
        ResourceName::parse("Anonymous delivery application").expect("Application name"),
        "Anonymous delivery commands".into(),
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
                "application-anonymous-delivery-test",
                "application",
                release.contract.canonical_acl().as_bytes(),
            )
            .expect("Application idempotency"),
        })
        .await
        .expect("persist Application");
    let credential = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::new(),
        &release,
        "public-embed-key",
        SecretVersionReference::new(SecretId::new(), 1).expect("secret"),
        actor,
        created_at,
    )
    .expect("credential");
    let credentials = Arc::new(InMemoryApplicationDeliveryCredentialRepository::default());
    credentials
        .create_delivery_credential(credential.clone())
        .await
        .expect("persist credential");
    Fixture {
        applications,
        sessions: Arc::new(InMemoryApplicationSessionRepository::new()),
        credentials,
        release,
        credential,
        actor,
        created_at,
    }
}

#[tokio::test]
async fn anonymous_session_opens_once_and_replays_for_same_credential() {
    let fixture = anonymous_fixture().await;
    let session_id = ApplicationSessionId::new();
    let handler = fixture.handler();
    let first = handler
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    assert!(!first.replayed);
    assert_eq!(first.end_user.audience, ApplicationAudience::Anonymous);
    assert!(first.end_user.linked_principal_id.is_none());
    assert_eq!(
        first.end_user.id,
        fixture
            .credential
            .anonymous_end_user_id()
            .expect("end user id")
    );

    let replay = handler
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("replay anonymous session");
    assert!(replay.replayed);
    assert_eq!(replay.session, first.session);
    assert_eq!(replay.end_user, first.end_user);
}

#[tokio::test]
async fn anonymous_open_fails_closed_for_inactive_or_missing_credentials() {
    let fixture = anonymous_fixture().await;
    let handler = fixture.handler();

    let mut missing = fixture.open(ApplicationSessionId::new());
    missing.credential_lookup_key = "missing-key".into();
    assert!(matches!(
        handler
            .execute(missing, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::NotFound(_))
    ));

    let mut disabled = fixture.credential.clone();
    disabled
        .disable(
            disabled.generation,
            fixture.created_at + Duration::seconds(2),
        )
        .expect("disable");
    fixture
        .credentials
        .update_delivery_credential(disabled, fixture.credential.generation)
        .await
        .expect("persist disable");
    assert!(matches!(
        handler
            .execute(fixture.open(ApplicationSessionId::new()), context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));
}

#[tokio::test]
async fn project_member_open_still_rejects_anonymous_release() {
    let fixture = anonymous_fixture().await;
    let handler =
        OpenApplicationSessionHandler::new(fixture.applications.clone(), fixture.sessions.clone());
    let command = OpenApplicationSession {
        organization_id: fixture.release.organization_id,
        project_id: fixture.release.project_id,
        application_id: fixture.release.application_id,
        application_release_id: fixture.release.id,
        session_id: ApplicationSessionId::new(),
        initial_variables: json!({"locale": "en-US"}),
        actor_principal_id: fixture.actor,
        access: ApplicationAccess::organization_wide(),
        opened_at: fixture.created_at + Duration::seconds(1),
    };
    assert!(matches!(
        handler
            .execute(command, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));
}

#[tokio::test]
async fn foreign_credential_cannot_replay_another_session() {
    let fixture = anonymous_fixture().await;
    let session_id = ApplicationSessionId::new();
    fixture
        .handler()
        .execute(fixture.open(session_id), context())
        .await
        .expect("command framework")
        .expect("open");

    let foreign = ApplicationDeliveryCredential::issue(
        ApplicationDeliveryCredentialId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000099").expect("UUID"),
        ),
        &fixture.release,
        "other-embed-key",
        SecretVersionReference::new(SecretId::new(), 1).expect("secret"),
        fixture.actor,
        fixture.created_at,
    )
    .expect("foreign credential");
    fixture
        .credentials
        .create_delivery_credential(foreign.clone())
        .await
        .expect("persist foreign");
    let mut hijack = fixture.open(session_id);
    hijack.credential_lookup_key = foreign.lookup_key;
    assert!(matches!(
        fixture
            .handler()
            .execute(hijack, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::NotFound(_))
    ));
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn anonymous_open_handler_is_send_sync() {
    assert_send_sync::<OpenAnonymousApplicationSessionHandler>();
}
