use super::{
    ApplicationPresetAgentRelease, ApplicationPresetModelRevision, ApplicationPresetTarget,
    ApplicationPresetWorkflowRequest, CompileApplicationPresetWorkflow,
    CompileApplicationPresetWorkflowHandler, IApplicationPresetWorkflowPort,
};
use crate::modules::applications::domain::ApplicationExperience;
use crate::modules::applications::infrastructure::WorkflowApplicationPresetCompiler;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
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
    CapabilityType, IWorkflowDefinitionPublicationPort, IWorkflowDefinitionRepository,
    InMemoryWorkflowDefinitionRepository, WorkflowDefinitionPublicationService, WorkflowStepKind,
    WorkflowStepOwner,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use chrono::{TimeZone, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn all_four_presets_compile_to_stable_exact_workflow_revisions() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let actor = PrincipalId::new();
    let first_projects = Arc::new(InMemoryProjectsRepository::new());
    let second_projects = Arc::new(InMemoryProjectsRepository::new());
    seed_project(&first_projects, organization_id, project_id).await;
    seed_project(&second_projects, organization_id, project_id).await;
    let first_workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let second_workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let first = compiler(first_projects, first_workflows.clone());
    let second = compiler(second_projects, second_workflows.clone());
    let cases = [
        (
            ApplicationExperience::Chatbot,
            model_target('1'),
            WorkflowStepKind::Model,
            WorkflowStepOwner::Inference,
            CapabilityType::ModelRevision,
            "model.llm",
        ),
        (
            ApplicationExperience::TextGenerator,
            model_target('2'),
            WorkflowStepKind::Model,
            WorkflowStepOwner::Inference,
            CapabilityType::ModelRevision,
            "model.llm",
        ),
        (
            ApplicationExperience::ClassicAgent,
            agent_target('3'),
            WorkflowStepKind::Agent,
            WorkflowStepOwner::Agents,
            CapabilityType::AgentRelease,
            "agent.classic",
        ),
        (
            ApplicationExperience::NewAgent,
            agent_target('4'),
            WorkflowStepKind::Agent,
            WorkflowStepOwner::Agents,
            CapabilityType::AgentRelease,
            "agent.release",
        ),
    ];
    let mut workflow_digests = BTreeSet::new();

    for (experience, target, kind, owner, capability_type, semantic_profile) in cases {
        let request = ApplicationPresetWorkflowRequest {
            organization_id,
            project_id,
            application_id: ApplicationId::new(),
            application_release_number: 1,
            experience,
            target,
            actor_principal_id: actor,
            idempotency_key: format!("compile-{}", experience.as_str()),
            request_id: Uuid::now_v7(),
        };
        let compiled = first
            .compile_and_publish(&request)
            .await
            .expect("compile preset");
        assert!(!compiled.replayed);
        assert_eq!(
            compiled.evidence.binding.workflow_definition_id,
            request.workflow_definition_id()
        );
        assert_eq!(
            compiled.evidence.binding.workflow_revision_id,
            request.workflow_revision_id()
        );
        let replay = first
            .compile_and_publish(&request)
            .await
            .expect("replay preset");
        assert!(replay.replayed);
        assert_eq!(replay.evidence, compiled.evidence);

        let mut restarted_request = request.clone();
        restarted_request.request_id = Uuid::now_v7();
        let restarted = second
            .compile_and_publish(&restarted_request)
            .await
            .expect("compile in independent process state");
        assert!(!restarted.replayed);
        assert_eq!(restarted.evidence, compiled.evidence);

        let first_revision = first_workflows
            .find_revision(
                organization_id,
                request.workflow_definition_id(),
                request.workflow_revision_id(),
            )
            .await
            .expect("read first Workflow")
            .expect("first Workflow revision");
        let second_revision = second_workflows
            .find_revision(
                organization_id,
                request.workflow_definition_id(),
                request.workflow_revision_id(),
            )
            .await
            .expect("read restarted Workflow")
            .expect("restarted Workflow revision");
        first_revision.validate().expect("valid preset revision");
        assert_eq!(first_revision.contract, second_revision.contract);
        assert_eq!(first_revision.payloads, second_revision.payloads);
        assert_eq!(
            first_revision.semantic_contracts,
            second_revision.semantic_contracts
        );
        assert_eq!(first_revision.contract.spec().steps.len(), 3);
        assert_eq!(first_revision.contract.spec().edges.len(), 2);
        let target_step = first_revision
            .contract
            .spec()
            .steps
            .iter()
            .find(|step| step.id == "invoke")
            .expect("target step");
        assert_eq!(target_step.kind, kind);
        assert_eq!(
            target_step
                .capability
                .as_ref()
                .expect("exact target")
                .capability_type,
            capability_type
        );
        let semantics = first_revision
            .semantic_contracts
            .as_ref()
            .expect("semantic contracts");
        let target_binding = semantics
            .descriptor_bindings()
            .resolve("invoke")
            .expect("target descriptor binding");
        let descriptor = semantics
            .descriptor_registry()
            .resolve(
                &target_binding.descriptor_id,
                &target_binding.descriptor_revision,
            )
            .expect("target descriptor");
        assert_eq!(descriptor.spec().owner, owner);
        assert_eq!(descriptor.spec().semantic_profile, semantic_profile);
        assert_eq!(
            descriptor.spec().allowed_capability_types,
            [capability_type]
        );
        workflow_digests.insert(first_revision.contract.digest().clone());
    }

    assert_eq!(workflow_digests.len(), 4);
    assert_eq!(first_workflows.outbox_events().await.len(), 4);
    assert_eq!(second_workflows.outbox_events().await.len(), 4);
}

