use super::*;
use crate::modules::connectors::domain::{
    ConnectorHttpAuthentication, ConnectorHttpDefinition, ConnectorHttpDefinitionSpec,
    ConnectorHttpDestination, ConnectorHttpMethod, ConnectorHttpStatusPolicy,
    ConnectorSecretReference,
};
use crate::modules::connectors::infrastructure::InMemoryConnectorProfileRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::events::EnvironmentCreated;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
use crate::modules::secrets::domain::{
    CreateSecretWrite, EncryptedSecretValue, ISecretRepository, Secret, SecretChanged,
    TransitionSecretVersion,
};
use crate::modules::secrets::infrastructure::InMemorySecretRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, ResourceName,
    SecretId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use chrono::{Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn cqrs_authorizes_before_replay_and_preserves_exact_history() {
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let actor_principal_id = PrincipalId::new();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let environment = Environment::create(
        organization_id,
        project_id,
        environment_id,
        EnvironmentName::parse("Production").expect("environment name"),
        now,
    );
    IEnvironmentRepository::create(
        projects.as_ref(),
        environment.clone(),
        EnvironmentCreated::envelope(&environment, Uuid::now_v7()).expect("environment event"),
        IdempotencyRequest::new(
            "connector-application-environment",
            "create",
            environment_id.as_uuid().as_bytes(),
        )
        .expect("environment idempotency"),
    )
    .await
    .expect("store environment");

    let secrets = Arc::new(InMemorySecretRepository::new());
    let (mut destination, mut destination_version) = create_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        now,
    )
    .await;
    let connectors = Arc::new(InMemoryConnectorProfileRepository::new());
    let create_handler =
        CreateConnectorProfileHandler::new(projects, connectors.clone(), secrets.clone());
    let create = CreateConnectorProfile {
        organization_id,
        project_id,
        environment_id,
        name: "Incident delivery".into(),
        definition_acl: secret_definition(destination.id, 1, 1_000),
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "create-incident-delivery".into(),
        request_id: Uuid::now_v7(),
    };
    let created = create_handler
        .execute(create.clone(), context())
        .await
        .expect("command framework")
        .expect("create Connector profile");
    assert!(!created.replayed);

    let denied_replay = create_handler
        .execute(
            CreateConnectorProfile {
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
                ..create.clone()
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(denied_replay, Err(ApplicationError::NotFound(_))));

    let replay = create_handler
        .execute(create.clone(), context())
        .await
        .expect("command framework")
        .expect("create replay");
    assert!(replay.replayed);
    assert_eq!(replay.record, created.record);

    let conflicting = create_handler
        .execute(
            CreateConnectorProfile {
                definition_acl: literal_definition("changed", 2_000),
                ..create.clone()
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(conflicting, Err(ApplicationError::Conflict(_))));

    let expected_secret_version = destination.aggregate_version;
    let expected_version = destination_version.aggregate_version;
    destination
        .revoke_version(&mut destination_version, now + Duration::seconds(1))
        .expect("revoke destination version");
    let event = SecretChanged::version_revoked(&destination, &destination_version, Uuid::now_v7())
        .expect("Secret revocation event");
    secrets
        .transition_version(TransitionSecretVersion {
            secret: destination,
            version: destination_version,
            expected_secret_version,
            expected_version,
            idempotency: IdempotencyRequest::new(
                "connector-application-secret",
                "revoke",
                b"revoke",
            )
            .expect("Secret revocation idempotency"),
            event,
        })
        .await
        .expect("store Secret revocation");

    assert!(
        create_handler
            .execute(create.clone(), context())
            .await
            .expect("command framework")
            .expect("replay survives later Secret revocation")
            .replayed
    );
    let new_write_with_revoked_secret = create_handler
        .execute(
            CreateConnectorProfile {
                name: "Revoked destination".into(),
                idempotency_key: "new-write-after-revoke".into(),
                request_id: Uuid::now_v7(),
                ..create
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(
        new_write_with_revoked_secret,
        Err(ApplicationError::Invalid(_))
    ));

    let revise_handler = ReviseConnectorProfileHandler::new(connectors.clone(), secrets);
    let revise = ReviseConnectorProfile {
        organization_id,
        project_id,
        environment_id,
        profile_id: created.record.profile.id,
        expected_version: 1,
        definition_acl: literal_definition("revised", 2_000),
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "revise-incident-delivery".into(),
        request_id: Uuid::now_v7(),
    };
    let revised = revise_handler
        .execute(revise.clone(), context())
        .await
        .expect("command framework")
        .expect("revise Connector profile");
    assert!(!revised.replayed);
    assert_eq!(revised.record.profile.aggregate_version, 2);
    assert!(
        revise_handler
            .execute(revise, context())
            .await
            .expect("command framework")
            .expect("revision replay")
            .replayed
    );

    let get = GetConnectorProfileHandler::new(connectors.clone())
        .execute(
            GetConnectorProfile {
                organization_id,
                project_id,
                environment_id,
                profile_id: created.record.profile.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get Connector profile");
    assert_eq!(get, revised.record);

    let hidden = GetConnectorProfileHandler::new(connectors.clone())
        .execute(
            GetConnectorProfile {
                organization_id,
                project_id,
                environment_id,
                profile_id: created.record.profile.id,
                resource_access: ResourceAccessEvaluator::restricted([
                    ResourceGrantScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    },
                ]),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(hidden, Err(ApplicationError::NotFound(_))));

    let profiles = ListConnectorProfilesHandler::new(connectors.clone())
        .execute(
            ListConnectorProfiles {
                organization_id,
                project_id,
                environment_id,
                limit: 50,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("list profiles");
    assert_eq!(profiles, vec![revised.record.profile.clone()]);

    let unbounded_profiles = ListConnectorProfilesHandler::new(connectors.clone())
        .execute(
            ListConnectorProfiles {
                organization_id,
                project_id,
                environment_id,
                limit: MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT + 1,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(
        unbounded_profiles,
        Err(ApplicationError::Invalid(_))
    ));

    let history = ListConnectorRevisionsHandler::new(connectors.clone())
        .execute(
            ListConnectorRevisions {
                organization_id,
                project_id,
                environment_id,
                profile_id: created.record.profile.id,
                limit: 50,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("list revisions");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0], revised.record.revision);
    assert_eq!(history[1], created.record.revision);

    let empty_history = ListConnectorRevisionsHandler::new(connectors.clone())
        .execute(
            ListConnectorRevisions {
                organization_id,
                project_id,
                environment_id,
                profile_id: created.record.profile.id,
                limit: 0,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(empty_history, Err(ApplicationError::Invalid(_))));

    let initial_revision = GetConnectorRevisionHandler::new(connectors.clone())
        .execute(
            GetConnectorRevision {
                organization_id,
                project_id,
                environment_id,
                profile_id: created.record.profile.id,
                revision_id: created.record.revision.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get initial revision");
    assert_eq!(initial_revision, created.record.revision);
    assert_eq!(connectors.outbox_events().await.len(), 2);
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn secret_definition(secret_id: SecretId, version: u64, timeout_milliseconds: u64) -> String {
    ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
        destination: ConnectorHttpDestination::SecretHttpsUrl {
            reference: ConnectorSecretReference::new(secret_id, version)
                .expect("Secret destination reference"),
        },
        method: ConnectorHttpMethod::Post,
        request_content_type: "application/json".into(),
        maximum_request_bytes: 1024,
        maximum_response_bytes: 1024,
        timeout_milliseconds,
        status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
        authentication: ConnectorHttpAuthentication::None,
    })
    .expect("Secret Connector definition")
    .canonical_acl()
    .to_owned()
}

fn literal_definition(path: &str, timeout_milliseconds: u64) -> String {
    ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
        destination: ConnectorHttpDestination::LiteralHttps {
            endpoint: format!("https://hooks.example.test/{path}"),
        },
        method: ConnectorHttpMethod::Post,
        request_content_type: "application/json".into(),
        maximum_request_bytes: 1024,
        maximum_response_bytes: 1024,
        timeout_milliseconds,
        status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
        authentication: ConnectorHttpAuthentication::None,
    })
    .expect("literal Connector definition")
    .canonical_acl()
    .to_owned()
}

async fn create_secret(
    repository: &InMemorySecretRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    created_at: chrono::DateTime<Utc>,
) -> (Secret, crate::modules::secrets::domain::SecretVersion) {
    let secret_id = SecretId::new();
    let (secret, version) = Secret::create(
        secret_id,
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("Connector destination").expect("Secret name"),
        EncryptedSecretValue::new("test-key", "destination-ciphertext").expect("encrypted value"),
        created_at,
    )
    .expect("Secret");
    repository
        .create(CreateSecretWrite {
            secret: secret.clone(),
            version: version.clone(),
            idempotency: IdempotencyRequest::new(
                "connector-application-secret",
                "create",
                secret_id.as_uuid().as_bytes(),
            )
            .expect("Secret idempotency"),
            event: SecretChanged::created(&secret, &version, Uuid::now_v7()).expect("Secret event"),
        })
        .await
        .expect("store Secret");
    (secret, version)
}
