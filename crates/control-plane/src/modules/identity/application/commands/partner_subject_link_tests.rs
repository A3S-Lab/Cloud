use super::link_partner_subject::{LinkPartnerSubject, LinkPartnerSubjectHandler};
use super::revoke_partner_subject_link::{
    RevokePartnerSubjectLink, RevokePartnerSubjectLinkHandler,
};
use crate::modules::identity::application::commands::bootstrap_identity::{
    BootstrapIdentity, BootstrapIdentityHandler,
};
use crate::modules::identity::application::commands::create_membership::{
    CreateMembership, CreateMembershipHandler,
};
use crate::modules::identity::application::queries::list_partner_subject_links::{
    ListPartnerSubjectLinks, ListPartnerSubjectLinksHandler,
};
use crate::modules::identity::application::queries::resolve_partner_subject::{
    ResolvePartnerSubject, ResolvePartnerSubjectHandler,
};
use crate::modules::identity::domain::repositories::{
    IIdentityBootstrapRepository, IMembershipRepository, IPartnerSubjectLinkRepository,
};
use crate::modules::identity::domain::value_objects::KENSE_DIRECTORY_PROVIDER_KEY;
use crate::modules::identity::infrastructure::persistence::InMemoryIdentityRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use std::sync::Arc;
use uuid::Uuid;

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

const ISSUER: &str = "https://kense.example/directory";

struct Fixture {
    repository: Arc<InMemoryIdentityRepository>,
    organization_id: OrganizationId,
    owner_principal_id: PrincipalId,
    human_principal_id: PrincipalId,
}