#[tokio::test]
async fn preset_identity_and_idempotency_reject_target_drift() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    seed_project(&projects, organization_id, project_id).await;
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let compiler = compiler(projects, workflows.clone());
    let request = ApplicationPresetWorkflowRequest {
        organization_id,
        project_id,
        application_id: ApplicationId::new(),
        application_release_number: 7,
        experience: ApplicationExperience::Chatbot,
        target: model_target('5'),
        actor_principal_id: PrincipalId::new(),
        idempotency_key: "release-seven".into(),
        request_id: Uuid::now_v7(),
    };
    let original = compiler
        .compile_and_publish(&request)
        .await
        .expect("compile original");

    let mut drifted = request.clone();
    drifted.target = model_target('6');
    let idempotency_conflict = compiler
        .compile_and_publish(&drifted)
        .await
        .expect_err("changed target cannot replay");
    assert!(matches!(
        idempotency_conflict,
        ApplicationError::Conflict(_)
    ));

    drifted.idempotency_key = "different-key".into();
    let identity_conflict = compiler
        .compile_and_publish(&drifted)
        .await
        .expect_err("release slot cannot fork under another key");
    assert!(matches!(identity_conflict, ApplicationError::Conflict(_)));
    let retained = workflows
        .find_revision(
            organization_id,
            request.workflow_definition_id(),
            request.workflow_revision_id(),
        )
        .await
        .expect("read retained Workflow")
        .expect("retained Workflow");
    assert_eq!(
        retained.contract.digest(),
        &original.evidence.binding.workflow_contract_digest
    );
    assert_eq!(workflows.outbox_events().await.len(), 1);
}

#[tokio::test]
async fn command_authorizes_before_compilation_and_user_authored_modes_fail_closed() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let projects = Arc::new(InMemoryProjectsRepository::new());
    seed_project(&projects, organization_id, project_id).await;
    let workflows = Arc::new(InMemoryWorkflowDefinitionRepository::new());
    let port: Arc<dyn IApplicationPresetWorkflowPort> =
        Arc::new(WorkflowApplicationPresetCompiler::new(Arc::new(
            WorkflowDefinitionPublicationService::new(projects, workflows.clone()),
        )));
    let handler = CompileApplicationPresetWorkflowHandler::new(port);
    let command = CompileApplicationPresetWorkflow {
        organization_id,
        project_id,
        application_id: ApplicationId::new(),
        application_release_number: 1,
        experience: ApplicationExperience::NewAgent,
        target: agent_target('7'),
        actor_principal_id: PrincipalId::new(),
        resource_access: ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id,
            environment_id: EnvironmentId::new(),
        }]),
        idempotency_key: "new-agent".into(),
        request_id: Uuid::now_v7(),
    };
    let denied = handler
        .execute(command.clone(), context())
        .await
        .expect("command framework");
    assert_eq!(
        denied,
        Err(ApplicationError::NotFound(
            "Application project not found".into()
        ))
    );
    assert!(workflows
        .list(organization_id, project_id)
        .await
        .expect("list Workflows")
        .is_empty());

    let authored = handler
        .execute(
            CompileApplicationPresetWorkflow {
                experience: ApplicationExperience::Chatflow,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                ..command
            },
            context(),
        )
        .await
        .expect("command framework");
    assert!(matches!(authored, Err(ApplicationError::Invalid(_))));
    assert!(workflows
        .list(organization_id, project_id)
        .await
        .expect("list Workflows")
        .is_empty());
}

#[test]
fn preset_contract_types_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ApplicationPresetWorkflowRequest>();
    assert_send_sync::<super::ApplicationPresetWorkflowResult>();
    assert_send_sync::<CompileApplicationPresetWorkflow>();
    assert_send_sync::<CompileApplicationPresetWorkflowHandler>();
    assert_send_sync::<WorkflowApplicationPresetCompiler>();
}

fn compiler(
    projects: Arc<InMemoryProjectsRepository>,
    workflows: Arc<InMemoryWorkflowDefinitionRepository>,
) -> WorkflowApplicationPresetCompiler {
    let publications: Arc<dyn IWorkflowDefinitionPublicationPort> = Arc::new(
        WorkflowDefinitionPublicationService::new(projects, workflows),
    );
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
        ProjectName::parse("preset-compilers").expect("project name"),
        Utc.with_ymd_and_hms(2026, 8, 21, 7, 0, 0)
            .single()
            .expect("timestamp"),
    );
    IProjectRepository::create(
        projects.as_ref(),
        project.clone(),
        ProjectCreated::envelope(&project, Uuid::now_v7()).expect("project event"),
        IdempotencyRequest::new(
            "application-preset-tests/projects",
            project_id.to_string(),
            project_id.as_uuid().as_bytes(),
        )
        .expect("project idempotency"),
    )
    .await
    .expect("seed project");
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
        asset_id: AssetId::new(),
        asset_release_id: AssetReleaseId::new(),
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
