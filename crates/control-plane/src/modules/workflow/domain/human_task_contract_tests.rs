use super::{
    FlowResumePayload, FlowResumeReceipt, HumanTaskInteractionSpec, HumanTaskRecord,
    HumanTaskStatus, WorkflowDecision, WorkflowDecisionOutcome,
};
use crate::modules::shared_kernel::domain::{PrincipalId, WorkflowDecisionId};
use crate::modules::workflow::test_support::{
    accepted_submission, authorization_reference, claimed_task, pending_task, timestamp,
};
use a3s_form_core::parse_json;
use serde_json::json;
use uuid::Uuid;

#[test]
fn enforces_activation_claim_release_and_optimistic_versions() {
    let (mut task, principal_id) = pending_task();
    let other_principal = PrincipalId::new();

    assert_eq!(task.status, HumanTaskStatus::PendingActivation);
    assert!(task.claim(1, principal_id, timestamp(8, 1)).is_err());
    task.activate(1, timestamp(8, 1)).expect("activation");
    assert_eq!(task.status, HumanTaskStatus::Ready);
    assert_eq!(task.aggregate_version, 2);

    assert!(task.claim(1, principal_id, timestamp(8, 2)).is_err());
    task.claim(2, principal_id, timestamp(8, 2)).expect("claim");
    assert_eq!(task.status, HumanTaskStatus::Claimed);
    assert_eq!(task.claimed_by, Some(principal_id));
    assert_eq!(task.aggregate_version, 3);

    assert!(task.release(3, other_principal, timestamp(8, 3)).is_err());
    assert_eq!(task.aggregate_version, 3);
    task.release(3, principal_id, timestamp(8, 3))
        .expect("claim release");
    assert_eq!(task.status, HumanTaskStatus::Ready);
    assert_eq!(task.claimed_by, None);
    assert_eq!(task.aggregate_version, 4);
    task.validate().expect("task should remain valid");
}

