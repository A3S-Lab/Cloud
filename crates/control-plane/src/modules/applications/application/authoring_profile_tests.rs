use super::authoring_profile::ApplicationAuthoringProfile;
use super::authoring_profile_commands::{
    PublishApplicationAuthoringProfile, PublishApplicationAuthoringProfileHandler,
};
use super::preset_workflow_port::{
    ApplicationPresetAgentRelease, ApplicationPresetModelRevision, ApplicationPresetTarget,
};
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationReleaseContract, ApplicationResponseMode,
};
use crate::modules::applications::infrastructure::WorkflowApplicationPresetCompiler;
use crate::modules::applications::{ApplicationAccess, ApplicationAccessScope};
use crate::modules::projects::domain::entities::Project;
use crate::modules::projects::domain::events::ProjectCreated;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::projects::domain::value_objects::ProjectName;
use crate::modules::projects::InMemoryProjectsRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    ApplicationId, AssetId, AssetReleaseId, EnvironmentId, IdempotencyRequest, OrganizationId,
    PrincipalId, ProjectId, Sha256Digest,
};
use crate::modules::workflow::{
    IWorkflowDefinitionPublicationPort, InMemoryWorkflowDefinitionRepository,
    ProjectsWorkflowProjectAccessAdapter, WorkflowDefinitionPublicationService,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use chrono::{TimeZone, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn publishes_four_presets_to_stable_release_contracts() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    seed_project(&projects, organization_id, project_id).await;
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let handler = PublishApplicationAuthoringProfileHandler::new(Arc::new(compiler(
        Arc::clone(&projects),
        Arc::clone(&workflows),
    )));
    let cases = [
        (ApplicationExperience::Chatbot, model_target('1')),
        (ApplicationExperience::TextGenerator, model_target('2')),
        (ApplicationExperience::ClassicAgent, agent_target('3')),
        (ApplicationExperience::NewAgent, agent_target('4')),
    ];
    let mut digests = BTreeSet::new();

    for (experience, target) in cases {
        let application_id = ApplicationId::new();
        let command = PublishApplicationAuthoringProfile {
            organization_id,
            project_id,
            application_id,
            application_release_number: 1,
            experience,
            audience: ApplicationAudience::ProjectMembers,
            delivery: delivery(experience),
            target: target.clone(),
            presentation_digest: digest('a'),
            actor_principal_id: actor,
            access: ApplicationAccess::organization_wide(),
            idempotency_key: format!("authoring-{}", experience.as_str()),
            request_id: Uuid::now_v7(),
        };
        let published = handler
            .execute(command.clone(), context())
            .await
            .expect("handler")
            .expect("publish");
        assert!(!published.replayed);
        assert_eq!(published.profile.experience, experience);
        assert_eq!(published.profile.target, target);
        let contract = ApplicationReleaseContract::restore(
            &published.release_contract_acl,
            published.release_contract_digest.as_str(),
        )
        .expect("restore contract");
        assert_eq!(contract.spec().experience, experience);
        assert_eq!(contract.spec().workflow, published.preset.evidence.binding);
        assert!(digests.insert(published.release_contract_digest.clone()));

        let replayed = handler
            .execute(command, context())
            .await
            .expect("handler")
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(
            replayed.release_contract_digest,
            published.release_contract_digest
        );
        assert_eq!(replayed.profile.id, published.profile.id);
    }
}

#[tokio::test]
async fn unauthorized_and_chatflow_fail_closed() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    seed_project(&projects, organization_id, project_id).await;
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let handler =
        PublishApplicationAuthoringProfileHandler::new(Arc::new(compiler(projects, workflows)));

    let denied = handler
        .execute(
            PublishApplicationAuthoringProfile {
                organization_id,
                project_id,
                application_id: ApplicationId::new(),
                application_release_number: 1,
                experience: ApplicationExperience::Chatbot,
                audience: ApplicationAudience::ProjectMembers,
                delivery: delivery(ApplicationExperience::Chatbot),
                target: model_target('a'),
                presentation_digest: digest('a'),
                actor_principal_id: actor,
                access: ApplicationAccess::restricted([ApplicationAccessScope::Environment {
                    project_id,
                    environment_id: EnvironmentId::new(),
                }]),
                idempotency_key: "denied".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("handler");
    assert_eq!(
        denied,
        Err(ApplicationError::NotFound(
            "Application project not found".into()
        ))
    );

    let chatflow = handler
        .execute(
            PublishApplicationAuthoringProfile {
                organization_id,
                project_id,
                application_id: ApplicationId::new(),
                application_release_number: 1,
                experience: ApplicationExperience::Chatflow,
                audience: ApplicationAudience::ProjectMembers,
                delivery: delivery(ApplicationExperience::Chatflow),
                target: model_target('b'),
                presentation_digest: digest('b'),
                actor_principal_id: actor,
                access: ApplicationAccess::organization_wide(),
                idempotency_key: "chatflow".into(),
                request_id: Uuid::now_v7(),
            },
            context(),
        )
        .await
        .expect("handler");
    assert!(matches!(chatflow, Err(ApplicationError::Invalid(_))));
}

#[test]
fn profile_identity_is_stable_and_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ApplicationAuthoringProfile>();
    assert_send_sync::<PublishApplicationAuthoringProfile>();
    assert_send_sync::<PublishApplicationAuthoringProfileHandler>();

    let first = ApplicationAuthoringProfile::create(
        OrganizationId::from_uuid(Uuid::from_u128(1)),
        ProjectId::from_uuid(Uuid::from_u128(2)),
        ApplicationId::from_uuid(Uuid::from_u128(3)),
        9,
        ApplicationExperience::NewAgent,
        ApplicationAudience::ProjectMembers,
        delivery(ApplicationExperience::NewAgent),
        agent_target('c'),
        digest('c'),
        Utc.with_ymd_and_hms(2026, 9, 14, 12, 0, 0)
            .single()
            .expect("ts"),
    )
    .expect("profile");
    let second = ApplicationAuthoringProfile::create(
        OrganizationId::from_uuid(Uuid::from_u128(1)),
        ProjectId::from_uuid(Uuid::from_u128(2)),
        ApplicationId::from_uuid(Uuid::from_u128(3)),
        9,
        ApplicationExperience::NewAgent,
        ApplicationAudience::ProjectMembers,
        delivery(ApplicationExperience::NewAgent),
        agent_target('c'),
        digest('c'),
        Utc.with_ymd_and_hms(2026, 9, 14, 15, 0, 0)
            .single()
            .expect("ts"),
    )
    .expect("profile");
    assert_eq!(first.id, second.id);
}

fn compiler(
    projects: Arc<InMemoryProjectsRepository>,
    workflows: Arc<InMemoryWorkflowDefinitionRepository>,
) -> WorkflowApplicationPresetCompiler {
    let publications: Arc<dyn IWorkflowDefinitionPublicationPort> =
        Arc::new(WorkflowDefinitionPublicationService::new(
            Arc::new(ProjectsWorkflowProjectAccessAdapter::new(projects)),
            workflows,
        ));
    WorkflowApplicationPresetCompiler::new(publications)
}

async fn seed_project(
    projects: &Arc<InMemoryProjectsRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
) {
    let project = Project::create(
        organization_id,
        project_id,
        ProjectName::parse("authoring-profiles").expect("project name"),
        Utc.with_ymd_and_hms(2026, 9, 14, 7, 0, 0)
            .single()
            .expect("timestamp"),
    );
    IProjectRepository::create(
        projects.as_ref(),
        project.clone(),
        ProjectCreated::envelope(&project, Uuid::now_v7()).expect("project event"),
        IdempotencyRequest::new(
            "application-authoring-tests/projects",
            project_id.to_string(),
            project_id.as_uuid().as_bytes(),
        )
        .expect("project idempotency"),
    )
    .await
    .expect("seed project");
}

fn delivery(experience: ApplicationExperience) -> ApplicationDeliveryPolicy {
    ApplicationDeliveryPolicy {
        interaction_mode: experience.interaction_mode(),
        response_modes: vec![ApplicationResponseMode::Blocking],
    }
}

fn model_target(marker: char) -> ApplicationPresetTarget {
    ApplicationPresetTarget::ModelRevision(ApplicationPresetModelRevision {
        model_id: Uuid::now_v7(),
        revision: format!("model-{marker}"),
        digest: digest(marker),
        capability: "model.invoke".into(),
    })
}

fn agent_target(marker: char) -> ApplicationPresetTarget {
    ApplicationPresetTarget::AgentRelease(ApplicationPresetAgentRelease {
        asset_id: AssetId::from_uuid(Uuid::from_u128(0x22)),
        asset_release_id: AssetReleaseId::from_uuid(Uuid::from_u128(0x33)),
        digest: digest(marker),
        capability: "agent.execute".into(),
    })
}

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
