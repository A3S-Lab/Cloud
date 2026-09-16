use super::{
    ApplicationAccess, ApplicationAccessScope, DisableApplicationDeliveryCredential,
    DisableApplicationDeliveryCredentialHandler, EnableApplicationDeliveryCredential,
    EnableApplicationDeliveryCredentialHandler, GetApplicationDeliveryCredential,
    GetApplicationDeliveryCredentialHandler, ListApplicationDeliveryCredentials,
    ListApplicationDeliveryCredentialsHandler, OpenAnonymousApplicationSession,
    OpenAnonymousApplicationSessionHandler, RegisterApplicationDeliveryCredential,
    RegisterApplicationDeliveryCredentialHandler, RevokeApplicationDeliveryCredential,
    RevokeApplicationDeliveryCredentialHandler,
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
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationDeliveryCredentialId, ApplicationId, ApplicationReleaseId, ApplicationSessionId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, ResourceName, SecretId,
    SecretVersionReference, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

struct Fixture {
    applications: Arc<InMemoryApplicationRepository>,
    sessions: Arc<InMemoryApplicationSessionRepository>,
    credentials: Arc<InMemoryApplicationDeliveryCredentialRepository>,
    release: ApplicationRelease,
    actor: PrincipalId,
    created_at: chrono::DateTime<Utc>,
    access: ApplicationAccess,
}

impl Fixture {
    fn register(
        &self,
        credential_id: ApplicationDeliveryCredentialId,
        lookup_key: &str,
    ) -> RegisterApplicationDeliveryCredential {
        RegisterApplicationDeliveryCredential {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            application_release_id: self.release.id,
            credential_id,
            lookup_key: lookup_key.into(),
            secret: SecretVersionReference::new(SecretId::new(), 1).expect("secret"),
            actor_principal_id: self.actor,
            access: self.access.clone(),
            created_at: self.created_at,
        }
    }

    fn register_handler(&self) -> RegisterApplicationDeliveryCredentialHandler {
        RegisterApplicationDeliveryCredentialHandler::new(
            self.applications.clone(),
            self.credentials.clone(),
        )
    }

    fn disable_handler(&self) -> DisableApplicationDeliveryCredentialHandler {
        DisableApplicationDeliveryCredentialHandler::new(self.credentials.clone())
    }

    fn enable_handler(&self) -> EnableApplicationDeliveryCredentialHandler {
        EnableApplicationDeliveryCredentialHandler::new(self.credentials.clone())
    }

    fn revoke_handler(&self) -> RevokeApplicationDeliveryCredentialHandler {
        RevokeApplicationDeliveryCredentialHandler::new(self.credentials.clone())
    }

    fn get_handler(&self) -> GetApplicationDeliveryCredentialHandler {
        GetApplicationDeliveryCredentialHandler::new(self.credentials.clone())
    }

    fn list_handler(&self) -> ListApplicationDeliveryCredentialsHandler {
        ListApplicationDeliveryCredentialsHandler::new(
            self.applications.clone(),
            self.credentials.clone(),
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
        .with_ymd_and_hms(2026, 9, 14, 13, 0, 0)
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
        ResourceName::parse("Delivery credential lifecycle").expect("Application name"),
        "APP0.2-C20 commands".into(),
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
                "application-delivery-credential-lifecycle-test",
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
        release,
        actor,
        created_at,
        access: ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]),
    }
}

#[tokio::test]
async fn register_creates_once_and_replays_exact_identity() {
    let fixture = anonymous_fixture().await;
    let credential_id = ApplicationDeliveryCredentialId::new();
    let handler = fixture.register_handler();
    let command = fixture.register(credential_id, "public-embed-key");

    let first = handler
        .execute(command.clone(), context())
        .await
        .expect("command framework")
        .expect("register credential");
    assert!(!first.replayed);
    assert_eq!(
        first.credential.status,
        ApplicationDeliveryCredentialStatus::Active
    );
    assert_eq!(first.credential.generation, 1);
    assert_eq!(first.credential.lookup_key, "public-embed-key");
    assert_eq!(first.credential.created_by, fixture.actor);

    let replay = handler
        .execute(command, context())
        .await
        .expect("command framework")
        .expect("replay register");
    assert!(replay.replayed);
    assert_eq!(replay.credential, first.credential);
}