#[test]
fn binds_the_form_request_to_the_exact_claim_generation() {
    let (task, principal_id) = pending_task();
    let interaction = HumanTaskInteractionSpec::approval(
        "Approve this change?",
        Some("Review the exact proposed values.".into()),
        Some(parse_json(br#"{"approved":false,"note":""}"#).expect("initial value")),
    )
    .expect("interaction spec");
    let mut record =
        HumanTaskRecord::create(task, interaction, 7, Uuid::now_v7()).expect("HumanTask record");

    record.activate(1, timestamp(8, 1)).expect("activation");
    record
        .claim(2, principal_id, timestamp(8, 2))
        .expect("claim");
    let request = record
        .interaction_request
        .clone()
        .expect("claim should create request");
    assert_eq!(request.task.version, 3);
    assert_eq!(
        request.assignment.claimed_principal_id,
        principal_id.to_string()
    );
    record.validate().expect("claimed record");

    let mut drifted = record.clone();
    drifted
        .interaction_request
        .as_mut()
        .expect("request")
        .max_value_bytes += 1;
    assert!(drifted.validate().is_err());

    record
        .release(3, principal_id, timestamp(8, 3))
        .expect("release");
    assert!(record.interaction_request.is_none());
    record
        .claim(4, principal_id, timestamp(8, 4))
        .expect("second claim");
    assert_eq!(
        record
            .interaction_request
            .as_ref()
            .expect("second request")
            .task
            .version,
        5
    );
}

#[test]
fn commits_one_submission_decision_and_terminal_task() {
    let (mut task, principal_id) = claimed_task();
    let submission = accepted_submission(&task, principal_id);
    let output = submission.accepted_output().expect("accepted output");
    let decision = WorkflowDecision::from_submission(
        WorkflowDecisionId::new(),
        &task,
        &submission,
        output,
        timestamp(8, 31),
    )
    .expect("decision");

    assert_eq!(decision.outcome, WorkflowDecisionOutcome::Approve);
    assert_eq!(decision.task_version, 3);
    decision.validate().expect("decision should validate");
    task.complete(3, &decision).expect("task completion");
    assert_eq!(task.status, HumanTaskStatus::Completed);
    assert_eq!(task.aggregate_version, 4);
    assert_eq!(task.decision_id, Some(decision.id));
    assert!(task.complete(4, &decision).is_err());
}

#[test]
fn rejects_corrupted_task_decision_and_resume_state() {
    let (task, principal_id) = claimed_task();
    let submission = accepted_submission(&task, principal_id);
    let decision = WorkflowDecision::from_submission(
        WorkflowDecisionId::new(),
        &task,
        &submission,
        submission.accepted_output().expect("accepted output"),
        timestamp(8, 31),
    )
    .expect("decision");
    let payload = FlowResumePayload::from_decision(&decision).expect("resume payload");

    let mut invalid_task = task.clone();
    invalid_task.decision_id = Some(decision.id);
    assert!(invalid_task.validate().is_err());

    let mut changed_decision = decision;
    changed_decision.canonical_output = r#"{"approved":false}"#.into();
    assert!(changed_decision.validate().is_err());

    let mut changed_payload = payload;
    changed_payload.flow_hook_id.clear();
    assert!(changed_payload.validate().is_err());
}

#[test]
fn records_expiry_and_cancellation_as_bound_decisions() {
    let (mut expiring, principal_id) = pending_task();
    expiring.activate(1, timestamp(8, 1)).expect("activation");
    assert!(WorkflowDecision::expire(
        WorkflowDecisionId::new(),
        &expiring,
        principal_id,
        authorization_reference(),
        timestamp(9, 59),
    )
    .is_err());
    let expired = WorkflowDecision::expire(
        WorkflowDecisionId::new(),
        &expiring,
        principal_id,
        authorization_reference(),
        timestamp(10, 0),
    )
    .expect("expiry decision");
    expiring.expire(2, &expired).expect("task expiry");
    assert_eq!(expiring.status, HumanTaskStatus::Expired);

    let (mut cancelled, principal_id) = pending_task();
    let cancellation = WorkflowDecision::cancel(
        WorkflowDecisionId::new(),
        &cancelled,
        principal_id,
        authorization_reference(),
        timestamp(8, 1),
    )
    .expect("cancellation decision");
    cancelled
        .cancel(1, &cancellation)
        .expect("task cancellation");
    assert_eq!(cancelled.status, HumanTaskStatus::Cancelled);
}

#[test]
fn derives_a_resume_receipt_only_from_matching_hook_evidence() {
    let (task, principal_id) = claimed_task();
    let submission = accepted_submission(&task, principal_id);
    let decision = WorkflowDecision::from_submission(
        WorkflowDecisionId::new(),
        &task,
        &submission,
        submission.accepted_output().expect("accepted output"),
        timestamp(8, 31),
    )
    .expect("decision");
    let payload = FlowResumePayload::from_decision(&decision).expect("resume payload");
    let flow_value = payload.to_flow_value().expect("Flow payload");
    let event_id = Uuid::now_v7();

    let receipt = FlowResumeReceipt::from_hook_received(
        &payload,
        &task.flow_run_id,
        &task.flow_hook_id,
        &flow_value,
        11,
        event_id,
        timestamp(8, 32),
    )
    .expect("matching receipt should be observed");
    assert_eq!(receipt.hook_event_sequence, 11);
    assert_eq!(receipt.payload_digest, payload.digest);

    assert!(FlowResumeReceipt::from_hook_received(
        &payload,
        &task.flow_run_id,
        &task.flow_hook_id,
        &json!({"different": true}),
        11,
        event_id,
        timestamp(8, 32),
    )
    .is_err());
    assert!(FlowResumeReceipt::from_hook_received(
        &payload,
        &task.flow_run_id,
        "other-hook",
        &flow_value,
        11,
        event_id,
        timestamp(8, 32),
    )
    .is_err());
}