impl Fixture {
    async fn new() -> Self {
        let repository = Arc::new(InMemoryIdentityRepository::new());
        let bootstrap: Arc<dyn IIdentityBootstrapRepository> = repository.clone();
        let bootstrapped = BootstrapIdentityHandler::new(bootstrap)
            .execute(
                BootstrapIdentity {
                    organization_name: "Partner Org".into(),
                    token_name: "bootstrap".into(),
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
        let organization_id = bootstrapped.identity.organization.id;
        let owner_id = bootstrapped.identity.principal.id;
        let memberships: Arc<dyn IMembershipRepository> = repository.clone();
        let human = CreateMembershipHandler::new(memberships)
            .execute(
                CreateMembership {
                    organization_id,
                    principal_kind: "human".into(),
                    name: "Kense Member".into(),
                    role: "member".into(),
                    actor_principal_id: owner_id,
                    idempotency_key: format!("test:{}", Uuid::new_v4()),
                    request_id: Uuid::new_v4(),
                },
                context(),
            )
            .await
            .expect("membership command")
            .expect("human membership");
        Self {
            repository,
            organization_id,
            owner_principal_id: owner_id,
            human_principal_id: human.membership.principal.id,
        }
    }

    fn links(&self) -> Arc<dyn IPartnerSubjectLinkRepository> {
        self.repository.clone()
    }

    fn memberships(&self) -> Arc<dyn IMembershipRepository> {
        self.repository.clone()
    }
}

#[tokio::test]
async fn link_resolve_and_revoke_partner_subject() {
    let fixture = Fixture::new().await;
    let subject = Uuid::new_v4().to_string();
    let link_handler =
        LinkPartnerSubjectHandler::new(fixture.links(), fixture.memberships());
    let linked = link_handler
        .execute(
            LinkPartnerSubject {
                organization_id: fixture.organization_id,
                provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                issuer: ISSUER.into(),
                subject: subject.clone(),
                principal_id: fixture.human_principal_id,
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("link command")
        .expect("link partner subject");
    assert_eq!(linked.link.principal_id, fixture.human_principal_id);
    assert_eq!(linked.link.subject.as_str(), subject.as_str());
    assert!(linked.link.revoked_at.is_none());
    assert!(!linked.replayed);

    let resolved = ResolvePartnerSubjectHandler::new(fixture.links(), fixture.memberships())
        .execute(
            ResolvePartnerSubject {
                organization_id: fixture.organization_id,
                provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                issuer: ISSUER.into(),
                subject: subject.clone(),
            },
            context(),
        )
        .await
        .expect("resolve query")
        .expect("resolve partner subject");
    assert_eq!(resolved.link_id, linked.link.id.as_uuid());
    assert_eq!(resolved.principal_id, fixture.human_principal_id.as_uuid());

    RevokePartnerSubjectLinkHandler::new(fixture.links())
        .execute(
            RevokePartnerSubjectLink {
                organization_id: fixture.organization_id,
                provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                issuer: ISSUER.into(),
                subject: subject.clone(),
                expected_version: linked.link.aggregate_version,
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("revoke command")
        .expect("revoke partner subject");

    let after_revoke =
        ResolvePartnerSubjectHandler::new(fixture.links(), fixture.memberships())
            .execute(
                ResolvePartnerSubject {
                    organization_id: fixture.organization_id,
                    provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                    issuer: ISSUER.into(),
                    subject,
                },
                context(),
            )
            .await
            .expect("resolve after revoke");
    assert!(matches!(after_revoke, Err(ApplicationError::NotFound(_))));
}

#[tokio::test]
async fn duplicate_active_subject_for_another_principal_is_rejected() {
    let fixture = Fixture::new().await;
    let subject = Uuid::new_v4().to_string();
    let memberships: Arc<dyn IMembershipRepository> = fixture.repository.clone();
    let other = CreateMembershipHandler::new(memberships)
        .execute(
            CreateMembership {
                organization_id: fixture.organization_id,
                principal_kind: "human".into(),
                name: "Other Kense Member".into(),
                role: "member".into(),
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("membership command")
        .expect("other human membership");
    let handler = LinkPartnerSubjectHandler::new(fixture.links(), fixture.memberships());

    handler
        .execute(
            LinkPartnerSubject {
                organization_id: fixture.organization_id,
                provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                issuer: ISSUER.into(),
                subject: subject.clone(),
                principal_id: fixture.human_principal_id,
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("first link")
        .expect("linked");

    let duplicate = handler
        .execute(
            LinkPartnerSubject {
                organization_id: fixture.organization_id,
                provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                issuer: ISSUER.into(),
                subject,
                principal_id: other.membership.principal.id,
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("second link command");
    let Err(error) = duplicate else {
        panic!("duplicate subject was accepted for another principal");
    };
    assert!(matches!(error, ApplicationError::Conflict(_)));
}

#[tokio::test]
async fn non_uuid_subject_is_rejected() {
    let fixture = Fixture::new().await;
    let error = LinkPartnerSubjectHandler::new(fixture.links(), fixture.memberships())
        .execute(
            LinkPartnerSubject {
                organization_id: fixture.organization_id,
                provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                issuer: ISSUER.into(),
                subject: "not-a-uuid".into(),
                principal_id: fixture.human_principal_id,
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("link command");
    let Err(error) = error else {
        panic!("non-uuid subject was accepted");
    };
    assert!(matches!(error, ApplicationError::Invalid(_)));
}

#[tokio::test]
async fn non_partner_provider_key_is_rejected() {
    let fixture = Fixture::new().await;
    let error = LinkPartnerSubjectHandler::new(fixture.links(), fixture.memberships())
        .execute(
            LinkPartnerSubject {
                organization_id: fixture.organization_id,
                provider_key: "workforce".into(),
                issuer: ISSUER.into(),
                subject: Uuid::new_v4().to_string(),
                principal_id: fixture.human_principal_id,
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("link command");
    let Err(error) = error else {
        panic!("non-partner provider key was accepted");
    };
    assert!(matches!(error, ApplicationError::Invalid(_)));
}

#[tokio::test]
async fn list_partner_subject_links_for_principal_with_optional_provider_filter() {
    let fixture = Fixture::new().await;
    let subject = Uuid::new_v4().to_string();
    let linked = LinkPartnerSubjectHandler::new(fixture.links(), fixture.memberships())
        .execute(
            LinkPartnerSubject {
                organization_id: fixture.organization_id,
                provider_key: KENSE_DIRECTORY_PROVIDER_KEY.into(),
                issuer: ISSUER.into(),
                subject,
                principal_id: fixture.human_principal_id,
                actor_principal_id: fixture.owner_principal_id,
                idempotency_key: format!("test:{}", Uuid::new_v4()),
                request_id: Uuid::new_v4(),
            },
            context(),
        )
        .await
        .expect("link command")
        .expect("link partner subject");

    let listed = ListPartnerSubjectLinksHandler::new(fixture.links(), fixture.memberships())
        .execute(
            ListPartnerSubjectLinks {
                organization_id: fixture.organization_id,
                principal_id: fixture.human_principal_id,
                provider_key: None,
            },
            context(),
        )
        .await
        .expect("list query")
        .expect("list partner subject links");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].link_id, linked.link.id.as_uuid());

    let filtered = ListPartnerSubjectLinksHandler::new(fixture.links(), fixture.memberships())
        .execute(
            ListPartnerSubjectLinks {
                organization_id: fixture.organization_id,
                principal_id: fixture.human_principal_id,
                provider_key: Some(KENSE_DIRECTORY_PROVIDER_KEY.into()),
            },
            context(),
        )
        .await
        .expect("filtered list query")
        .expect("filtered list");
    assert_eq!(filtered.len(), 1);

    let empty = ListPartnerSubjectLinksHandler::new(fixture.links(), fixture.memberships())
        .execute(
            ListPartnerSubjectLinks {
                organization_id: fixture.organization_id,
                principal_id: fixture.human_principal_id,
                provider_key: Some("partner-other-directory".into()),
            },
            context(),
        )
        .await
        .expect("empty filter query")
        .expect("empty filter");
    assert!(empty.is_empty());
}

#[tokio::test]
async fn list_partner_subject_links_rejects_non_member_principal() {
    let fixture = Fixture::new().await;
    let unknown = PrincipalId::from_uuid(Uuid::new_v4());
    let result = ListPartnerSubjectLinksHandler::new(fixture.links(), fixture.memberships())
        .execute(
            ListPartnerSubjectLinks {
                organization_id: fixture.organization_id,
                principal_id: unknown,
                provider_key: None,
            },
            context(),
        )
        .await
        .expect("list query");
    assert!(matches!(result, Err(ApplicationError::NotFound(_))));
}
