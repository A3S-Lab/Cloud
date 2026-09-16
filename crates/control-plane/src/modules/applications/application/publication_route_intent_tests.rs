use super::{
    ApplicationAccess, ApplicationAccessScope, CreateApplicationPublicationRouteIntent,
    CreateApplicationPublicationRouteIntentHandler, GetApplicationPublicationRouteIntent,
    GetApplicationPublicationRouteIntentHandler,
    ListApplicationPublicationRouteIntentsByRelease,
    ListApplicationPublicationRouteIntentsByReleaseHandler,
};
use crate::modules::applications::domain::{
    Application, ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationPublicationChannel,
    ApplicationPublicationRateShapingPolicyRef, ApplicationRecord, ApplicationRelease,
    ApplicationReleaseContract, ApplicationReleaseContractSpec, ApplicationReleasePublished,
    ApplicationResponseMode, ApplicationWorkflowBinding, CreateApplicationWrite,
    IApplicationRepository,
};
use crate::modules::applications::infrastructure::{
    InMemoryApplicationPublicationRouteIntentRepository, InMemoryApplicationRepository,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationPublicationRouteIntentId, ApplicationReleaseId, IdempotencyRequest,
    OrganizationId, PrincipalId, ProjectId, ResourceName, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use chrono::{TimeZone, Utc};
use std::sync::Arc;
use uuid::Uuid;

struct Fixture {
    applications: Arc<InMemoryApplicationRepository>,
    intents: Arc<InMemoryApplicationPublicationRouteIntentRepository>,
    release: ApplicationRelease,
    actor: PrincipalId,
    access: ApplicationAccess,
}

impl Fixture {
    fn create(
        &self,
        channels: Vec<ApplicationPublicationChannel>,
        origins: Vec<String>,
    ) -> CreateApplicationPublicationRouteIntent {
        CreateApplicationPublicationRouteIntent {
            organization_id: self.release.organization_id,
            project_id: self.release.project_id,
            application_id: self.release.application_id,
            application_release_id: self.release.id,
            application_release_digest: self.release.contract.digest().clone(),
            channels,
            embed_origin_allowlist: origins,
            rate_shaping_policy: rate_policy(),
            actor_principal_id: self.actor,
            access: self.access.clone(),
        }
    }

    fn create_handler(&self) -> CreateApplicationPublicationRouteIntentHandler {
        CreateApplicationPublicationRouteIntentHandler::new(
            self.applications.clone(),
            self.intents.clone(),
        )
    }

    fn get_handler(&self) -> GetApplicationPublicationRouteIntentHandler {
        GetApplicationPublicationRouteIntentHandler::new(self.intents.clone())
    }

    fn list_handler(&self) -> ListApplicationPublicationRouteIntentsByReleaseHandler {
        ListApplicationPublicationRouteIntentsByReleaseHandler::new(
            self.applications.clone(),
            self.intents.clone(),
        )
    }
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn rate_policy() -> ApplicationPublicationRateShapingPolicyRef {
    ApplicationPublicationRateShapingPolicyRef::create("public-api-default", digest('a'))
        .expect("rate policy")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

async fn fixture() -> Fixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let application_id = ApplicationId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 9, 15, 14, 0, 0)
        .single()
        .expect("timestamp");
    let contract = ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![ApplicationResponseMode::Blocking],
        },
        workflow: ApplicationWorkflowBinding {
            workflow_definition_id: WorkflowDefinitionId::new(),
            workflow_revision_id: WorkflowRevisionId::new(),
            workflow_contract_digest: digest('a'),
            workflow_payload_set_digest: digest('b'),
            workflow_semantic_contract_set_digest: digest('c'),
            input_schema_digest: digest('d'),
            output_schema_digest: digest('e'),
        },
        presentation_digest: digest('f'),
    })
    .expect("contract");
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        contract,
        actor,
        created_at,
    )
    .expect("release");
    let application = Application::create(
        application_id,
        ResourceName::parse("Publication route intent").expect("name"),
        "APP0.3-C12 commands".into(),
        &release,
    )
    .expect("application");
    let applications = Arc::new(InMemoryApplicationRepository::new());
    let record = ApplicationRecord::new(application.clone(), release.clone()).expect("record");
    let request_id = Uuid::now_v7();
    applications
        .create(CreateApplicationWrite {
            event: ApplicationReleasePublished::published(&application, &release, request_id)
                .expect("event"),
            record,
            actor_principal_id: actor,
            request_id,
            idempotency: IdempotencyRequest::new(
                "application-publication-route-intent-cqrs-test",
                "application",
                release.contract.canonical_acl().as_bytes(),
            )
            .expect("idempotency"),
        })
        .await
        .expect("persist");
    Fixture {
        applications,
        intents: Arc::new(InMemoryApplicationPublicationRouteIntentRepository::default()),
        release,
        actor,
        access: ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]),
    }
}

