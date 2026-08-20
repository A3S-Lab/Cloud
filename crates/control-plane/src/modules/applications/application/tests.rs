use super::*;
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationResponseMode, ApplicationWorkflowBinding, ApplicationWorkflowRevisionEvidence,
};
use crate::modules::applications::infrastructure::InMemoryApplicationRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

struct ExactWorkflowEvidence {
    evidence: ApplicationWorkflowRevisionEvidence,
    calls: AtomicUsize,
}

#[async_trait]
impl IApplicationWorkflowRevisionPort for ExactWorkflowEvidence {
    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
        workflow_revision_id: WorkflowRevisionId,
    ) -> ApplicationResult<ApplicationWorkflowRevisionEvidence> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.evidence.organization_id != organization_id
            || self.evidence.project_id != project_id
            || self.evidence.binding.workflow_definition_id != workflow_definition_id
            || self.evidence.binding.workflow_revision_id != workflow_revision_id
        {
            return Err(ApplicationError::NotFound(
                "Workflow revision not found".into(),
            ));
        }
        Ok(self.evidence.clone())
    }
}

#[tokio::test]
async fn cqrs_authorizes_before_replay_and_preserves_exact_release_history() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor_principal_id = PrincipalId::new();
    let evidence = evidence(organization_id, project_id);
    let workflow = Arc::new(ExactWorkflowEvidence {
        evidence: evidence.clone(),
        calls: AtomicUsize::new(0),
    });
    let applications = Arc::new(InMemoryApplicationRepository::new());
    let create_handler = CreateApplicationHandler::new(applications.clone(), workflow.clone());
    let initial_contract = contract(&evidence, '1');
    let create = CreateApplication {
        organization_id,
        project_id,
        name: "Support copilot".into(),
        description: "Project-scoped support experience".into(),
        release_acl: initial_contract.canonical_acl().into(),
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::restricted([ResourceGrantScope::Project {
            project_id,
        }]),
        idempotency_key: "application-create".into(),
        request_id: Uuid::now_v7(),
    };
    let created = create_handler
        .execute(create.clone(), context())
        .await
        .expect("command framework")
        .expect("create Application");
    assert!(!created.replayed);
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 1);

    let denied = create_handler
        .execute(
            CreateApplication {
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
    assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 1);

    let replay = create_handler
        .execute(create.clone(), context())
        .await
        .expect("command framework")
        .expect("create replay");
    assert!(replay.replayed);
    assert_eq!(replay.record, created.record);
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 1);

    let conflicting = create_handler
        .execute(
            CreateApplication {
                release_acl: contract(&evidence, '2').canonical_acl().into(),
                ..create
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(conflicting, Err(ApplicationError::Conflict(_))));
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 1);

    let publish_handler =
        PublishApplicationReleaseHandler::new(applications.clone(), workflow.clone());
    let second_contract = contract(&evidence, '2');
    let publish = PublishApplicationRelease {
        organization_id,
        project_id,
        application_id: created.record.application.id,
        expected_version: 1,
        release_acl: second_contract.canonical_acl().into(),
        actor_principal_id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "application-publish-2".into(),
        request_id: Uuid::now_v7(),
    };
    let published = publish_handler
        .execute(publish.clone(), context())
        .await
        .expect("command framework")
        .expect("publish release");
    assert!(!published.replayed);
    assert_eq!(published.record.application.aggregate_version, 2);
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 2);

    let published_replay = publish_handler
        .execute(publish, context())
        .await
        .expect("command framework")
        .expect("publication replay");
    assert!(published_replay.replayed);
    assert_eq!(published_replay.record, published.record);
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 2);

    let stale = publish_handler
        .execute(
            PublishApplicationRelease {
                organization_id,
                project_id,
                application_id: created.record.application.id,
                expected_version: 1,
                release_acl: contract(&evidence, '3').canonical_acl().into(),
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "application-stale".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(stale, Err(ApplicationError::Conflict(_))));
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 2);

    let mut drifted_spec = contract(&evidence, '3').spec().clone();
    drifted_spec.workflow.workflow_contract_digest = digest('9');
    let drifted = ApplicationReleaseContract::from_spec(drifted_spec).expect("drifted contract");
    let rejected = publish_handler
        .execute(
            PublishApplicationRelease {
                organization_id,
                project_id,
                application_id: created.record.application.id,
                expected_version: 2,
                release_acl: drifted.canonical_acl().into(),
                actor_principal_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "application-drifted-workflow".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(rejected, Err(ApplicationError::Conflict(_))));
    assert_eq!(workflow.calls.load(Ordering::SeqCst), 3);

    let current = GetApplicationHandler::new(applications.clone())
        .execute(
            GetApplication {
                organization_id,
                project_id,
                application_id: created.record.application.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("get Application");
    assert_eq!(current, published.record.application);

    let hidden = GetApplicationHandler::new(applications.clone())
        .execute(
            GetApplication {
                organization_id,
                project_id,
                application_id: created.record.application.id,
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

    let listed = ListApplicationsHandler::new(applications.clone())
        .execute(
            ListApplications {
                organization_id,
                project_id,
                limit: None,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("list Applications");
    assert_eq!(listed, vec![published.record.application.clone()]);

    let history = ListApplicationReleasesHandler::new(applications.clone())
        .execute(
            ListApplicationReleases {
                organization_id,
                project_id,
                application_id: created.record.application.id,
                limit: Some(50),
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework")
        .expect("list Application releases");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0], published.record.release);
    assert_eq!(history[1], created.record.release);

    let unbounded = ListApplicationsHandler::new(applications.clone())
        .execute(
            ListApplications {
                organization_id,
                project_id,
                limit: Some(MAXIMUM_APPLICATION_LIST_LIMIT + 1),
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            context(),
        )
        .await
        .expect("query framework");
    assert!(matches!(unbounded, Err(ApplicationError::Invalid(_))));
    assert_eq!(applications.outbox_events().await.len(), 2);
}

#[test]
fn application_component_reuses_shared_authorities_without_new_runtime_mechanisms() {
    let source = [
        include_str!("commands.rs"),
        include_str!("queries.rs"),
        include_str!("workflow_revision_port.rs"),
        include_str!("../infrastructure/application_in_memory.rs"),
        include_str!("../infrastructure/workflow_revision.rs"),
    ]
    .join("\n")
    .to_ascii_lowercase();
    for forbidden in [
        "a3s_flow",
        "tokio::spawn",
        "reqwest",
        "application_session",
        "application_message",
        "retry_count",
        "provider_endpoint",
    ] {
        assert!(
            !source.contains(forbidden),
            "Applications component duplicated another authority through {forbidden}"
        );
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn evidence(
    organization_id: OrganizationId,
    project_id: ProjectId,
) -> ApplicationWorkflowRevisionEvidence {
    ApplicationWorkflowRevisionEvidence {
        organization_id,
        project_id,
        binding: ApplicationWorkflowBinding {
            workflow_definition_id: WorkflowDefinitionId::new(),
            workflow_revision_id: WorkflowRevisionId::new(),
            workflow_contract_digest: digest('a'),
            workflow_payload_set_digest: digest('b'),
            workflow_semantic_contract_set_digest: digest('c'),
            input_schema_digest: digest('d'),
            output_schema_digest: digest('e'),
        },
    }
}

fn contract(
    evidence: &ApplicationWorkflowRevisionEvidence,
    presentation: char,
) -> ApplicationReleaseContract {
    ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![
                ApplicationResponseMode::Blocking,
                ApplicationResponseMode::Streaming,
            ],
        },
        workflow: evidence.binding.clone(),
        presentation_digest: digest(presentation),
    })
    .expect("Application release contract")
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64))).expect("digest")
}
