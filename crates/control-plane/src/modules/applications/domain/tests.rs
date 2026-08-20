use super::*;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, OrganizationId, PrincipalId, ProjectId, ResourceName,
    Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn binding() -> ApplicationWorkflowBinding {
    ApplicationWorkflowBinding {
        workflow_definition_id: WorkflowDefinitionId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000101").expect("UUID"),
        ),
        workflow_revision_id: WorkflowRevisionId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000102").expect("UUID"),
        ),
        workflow_contract_digest: digest('a'),
        workflow_payload_set_digest: digest('b'),
        workflow_semantic_contract_set_digest: digest('c'),
        input_schema_digest: digest('d'),
        output_schema_digest: digest('e'),
    }
}

fn contract(experience: ApplicationExperience, marker: char) -> ApplicationReleaseContract {
    ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: experience.interaction_mode(),
            response_modes: vec![
                ApplicationResponseMode::Streaming,
                ApplicationResponseMode::Blocking,
            ],
        },
        workflow: binding(),
        presentation_digest: digest(marker),
    })
    .expect("Application release contract")
}

fn release(contract: ApplicationReleaseContract) -> ApplicationRelease {
    ApplicationRelease::initial(
        OrganizationId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000001").expect("UUID"),
        ),
        ProjectId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000002").expect("UUID"),
        ),
        ApplicationId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000003").expect("UUID"),
        ),
        ApplicationReleaseId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000004").expect("UUID"),
        ),
        contract,
        PrincipalId::from_uuid(
            Uuid::parse_str("018f0000-0000-7000-8000-000000000005").expect("UUID"),
        ),
        Utc.with_ymd_and_hms(2026, 8, 20, 8, 0, 0)
            .single()
            .expect("timestamp"),
    )
    .expect("initial release")
}

#[test]
fn all_six_experiences_have_one_closed_interaction_mapping() {
    for (experience, interaction) in [
        (
            ApplicationExperience::Chatbot,
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::TextGenerator,
            ApplicationInteractionMode::Invocation,
        ),
        (
            ApplicationExperience::ClassicAgent,
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::NewAgent,
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::Chatflow,
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::Workflow,
            ApplicationInteractionMode::Invocation,
        ),
    ] {
        let value = contract(experience, 'f');
        assert_eq!(value.spec().experience, experience);
        assert_eq!(value.spec().delivery.interaction_mode, interaction);
        assert_eq!(
            value.spec().delivery.response_modes,
            vec![
                ApplicationResponseMode::Blocking,
                ApplicationResponseMode::Streaming,
            ]
        );
        assert_eq!(
            ApplicationReleaseContract::parse_acl(value.canonical_acl()).expect("round trip"),
            value
        );
    }
}

#[test]
fn canonical_contract_is_closed_and_digest_bound() {
    let value = contract(ApplicationExperience::Chatflow, 'f');
    let restored =
        ApplicationReleaseContract::restore(value.canonical_acl(), value.digest().as_str())
            .expect("restore");
    assert_eq!(restored, value);

    let windows = value.canonical_acl().replace('\n', "\r\n");
    assert_eq!(
        ApplicationReleaseContract::parse_acl(&windows).expect("normalized line endings"),
        value
    );
    assert!(
        ApplicationReleaseContract::restore(value.canonical_acl(), digest('9').as_str()).is_err()
    );

    let noncanonical = value.canonical_acl().replacen("schema =", "schema  =", 1);
    assert!(ApplicationReleaseContract::parse_acl(&noncanonical).is_err());
    let unknown = value.canonical_acl().replacen(
        "schema = \"cloud.application.release.v1\"",
        "schema = \"cloud.application.release.v1\"\n  copied_state = \"forbidden\"",
        1,
    );
    assert!(ApplicationReleaseContract::parse_acl(&unknown).is_err());
}

#[test]
fn checked_in_release_contract_is_byte_stable() {
    let fixture = include_str!("../../../../../../contracts/app0.1/application-release.acl");
    let parsed = ApplicationReleaseContract::parse_acl(fixture).expect("checked-in contract");
    assert_eq!(parsed, contract(ApplicationExperience::Chatflow, 'f'));
    assert_eq!(parsed.canonical_acl(), fixture);
}