#[tokio::test]
async fn creates_gets_lists_and_replays_exact_intent() {
    let fixture = fixture().await;
    let handler = fixture.create_handler();
    let command = fixture.create(
        vec![
            ApplicationPublicationChannel::ApiBlocking,
            ApplicationPublicationChannel::Embed,
        ],
        vec!["https://app.example.com".into()],
    );
    let created = handler
        .execute(command.clone(), context())
        .await
        .expect("boot")
        .expect("create");
    assert!(!created.replayed);
    assert_eq!(
        created.intent.application_release_digest,
        *fixture.release.contract.digest()
    );

    let replayed = handler
        .execute(command, context())
        .await
        .expect("boot")
        .expect("replay");
    assert!(replayed.replayed);
    assert_eq!(replayed.intent, created.intent);

    let got = fixture
        .get_handler()
        .execute(
            GetApplicationPublicationRouteIntent {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                intent_id: created.intent.id,
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("get");
    assert_eq!(got, created.intent);

    let listed = fixture
        .list_handler()
        .execute(
            ListApplicationPublicationRouteIntentsByRelease {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                application_release_id: fixture.release.id,
                application_release_digest: fixture.release.contract.digest().clone(),
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect("list");
    assert_eq!(listed, vec![created.intent]);
}

#[tokio::test]
async fn unauthorized_project_fails_closed_for_create_get_and_list() {
    let fixture = fixture().await;
    let mut command = fixture.create(
        vec![ApplicationPublicationChannel::Web],
        Vec::new(),
    );
    command.access = ApplicationAccess::restricted([ApplicationAccessScope::Project {
        project_id: ProjectId::new(),
    }]);
    let create_error = fixture
        .create_handler()
        .execute(command, context())
        .await
        .expect("boot")
        .expect_err("unauthorized create");
    assert!(matches!(create_error, ApplicationError::NotFound(_)));

    let get_error = fixture
        .get_handler()
        .execute(
            GetApplicationPublicationRouteIntent {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                intent_id: ApplicationPublicationRouteIntentId::new(),
                actor_principal_id: fixture.actor,
                access: ApplicationAccess::restricted([ApplicationAccessScope::Project {
                    project_id: ProjectId::new(),
                }]),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect_err("unauthorized get");
    assert!(matches!(get_error, ApplicationError::NotFound(_)));

    let list_error = fixture
        .list_handler()
        .execute(
            ListApplicationPublicationRouteIntentsByRelease {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                application_release_id: fixture.release.id,
                application_release_digest: fixture.release.contract.digest().clone(),
                actor_principal_id: fixture.actor,
                access: ApplicationAccess::restricted([ApplicationAccessScope::Project {
                    project_id: ProjectId::new(),
                }]),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect_err("unauthorized list");
    assert!(matches!(list_error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn digest_mismatch_and_missing_intent_fail_closed() {
    let fixture = fixture().await;
    let mut command = fixture.create(
        vec![ApplicationPublicationChannel::Internal],
        Vec::new(),
    );
    command.application_release_digest = digest('9');
    let mismatch = fixture
        .create_handler()
        .execute(command, context())
        .await
        .expect("boot")
        .expect_err("digest mismatch");
    assert!(matches!(mismatch, ApplicationError::NotFound(_)));

    let missing = fixture
        .get_handler()
        .execute(
            GetApplicationPublicationRouteIntent {
                organization_id: fixture.release.organization_id,
                project_id: fixture.release.project_id,
                application_id: fixture.release.application_id,
                intent_id: ApplicationPublicationRouteIntentId::new(),
                actor_principal_id: fixture.actor,
                access: fixture.access.clone(),
            },
            context(),
        )
        .await
        .expect("boot")
        .expect_err("missing intent");
    assert!(matches!(missing, ApplicationError::NotFound(_)));
}

#[test]
fn publication_route_intent_handlers_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CreateApplicationPublicationRouteIntentHandler>();
    assert_send_sync::<GetApplicationPublicationRouteIntentHandler>();
    assert_send_sync::<ListApplicationPublicationRouteIntentsByReleaseHandler>();
}
