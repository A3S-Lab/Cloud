use super::begin_recipient_contact_verification::{
    BeginRecipientContactVerification, BeginRecipientContactVerificationHandler,
};
use super::bootstrap_identity::{BootstrapIdentity, BootstrapIdentityHandler};
use super::complete_recipient_contact_verification::{
    CompleteRecipientContactVerification, CompleteRecipientContactVerificationHandler,
};
use super::create_membership::{CreateMembership, CreateMembershipHandler};
use super::create_organization::{CreateOrganization, CreateOrganizationHandler};
use super::revoke_recipient_contact::{RevokeRecipientContact, RevokeRecipientContactHandler};
use crate::modules::identity::application::queries::get_recipient_contact::{
    GetRecipientContact, GetRecipientContactHandler,
};
use crate::modules::identity::application::queries::list_recipient_contacts::{
    ListRecipientContacts, ListRecipientContactsHandler,
};
use crate::modules::identity::domain::entities::RecipientContactStatus;
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IMembershipRepository, IOrganizationRepository,
    IRecipientContactRepository,
};
use crate::modules::identity::domain::services::IRecipientContactProofService;
use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
use crate::modules::identity::infrastructure::HmacRecipientContactProofService;
use crate::modules::identity::InMemoryIdentityRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

struct Fixture {
    repository: Arc<InMemoryIdentityRepository>,
    proof_service: Arc<HmacRecipientContactProofService>,
    organization_id: OrganizationId,
    service_owner_id: PrincipalId,
    principal_id: PrincipalId,
    other_principal_id: PrincipalId,
}

impl Fixture {
    async fn new() -> Self {
        let repository = Arc::new(InMemoryIdentityRepository::new());
        let api_tokens: Arc<dyn IApiTokenRepository> = repository.clone();
        let bootstrap = BootstrapIdentityHandler::new(api_tokens)
            .execute(
                BootstrapIdentity {
                    organization_name: "Recipient Contact Test".into(),
                    token_name: "recipient-contact-bootstrap".into(),
                    token_secret: format!("a3s_{}", "a".repeat(64)),
                    expires_at: None,
                    idempotency_key: format!("test:{}", Uuid::new_v4()),
                    request_id: Uuid::new_v4(),
                },
                context(),
            )
            .await
            .expect("bootstrap command")
            .expect("bootstrap identity");
        let organization_id = bootstrap.identity.organization.id;
        let service_owner_id = bootstrap.identity.principal.id;
        let principal_id = create_human(
            &repository,
            organization_id,
            service_owner_id,
            "Recipient Contact Owner",
        )
        .await;
        let other_principal_id = create_human(
            &repository,
            organization_id,
            service_owner_id,
            "Other Recipient Contact Owner",
        )
        .await;
        let proof_service = Arc::new(
            HmacRecipientContactProofService::new(
                RecipientContactSigningKeyId::parse("recipient-contact-v1").expect("key ID"),
                Zeroizing::new(vec![0x42; 32]),
            )
            .expect("proof service"),
        );
        Self {
            repository,
            proof_service,
            organization_id,
            service_owner_id,
            principal_id,
            other_principal_id,
        }
    }

    fn repository_port(&self) -> Arc<dyn IRecipientContactRepository> {
        self.repository.clone()
    }

    fn proof_port(&self) -> Arc<dyn IRecipientContactProofService> {
        self.proof_service.clone()
    }

    async fn begin(
        &self,
        actor_principal_id: PrincipalId,
        address: &str,
        key: &str,
    ) -> Result<
        crate::modules::identity::application::RecipientContactVerificationRequestResult,
        ApplicationError,
    > {
        BeginRecipientContactVerificationHandler::new(self.repository_port(), self.proof_port())
            .execute(
                BeginRecipientContactVerification {
                    organization_id: self.organization_id,
                    actor_principal_id,
                    address: Zeroizing::new(address.to_owned()),
                    idempotency_key: key.into(),
                    request_id: Uuid::new_v4(),
                },
                context(),
            )
            .await
            .expect("begin command")
    }
}