#[tokio::test]
async fn register_fails_closed_for_lookup_collision_and_unauthorized_project() {
    let fixture = anonymous_fixture().await;
    let handler = fixture.register_handler();
    handler
        .execute(
            fixture.register(ApplicationDeliveryCredentialId::new(), "shared-key"),
            context(),
        )
        .await
        .expect("command framework")
        .expect("first register");

    let collision = handler
        .execute(
            fixture.register(ApplicationDeliveryCredentialId::new(), "shared-key"),
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(collision, Err(ApplicationError::Conflict(_))));

    let mut unauthorized = fixture.register(ApplicationDeliveryCredentialId::new(), "other-key");
    unauthorized.access = ApplicationAccess::restricted([ApplicationAccessScope::Project {
        project_id: ProjectId::new(),
    }]);
    assert!(matches!(
        handler
            .execute(unauthorized, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::NotFound(_))
    ));
}

#[tokio::test]
async fn generation_fences_disable_enable_and_revoke() {
    let fixture = anonymous_fixture().await;
    let credential_id = ApplicationDeliveryCredentialId::new();
    let registered = fixture
        .register_handler()
        .execute(fixture.register(credential_id, "lifecycle-key"), context())
        .await
        .expect("command framework")
        .expect("register");

    let disabled = fixture
        .disable_handler()
        .execute(
            DisableApplicationDeliveryCredential {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                credential_id,
                expected_generation: registered.credential.generation,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                updated_at: fixture.created_at + Duration::seconds(1),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("disable");
    assert_eq!(
        disabled.credential.status,
        ApplicationDeliveryCredentialStatus::Disabled
    );
    assert_eq!(disabled.credential.generation, 2);

    let stale = fixture
        .enable_handler()
        .execute(
            EnableApplicationDeliveryCredential {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                credential_id,
                expected_generation: registered.credential.generation,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                updated_at: fixture.created_at + Duration::seconds(2),
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(stale, Err(ApplicationError::Conflict(_))));

    let enabled = fixture
        .enable_handler()
        .execute(
            EnableApplicationDeliveryCredential {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                credential_id,
                expected_generation: disabled.credential.generation,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                updated_at: fixture.created_at + Duration::seconds(3),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("enable");
    assert_eq!(
        enabled.credential.status,
        ApplicationDeliveryCredentialStatus::Active
    );
    assert_eq!(enabled.credential.generation, 3);

    let revoked = fixture
        .revoke_handler()
        .execute(
            RevokeApplicationDeliveryCredential {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                credential_id,
                expected_generation: enabled.credential.generation,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                revoked_at: fixture.created_at + Duration::seconds(4),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("revoke");
    assert_eq!(
        revoked.credential.status,
        ApplicationDeliveryCredentialStatus::Revoked
    );
    assert!(revoked.credential.revoked_at.is_some());

    let immutable = fixture
        .enable_handler()
        .execute(
            EnableApplicationDeliveryCredential {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                credential_id,
                expected_generation: revoked.credential.generation,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                updated_at: fixture.created_at + Duration::seconds(5),
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(immutable, Err(ApplicationError::Conflict(_))));
}

#[tokio::test]
async fn registered_active_credential_admits_anonymous_session_and_inactive_fails_closed() {
    let fixture = anonymous_fixture().await;
    let credential_id = ApplicationDeliveryCredentialId::new();
    let registered = fixture
        .register_handler()
        .execute(
            fixture.register(credential_id, "session-admit-key"),
            context(),
        )
        .await
        .expect("command framework")
        .expect("register");

    let open = OpenAnonymousApplicationSession {
        organization_id: fixture.release.organization_id,
        project_id: fixture.release.project_id,
        application_id: fixture.release.application_id,
        application_release_id: fixture.release.id,
        session_id: ApplicationSessionId::new(),
        credential_lookup_key: registered.credential.lookup_key.clone(),
        initial_variables: json!({"locale": "en-US"}),
        opened_at: fixture.created_at + Duration::seconds(1),
    };
    let opened = fixture
        .open_handler()
        .execute(open.clone(), context())
        .await
        .expect("command framework")
        .expect("open anonymous session");
    assert!(!opened.replayed);
    assert_eq!(opened.end_user.audience, ApplicationAudience::Anonymous);

    fixture
        .disable_handler()
        .execute(
            DisableApplicationDeliveryCredential {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                credential_id,
                expected_generation: registered.credential.generation,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
                updated_at: fixture.created_at + Duration::seconds(2),
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("disable");

    let mut inactive_open = open;
    inactive_open.session_id = ApplicationSessionId::new();
    inactive_open.opened_at = fixture.created_at + Duration::seconds(3);
    assert!(matches!(
        fixture
            .open_handler()
            .execute(inactive_open, context())
            .await
            .expect("command framework"),
        Err(ApplicationError::Conflict(_))
    ));
}

#[tokio::test]
async fn delivery_credential_handlers_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RegisterApplicationDeliveryCredentialHandler>();
    assert_send_sync::<DisableApplicationDeliveryCredentialHandler>();
    assert_send_sync::<EnableApplicationDeliveryCredentialHandler>();
    assert_send_sync::<RevokeApplicationDeliveryCredentialHandler>();
    assert_send_sync::<GetApplicationDeliveryCredentialHandler>();
    assert_send_sync::<ListApplicationDeliveryCredentialsHandler>();
}

#[tokio::test]
async fn gets_and_lists_registered_delivery_credentials() {
    let fixture = anonymous_fixture().await;
    let first_id = ApplicationDeliveryCredentialId::new();
    let second_id = ApplicationDeliveryCredentialId::new();
    let first = fixture
        .register_handler()
        .execute(fixture.register(first_id, "list-key-a"), context())
        .await
        .expect("command framework")
        .expect("register first");
    let second = fixture
        .register_handler()
        .execute(
            RegisterApplicationDeliveryCredential {
                created_at: fixture.created_at + Duration::seconds(1),
                ..fixture.register(second_id, "list-key-b")
            },
            context(),
        )
        .await
        .expect("command framework")
        .expect("register second");

    let got = fixture
        .get_handler()
        .execute(
            GetApplicationDeliveryCredential {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                credential_id: first_id,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get");
    assert_eq!(got.id, first.credential.id);
    assert_eq!(got.lookup_key, "list-key-a");

    let listed = fixture
        .list_handler()
        .execute(
            ListApplicationDeliveryCredentials {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("list");
    assert_eq!(
        listed
            .iter()
            .map(|credential| credential.id)
            .collect::<Vec<_>>(),
        vec![first.credential.id, second.credential.id]
    );
}

#[tokio::test]
async fn missing_and_unauthorized_delivery_credential_reads_fail_closed() {
    let fixture = anonymous_fixture().await;
    let credential_id = ApplicationDeliveryCredentialId::new();
    fixture
        .register_handler()
        .execute(fixture.register(credential_id, "read-key"), context())
        .await
        .expect("command framework")
        .expect("register");

    assert!(matches!(
        fixture
            .get_handler()
            .execute(
                GetApplicationDeliveryCredential {
                    organization_id: fixture.release.organization_id,
                    project_id: fixture.release.project_id,
                    application_id: fixture.release.application_id,
                    credential_id: ApplicationDeliveryCredentialId::new(),
                    actor_principal_id: fixture.actor,
                    access: fixture.access.clone(),
                },
                context(),
            )
            .await
            .expect("query framework"),
        Err(ApplicationError::NotFound(_))
    ));

    let unauthorized_access = ApplicationAccess::restricted([ApplicationAccessScope::Project {
        project_id: ProjectId::new(),
    }]);
    assert!(matches!(
        fixture
            .get_handler()
            .execute(
                GetApplicationDeliveryCredential {
                    organization_id: fixture.release.organization_id,
                    project_id: fixture.release.project_id,
                    application_id: fixture.release.application_id,
                    credential_id,
                    actor_principal_id: fixture.actor,
                    access: unauthorized_access.clone(),
                },
                context(),
            )
            .await
            .expect("query framework"),
        Err(ApplicationError::NotFound(_))
    ));

    assert!(matches!(
        fixture
            .list_handler()
            .execute(
                ListApplicationDeliveryCredentials {
                    organization_id: fixture.release.organization_id,
                    project_id: fixture.release.project_id,
                    application_id: fixture.release.application_id,
                    actor_principal_id: fixture.actor,
                    access: unauthorized_access,
                },
                context(),
            )
            .await
            .expect("query framework"),
        Err(ApplicationError::NotFound(_))
    ));

    assert!(matches!(
        fixture
            .list_handler()
            .execute(
                ListApplicationDeliveryCredentials {
                    organization_id: fixture.release.organization_id,
                    project_id: fixture.release.project_id,
                    application_id: ApplicationId::new(),
                    actor_principal_id: fixture.actor,
                    access: fixture.access.clone(),
                },
                context(),
            )
            .await
            .expect("query framework"),
        Err(ApplicationError::NotFound(_))
    ));
}
