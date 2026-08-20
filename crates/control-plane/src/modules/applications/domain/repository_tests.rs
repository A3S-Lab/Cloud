use super::*;
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, IdempotencyRequest, OrganizationId, PrincipalId,
    ProjectId, ResourceName, Sha256Digest, WorkflowDefinitionId, WorkflowRevisionId,
};
use chrono::{Duration, TimeZone, Utc};
use uuid::Uuid;

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn contract(marker: char) -> ApplicationReleaseContract {
    ApplicationReleaseContract::from_spec(ApplicationReleaseContractSpec {
        experience: ApplicationExperience::Chatflow,
        audience: ApplicationAudience::ProjectMembers,
        delivery: ApplicationDeliveryPolicy {
            interaction_mode: ApplicationInteractionMode::Conversation,
            response_modes: vec![
                ApplicationResponseMode::Streaming,
                ApplicationResponseMode::Blocking,
            ],
        },
        workflow: ApplicationWorkflowBinding {
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
        },
        presentation_digest: digest(marker),
    })
    .expect("Application release contract")
}

fn initial_record() -> ApplicationRecord {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let application_id = ApplicationId::new();
    let actor = PrincipalId::new();
    let created_at = Utc
        .with_ymd_and_hms(2026, 8, 20, 9, 0, 0)
        .single()
        .expect("timestamp");
    let release = ApplicationRelease::initial(
        organization_id,
        project_id,
        application_id,
        ApplicationReleaseId::new(),
        contract('f'),
        actor,
        created_at,
    )
    .expect("initial release");
    let application = Application::create(
        application_id,
        ResourceName::parse("Support application").expect("name"),
        "One exact Application release authority".into(),
        &release,
    )
    .expect("Application");
    ApplicationRecord::new(application, release).expect("record")
}

fn create_write(record: &ApplicationRecord) -> CreateApplicationWrite {
    let request_id = Uuid::now_v7();
    CreateApplicationWrite {
        record: record.clone(),
        event: ApplicationReleasePublished::published(
            &record.application,
            &record.release,
            request_id,
        )
        .expect("event"),
        actor_principal_id: record.application.created_by,
        request_id,
        idempotency: IdempotencyRequest::new(
            "applications",
            "create-application",
            record.release.contract.canonical_acl().as_bytes(),
        )
        .expect("idempotency"),
    }
}

#[test]
fn release_event_and_initial_write_bind_the_exact_record() {
    let record = initial_record();
    let write = create_write(&record);
    write.validate().expect("valid initial write");

    let payload: ApplicationReleasePublished =
        serde_json::from_value(write.event.payload.clone()).expect("payload");
    assert_eq!(payload.project_id, record.application.project_id.as_uuid());
    assert_eq!(payload.application_id, record.application.id.as_uuid());
    assert_eq!(payload.release_id, record.release.id.as_uuid());
    assert_eq!(payload.release_number, 1);
    assert_eq!(payload.experience, "chatflow");
    assert_eq!(
        payload.response_modes,
        vec!["blocking".to_owned(), "streaming".to_owned()]
    );
    assert_eq!(
        payload.workflow_revision_id,
        record
            .release
            .contract
            .spec()
            .workflow
            .workflow_revision_id
            .as_uuid()
    );
}

#[test]
fn write_validation_rejects_actor_event_and_idempotency_drift() {
    let record = initial_record();

    let mut foreign_actor = create_write(&record);
    foreign_actor.actor_principal_id = PrincipalId::new();
    assert!(foreign_actor.validate().is_err());

    let mut changed_event = create_write(&record);
    changed_event.event.aggregate_version = 2;
    assert!(changed_event.validate().is_err());

    let mut changed_payload = create_write(&record);
    changed_payload.event.payload["workflowRevisionId"] =
        serde_json::json!(WorkflowRevisionId::new());
    assert!(changed_payload.validate().is_err());

    let mut extended_payload = create_write(&record);
    extended_payload.event.payload["foreignAuthority"] = serde_json::json!(true);
    assert!(extended_payload.validate().is_err());

    let mut invalid_idempotency = create_write(&record);
    invalid_idempotency.idempotency.key.clear();
    assert!(invalid_idempotency.validate().is_err());
}

#[test]
fn publish_write_requires_the_exact_locked_parent() {
    let initial = initial_record();
    let successor = ApplicationRelease::successor(
        &initial.release,
        ApplicationReleaseId::new(),
        contract('9'),
        initial.application.created_by,
        initial.release.created_at + Duration::seconds(1),
    )
    .expect("successor");
    let application = initial.application.advance(1, &successor).expect("advance");
    let record = ApplicationRecord::new(application, successor).expect("record");
    let request_id = Uuid::now_v7();
    let write = PublishApplicationReleaseWrite {
        event: ApplicationReleasePublished::published(
            &record.application,
            &record.release,
            request_id,
        )
        .expect("event"),
        actor_principal_id: record.release.created_by,
        request_id,
        expected_version: 1,
        idempotency: IdempotencyRequest::new(
            "application-releases",
            "publish-release",
            record.release.contract.canonical_acl().as_bytes(),
        )
        .expect("idempotency"),
        record: record.clone(),
    };
    write
        .validate_against(&initial.application)
        .expect("exact parent");

    let foreign = initial
        .application
        .at_release(&initial.release)
        .expect("head");
    let mut stale = write.clone();
    stale.expected_version = 2;
    assert!(stale.validate_against(&foreign).is_err());
}

#[test]
fn record_rejects_a_release_that_is_not_the_current_head() {
    let initial = initial_record();
    let successor = ApplicationRelease::successor(
        &initial.release,
        ApplicationReleaseId::new(),
        contract('9'),
        initial.application.created_by,
        initial.release.created_at + Duration::seconds(1),
    )
    .expect("successor");
    assert!(ApplicationRecord::new(initial.application, successor).is_err());
}