#[test]
fn delivery_policy_rejects_mode_drift_and_duplicate_modes() {
    let mut spec = contract(ApplicationExperience::Workflow, 'f')
        .spec()
        .clone();
    spec.delivery.interaction_mode = ApplicationInteractionMode::Conversation;
    assert!(ApplicationReleaseContract::from_spec(spec).is_err());

    let mut spec = contract(ApplicationExperience::Workflow, 'f')
        .spec()
        .clone();
    spec.delivery.response_modes = vec![
        ApplicationResponseMode::Blocking,
        ApplicationResponseMode::Blocking,
    ];
    assert!(ApplicationReleaseContract::from_spec(spec).is_err());

    let mut spec = contract(ApplicationExperience::Workflow, 'f')
        .spec()
        .clone();
    spec.delivery.response_modes.clear();
    assert!(ApplicationReleaseContract::from_spec(spec).is_err());
}

#[test]
fn workflow_admission_requires_exact_scope_identity_and_all_digests() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let value = contract(ApplicationExperience::Workflow, 'f');
    let evidence = ApplicationWorkflowRevisionEvidence {
        organization_id,
        project_id,
        binding: value.spec().workflow.clone(),
    };
    value
        .validate_workflow_evidence(organization_id, project_id, &evidence)
        .expect("exact evidence");

    let mut changed = evidence.clone();
    changed.binding.workflow_payload_set_digest = digest('8');
    assert!(value
        .validate_workflow_evidence(organization_id, project_id, &changed)
        .is_err());
    assert!(value
        .validate_workflow_evidence(organization_id, ProjectId::new(), &evidence)
        .is_err());
}

#[test]
fn release_lineage_keeps_application_experience_immutable() {
    let first = release(contract(ApplicationExperience::ClassicAgent, 'f'));
    let second_contract = contract(ApplicationExperience::ClassicAgent, '9');
    let second = ApplicationRelease::successor(
        &first,
        ApplicationReleaseId::new(),
        second_contract,
        first.created_by,
        first.created_at + chrono::Duration::seconds(1),
    )
    .expect("successor");
    assert_eq!(second.release_number, 2);
    assert_eq!(second.parent_release_id, Some(first.id));
    assert_eq!(second.parent_digest.as_ref(), Some(first.contract.digest()));

    assert!(ApplicationRelease::successor(
        &first,
        ApplicationReleaseId::new(),
        contract(ApplicationExperience::NewAgent, '9'),
        first.created_by,
        first.created_at + chrono::Duration::seconds(1),
    )
    .is_err());
    assert!(ApplicationRelease::successor(
        &first,
        ApplicationReleaseId::new(),
        first.contract.clone(),
        first.created_by,
        first.created_at + chrono::Duration::seconds(1),
    )
    .is_err());
}

#[test]
fn aggregate_advances_only_from_its_exact_current_release() {
    let first = release(contract(ApplicationExperience::Chatflow, 'f'));
    let application = Application::create(
        first.application_id,
        ResourceName::parse("Support Chat").expect("name"),
        "Customer support conversation".into(),
        &first,
    )
    .expect("Application");
    let second = ApplicationRelease::successor(
        &first,
        ApplicationReleaseId::new(),
        contract(ApplicationExperience::Chatflow, '9'),
        first.created_by,
        first.created_at + chrono::Duration::seconds(1),
    )
    .expect("successor");
    let advanced = application.advance(1, &second).expect("advance");
    assert_eq!(advanced.aggregate_version, 2);
    assert_eq!(advanced.current_release_id, second.id);
    assert_eq!(advanced.experience, ApplicationExperience::Chatflow);
    assert_eq!(
        advanced.at_release(&first).expect("historical head").name,
        application.name
    );
    assert!(application.advance(2, &second).is_err());

    let foreign = ApplicationRelease::initial(
        first.organization_id,
        first.project_id,
        ApplicationId::new(),
        ApplicationReleaseId::new(),
        contract(ApplicationExperience::Chatflow, '8'),
        first.created_by,
        first.created_at + chrono::Duration::seconds(1),
    )
    .expect("foreign release");
    assert!(application.advance(1, &foreign).is_err());
}

#[test]
fn component_boundary_contains_no_second_execution_or_provider_authority() {
    let implementation = [
        include_str!("application.rs"),
        include_str!("application_release_contract.rs"),
        include_str!("workflow_binding.rs"),
    ]
    .join("\n");
    for forbidden in [
        "a3s_flow",
        "reqwest",
        "tokio::spawn",
        "modules::workflow",
        "modules::agents",
        "modules::workloads",
        "modules::secrets",
    ] {
        assert!(
            !implementation.contains(forbidden),
            "Applications contract foundation duplicates authority through {forbidden}"
        );
    }

    let acl = contract(ApplicationExperience::NewAgent, 'f');
    for forbidden in [
        "graph",
        "provider_endpoint",
        "secret_material",
        "session_state",
        "flow_history",
        "gateway_route",
    ] {
        assert!(!acl.canonical_acl().contains(forbidden));
    }
}