async fn create_human(
    repository: &Arc<InMemoryIdentityRepository>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    name: &str,
) -> PrincipalId {
    let memberships: Arc<dyn IMembershipRepository> = repository.clone();
    CreateMembershipHandler::new(memberships)
        .execute(
            CreateMembership {
                organization_id,
                principal_kind: "human".into(),
                name: name.into(),
                role: "member".into(),
                actor_principal_id,
                actor_is_platform_admin: false,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("membership command")
        .expect("human membership")
        .membership
        .principal
        .id
}

#[tokio::test]
async fn application_boundary_is_exact_redacted_replay_safe_and_terminal() {
    let fixture = Fixture::new().await;
    let mailbox = "private.owner@example.com";
    let begin_command = BeginRecipientContactVerification {
        organization_id: fixture.organization_id,
        actor_principal_id: fixture.principal_id,
        address: Zeroizing::new(mailbox.into()),
        idempotency_key: "begin-redaction".into(),
        request_id: Uuid::new_v4(),
    };
    assert!(!format!("{begin_command:?}").contains(mailbox));
    let begun = BeginRecipientContactVerificationHandler::new(
        fixture.repository_port(),
        fixture.proof_port(),
    )
    .execute(begin_command, context())
    .await
    .expect("begin command")
    .expect("begin verification");
    assert_eq!(begun.contact.status, RecipientContactStatus::Pending);
    assert!(!format!("{begun:?}").contains(mailbox));

    let proof = fixture
        .proof_service
        .issue(&begun.verification)
        .await
        .expect("issued proof");
    let wrong_principal = CompleteRecipientContactVerificationHandler::new(
        fixture.repository_port(),
        fixture.proof_port(),
    )
    .execute(
        CompleteRecipientContactVerification {
            organization_id: fixture.organization_id,
            actor_principal_id: fixture.other_principal_id,
            contact_id: begun.contact.id,
            proof: proof.clone(),
            idempotency_key: "complete-wrong-principal".into(),
            request_id: Uuid::new_v4(),
        },
        context(),
    )
    .await
    .expect("wrong-principal command");
    assert!(matches!(
        wrong_principal,
        Err(ApplicationError::Forbidden(_))
    ));

    let completion = || CompleteRecipientContactVerification {
        organization_id: fixture.organization_id,
        actor_principal_id: fixture.principal_id,
        contact_id: begun.contact.id,
        proof: proof.clone(),
        idempotency_key: "complete-current".into(),
        request_id: Uuid::new_v4(),
    };
    let completion_debug = completion();
    assert!(!format!("{completion_debug:?}").contains(proof.as_str()));
    let completed = CompleteRecipientContactVerificationHandler::new(
        fixture.repository_port(),
        fixture.proof_port(),
    )
    .execute(completion_debug, context())
    .await
    .expect("complete command")
    .expect("complete verification");
    assert_eq!(completed.contact.status, RecipientContactStatus::Verified);
    assert_eq!(completed.contact.aggregate_version, 2);
    let replay = CompleteRecipientContactVerificationHandler::new(
        fixture.repository_port(),
        fixture.proof_port(),
    )
    .execute(completion(), context())
    .await
    .expect("replay command")
    .expect("replayed completion");
    assert!(replay.replayed);

    let listed = ListRecipientContactsHandler::new(fixture.repository_port())
        .execute(
            ListRecipientContacts {
                organization_id: fixture.organization_id,
                actor_principal_id: fixture.principal_id,
            },
            context(),
        )
        .await
        .expect("list query")
        .expect("listed contacts");
    assert_eq!(listed, vec![completed.contact.clone()]);
    let loaded = GetRecipientContactHandler::new(fixture.repository_port())
        .execute(
            GetRecipientContact {
                organization_id: fixture.organization_id,
                actor_principal_id: fixture.principal_id,
                contact_id: completed.contact.id,
            },
            context(),
        )
        .await
        .expect("get query")
        .expect("loaded contact");
    assert_eq!(loaded, completed.contact);
    let foreign = GetRecipientContactHandler::new(fixture.repository_port())
        .execute(
            GetRecipientContact {
                organization_id: fixture.organization_id,
                actor_principal_id: fixture.other_principal_id,
                contact_id: loaded.id,
            },
            context(),
        )
        .await
        .expect("foreign get query");
    assert!(matches!(foreign, Err(ApplicationError::NotFound(_))));

    let revoke = || RevokeRecipientContact {
        organization_id: fixture.organization_id,
        actor_principal_id: fixture.principal_id,
        contact_id: loaded.id,
        expected_version: 2,
        idempotency_key: "revoke-current".into(),
        request_id: Uuid::new_v4(),
    };
    let revoked = RevokeRecipientContactHandler::new(fixture.repository_port())
        .execute(revoke(), context())
        .await
        .expect("revoke command")
        .expect("revoked contact");
    assert_eq!(revoked.contact.status, RecipientContactStatus::Revoked);
    assert_eq!(revoked.contact.aggregate_version, 3);
    assert!(
        RevokeRecipientContactHandler::new(fixture.repository_port())
            .execute(revoke(), context())
            .await
            .expect("revoke replay command")
            .expect("replayed revocation")
            .replayed
    );
    assert!(fixture
        .repository
        .resolve_verified_recipient_contact(
            fixture.organization_id,
            fixture.principal_id,
            loaded.id,
        )
        .await
        .expect("internal resolver")
        .is_none());
}

#[tokio::test]
async fn reissue_invalidates_delivered_proof_and_service_principals_fail_closed() {
    let fixture = Fixture::new().await;
    let first = fixture
        .begin(fixture.principal_id, "alerts@example.com", "begin-first")
        .await
        .expect("first challenge");
    let first_proof = fixture
        .proof_service
        .issue(&first.verification)
        .await
        .expect("first proof");
    let organizations: Arc<dyn IOrganizationRepository> = fixture.repository.clone();
    let other_organization = CreateOrganizationHandler::new(organizations)
        .execute(
            CreateOrganization {
                name: "Other Recipient Contact Context".into(),
                actor_principal_id: fixture.principal_id,
                idempotency_key: "create-other-context".into(),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("other organization command")
        .expect("other organization")
        .organization;
    let cross_organization = CompleteRecipientContactVerificationHandler::new(
        fixture.repository_port(),
        fixture.proof_port(),
    )
    .execute(
        CompleteRecipientContactVerification {
            organization_id: other_organization.id,
            actor_principal_id: fixture.principal_id,
            contact_id: first.contact.id,
            proof: first_proof.clone(),
            idempotency_key: "complete-other-context".into(),
            request_id: Uuid::new_v4(),
        },
        context(),
    )
    .await
    .expect("cross-organization completion command");
    assert!(matches!(
        cross_organization,
        Err(ApplicationError::Conflict(_))
    ));
    let second = fixture
        .begin(fixture.principal_id, "ALERTS@EXAMPLE.COM", "begin-second")
        .await
        .expect("second challenge");
    assert_eq!(second.contact.id, first.contact.id);
    let stale = CompleteRecipientContactVerificationHandler::new(
        fixture.repository_port(),
        fixture.proof_port(),
    )
    .execute(
        CompleteRecipientContactVerification {
            organization_id: fixture.organization_id,
            actor_principal_id: fixture.principal_id,
            contact_id: first.contact.id,
            proof: first_proof,
            idempotency_key: "complete-stale".into(),
            request_id: Uuid::new_v4(),
        },
        context(),
    )
    .await
    .expect("stale completion command");
    assert!(matches!(stale, Err(ApplicationError::Conflict(_))));

    let invalid = fixture
        .begin(fixture.principal_id, "not-an-email", "begin-invalid")
        .await;
    assert!(matches!(invalid, Err(ApplicationError::Invalid(_))));
    let service = fixture
        .begin(
            fixture.service_owner_id,
            "service@example.com",
            "begin-service",
        )
        .await;
    assert!(matches!(service, Err(ApplicationError::Forbidden(_))));
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
