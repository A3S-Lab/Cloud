//! APP0.4-C2: Claim path for Chatflow / Workflow via exact user-authored Workflow.
//!
//! Freezes the non-invented contract: CreateApplication and
//! PublishApplicationRelease admit Chatflow and Workflow only through an exact
//! Workflow revision binding (APP0.1 release authority). Preset authoring-profile
//! creation continues to fail closed. Does not invent graph editors or engines.

use super::authoring_profile::ApplicationAuthoringProfile;
use super::commands::{
    CreateApplication, CreateApplicationHandler, PublishApplicationRelease,
    PublishApplicationReleaseHandler,
};
use super::preset_workflow_port::{ApplicationPresetModelRevision, ApplicationPresetTarget};
use super::workflow_revision_port::IApplicationWorkflowRevisionPort;
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationResponseMode, ApplicationWorkflowBinding, ApplicationWorkflowRevisionEvidence,
};
use crate::modules::applications::infrastructure::InMemoryApplicationRepository;
use crate::modules::applications::ApplicationAccess;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, OrganizationId, PrincipalId, ProjectId, Sha256Digest, WorkflowDefinitionId,
    WorkflowRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
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

fn contract_for(
    experience: ApplicationExperience,
    evidence: &ApplicationWorkflowRevisionEvidence,
    presentation: char,
) -> ApplicationReleaseContract {
    ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: experience.interaction_mode(),
            response_modes: vec![ApplicationResponseMode::Blocking],
        },
        workflow: evidence.binding.clone(),
        presentation_digest: digest(presentation),
    })
    .expect("Application release contract")
}

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
async fn chatflow_and_workflow_create_and_publish_via_exact_workflow_binding() {
    for (experience, name) in [
        (ApplicationExperience::Chatflow, "chatflow-app"),
        (ApplicationExperience::Workflow, "workflow-app"),
    ] {
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
        let initial = contract_for(experience, &evidence, '1');
        let created = create_handler
            .execute(
                CreateApplication {
                    organization_id,
                    project_id,
                    name: name.into(),
                    description: "Exact Workflow-backed application mode".into(),
                    release_acl: initial.canonical_acl().into(),
                    actor_principal_id,
                    access: ApplicationAccess::organization_wide(),
                    idempotency_key: format!("create-{name}"),
                    request_id: Uuid::now_v7(),
                },
                context(),
            )
            .await
            .expect("command framework")
            .expect("create Application");
        assert!(!created.replayed);
        assert_eq!(created.record.application.experience, experience);
        assert_eq!(workflow.calls.load(Ordering::SeqCst), 1);

        let publish_handler =
            PublishApplicationReleaseHandler::new(applications.clone(), workflow.clone());
        let second = contract_for(experience, &evidence, '2');
        let published = publish_handler
            .execute(
                PublishApplicationRelease {
                    organization_id,
                    project_id,
                    application_id: created.record.application.id,
                    expected_version: 1,
                    release_acl: second.canonical_acl().into(),
                    actor_principal_id,
                    access: ApplicationAccess::organization_wide(),
                    idempotency_key: format!("publish-{name}"),
                    request_id: Uuid::now_v7(),
                },
                context(),
            )
            .await
            .expect("command framework")
            .expect("publish release");
        assert!(!published.replayed);
        assert_eq!(published.record.application.experience, experience);
        assert_eq!(published.record.application.aggregate_version, 2);
        assert_eq!(workflow.calls.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn chatflow_and_workflow_remain_fail_closed_on_preset_authoring_profile() {
    let target = ApplicationPresetTarget::ModelRevision(ApplicationPresetModelRevision {
        model_id: Uuid::from_u128(7),
        revision: "model-f".into(),
        digest: digest('f'),
        capability: "model.invoke".into(),
    });
    for experience in [
        ApplicationExperience::Chatflow,
        ApplicationExperience::Workflow,
    ] {
        let err = ApplicationAuthoringProfile::create(
            OrganizationId::from_uuid(Uuid::from_u128(1)),
            ProjectId::from_uuid(Uuid::from_u128(2)),
            ApplicationId::from_uuid(Uuid::from_u128(3)),
            1,
            experience,
            ApplicationAudience::ProjectMembers,
            ApplicationDeliveryPolicy {
                interaction_mode: experience.interaction_mode(),
                response_modes: vec![ApplicationResponseMode::Blocking],
            },
            target.clone(),
            digest('a'),
            Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0)
                .single()
                .expect("ts"),
        )
        .expect_err("must fail closed");
        assert!(
            err.contains("Chatflow and Workflow"),
            "{} must fail closed on preset authoring, got {err}",
            experience.as_str()
        );
    }
}

#[test]
fn chatflow_conversation_and_workflow_invocation_modes_stay_frozen() {
    assert_eq!(
        ApplicationExperience::Chatflow.interaction_mode(),
        ApplicationInteractionMode::Conversation
    );
    assert_eq!(
        ApplicationExperience::Workflow.interaction_mode(),
        ApplicationInteractionMode::Invocation
    );
    let evidence = evidence(
        OrganizationId::from_uuid(Uuid::from_u128(1)),
        ProjectId::from_uuid(Uuid::from_u128(2)),
    );
    let chatflow = contract_for(ApplicationExperience::Chatflow, &evidence, '1');
    let workflow = contract_for(ApplicationExperience::Workflow, &evidence, '2');
    assert_eq!(
        chatflow.spec().delivery.interaction_mode,
        ApplicationInteractionMode::Conversation
    );
    assert_eq!(
        workflow.spec().delivery.interaction_mode,
        ApplicationInteractionMode::Invocation
    );
}

