use super::{
    AcceptedHumanTaskSubmission, HumanTaskSubmission, HUMAN_TASK_SUBMISSION_MAX_VALUE_BYTES,
};
use crate::modules::shared_kernel::domain::{
    AuthorizationDecisionRef, FormSubmissionId, HumanTaskId, OrganizationId, PrincipalId,
    ProjectId, Sha256Digest, WorkflowRunId,
};
use a3s_form_core::{
    canonicalize_interaction_request_content, canonicalize_interaction_value,
    digest_interaction_request, digest_interaction_value, parse_json, FormInteractionAssignment,
    FormInteractionOutcome, FormInteractionOutputMapping, FormInteractionRequest,
    FormInteractionSubmission, FormInteractionSubmissionAssignment, FormInteractionTaskBinding,
    FormReleaseMode, FormReleaseRef, WorkflowInteractionIdentity,
    DEFAULT_INTERACTION_MAX_VALUE_BYTES, FORM_INTERACTION_REQUEST_API_VERSION,
    FORM_INTERACTION_SUBMISSION_API_VERSION, FORM_RELEASE_REF_API_VERSION,
};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;

struct SubmissionFixture {
    input: AcceptedHumanTaskSubmission,
    expected_output: String,
    expected_output_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InteractionConformanceFixture {
    api_version: String,
    request_content_canonical: String,
    request_digest: String,
    value_canonical: String,
    value_digest: String,
    request: FormInteractionRequest,
    submission: FormInteractionSubmission,
}

#[test]
fn consumes_the_owner_form_interaction_golden_fixture() {
    let fixture: InteractionConformanceFixture = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/form-interaction-contract-v1.json"
    )))
    .expect("shared Form interaction fixture should decode");

    assert_eq!(
        fixture.api_version,
        "a3s.dev/form-interaction-conformance/v1"
    );
    fixture
        .request
        .form
        .validate()
        .expect("standalone FormReleaseRef");
    fixture.request.validate().expect("interaction request");
    fixture
        .submission
        .validate()
        .expect("interaction submission");
    assert_eq!(
        canonicalize_interaction_request_content(&fixture.request)
            .expect("canonical request content"),
        fixture.request_content_canonical.as_bytes()
    );
    assert_eq!(
        digest_interaction_request(&fixture.request).expect("request digest"),
        fixture.request_digest
    );
    assert_eq!(
        canonicalize_interaction_value(&fixture.submission.value).expect("canonical value"),
        fixture.value_canonical.as_bytes()
    );
    assert_eq!(
        digest_interaction_value(&fixture.submission.value).expect("value digest"),
        fixture.value_digest
    );
}

#[test]
fn accepts_a_request_bound_immutable_submission() {
    let fixture = submission_fixture();
    let submission =
        HumanTaskSubmission::accept(fixture.input).expect("submission should be accepted");

    assert_eq!(submission.canonical_output, fixture.expected_output);
    assert_eq!(
        submission.output_digest.as_str(),
        fixture.expected_output_digest
    );
    assert_eq!(submission.aggregate_version, 1);
    assert_eq!(submission.task_expires_at, Some(timestamp(10, 0)));
    submission
        .validate()
        .expect("stored submission should validate");
}

#[test]
fn rejects_corrupted_persisted_submission_invariants() {
    let fixture = submission_fixture();
    let submission =
        HumanTaskSubmission::accept(fixture.input).expect("submission should be accepted");

    let mut expired = submission.clone();
    expired.task_expires_at = Some(expired.accepted_at);
    assert!(expired.validate().is_err());

    let mut invalid_policy = submission.clone();
    invalid_policy.assignment_policy_revision = 0;
    assert!(invalid_policy.validate().is_err());

    let mut changed_output = submission;
    changed_output.canonical_output = r#"{"approved":false}"#.into();
    assert!(changed_output.validate().is_err());
}

#[test]
fn rejects_cross_request_identity_expiry_and_output_drift() {
    let mut fixture = submission_fixture();
    fixture.input.submission.request_digest = digest('0');
    assert!(HumanTaskSubmission::accept(fixture.input).is_err());

    let mut fixture = submission_fixture();
    fixture.input.workflow_run_id = WorkflowRunId::new();
    assert!(HumanTaskSubmission::accept(fixture.input).is_err());

    let mut fixture = submission_fixture();
    fixture.input.accepted_at = timestamp(10, 0);
    assert!(HumanTaskSubmission::accept(fixture.input).is_err());

    let mut fixture = submission_fixture();
    fixture.input.request.max_value_bytes = 8;
    fixture.input.request.digest = digest_interaction_request(&fixture.input.request)
        .expect("request digest should be recomputed");
    fixture.input.submission.request_digest = fixture.input.request.digest.clone();
    assert!(HumanTaskSubmission::accept(fixture.input).is_err());

    let mut fixture = submission_fixture();
    fixture.input.request.max_value_bytes = HUMAN_TASK_SUBMISSION_MAX_VALUE_BYTES + 1;
    fixture.input.request.digest = digest_interaction_request(&fixture.input.request)
        .expect("request digest should be recomputed");
    fixture.input.submission.request_digest = fixture.input.request.digest.clone();
    assert!(HumanTaskSubmission::accept(fixture.input).is_err());
}

fn submission_fixture() -> SubmissionFixture {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let workflow_run_id = WorkflowRunId::new();
    let human_task_id = HumanTaskId::new();
    let form_submission_id = FormSubmissionId::new();
    let principal_id = PrincipalId::new();
    let identity = WorkflowInteractionIdentity {
        workflow_run_id: workflow_run_id.to_string(),
        flow_run_id: format!("flow-{workflow_run_id}"),
        step_id: "human_review".into(),
        step_attempt: 2,
        human_task_id: human_task_id.to_string(),
        flow_hook_id: "human_review-2".into(),
    };
    let form = FormReleaseRef {
        api_version: FORM_RELEASE_REF_API_VERSION.into(),
        organization_id: organization_id.to_string(),
        project_id: project_id.to_string(),
        form_id: "approval-form".into(),
        release_id: "approval-form-r3".into(),
        uri: "a3s://forms/approval-form/releases/approval-form-r3".into(),
        revision: 3,
        digest: digest('a'),
        compiler_revision: "a3s-form-core@0.1.0".into(),
        schema_profile: "a3s.dev/form-schema-profile/1".into(),
        mode: FormReleaseMode::Interaction,
    };
    let mut request = FormInteractionRequest {
        api_version: FORM_INTERACTION_REQUEST_API_VERSION.into(),
        request_id: "request-human-review-2".into(),
        identity: identity.clone(),
        form: form.clone(),
        assignment: FormInteractionAssignment {
            policy_id: "approval-policy".into(),
            policy_revision: 4,
            policy_digest: digest('b'),
            claimed_principal_id: principal_id.to_string(),
        },
        task: FormInteractionTaskBinding {
            version: 7,
            created_at: form_timestamp(8, 0),
            due_at: Some(form_timestamp(9, 0)),
            expires_at: Some(form_timestamp(10, 0)),
        },
        allowed_outcomes: vec![
            FormInteractionOutcome::Approve,
            FormInteractionOutcome::Reject,
        ],
        output_mapping: FormInteractionOutputMapping::Identity,
        max_value_bytes: DEFAULT_INTERACTION_MAX_VALUE_BYTES,
        initial_value: None,
        digest: digest('0'),
    };
    request.digest = digest_interaction_request(&request).expect("request should hash");
    let accepted_value = parse_json(br#"{"approved":true,"note":"accepted"}"#)
        .expect("accepted output should parse");
    let output_digest =
        digest_interaction_value(&accepted_value).expect("accepted output should hash");
    let mut submission = FormInteractionSubmission {
        api_version: FORM_INTERACTION_SUBMISSION_API_VERSION.into(),
        submission_id: form_submission_id.to_string(),
        request_id: request.request_id.clone(),
        request_digest: request.digest.clone(),
        identity,
        form,
        assignment: FormInteractionSubmissionAssignment {
            policy_id: request.assignment.policy_id.clone(),
            policy_revision: request.assignment.policy_revision,
            policy_digest: request.assignment.policy_digest.clone(),
        },
        task_version: request.task.version,
        principal_id: principal_id.to_string(),
        outcome: FormInteractionOutcome::Approve,
        idempotency_key: "approve-human-review-2".into(),
        submitted_at: form_timestamp(8, 29),
        value: accepted_value.clone(),
        value_digest: digest('0'),
    };
    submission.value_digest =
        digest_interaction_value(&submission.value).expect("candidate value should hash");
    SubmissionFixture {
        input: AcceptedHumanTaskSubmission {
            organization_id,
            project_id,
            id: form_submission_id,
            workflow_run_id,
            human_task_id,
            principal_id,
            authorization_decision: AuthorizationDecisionRef::new(
                "authorization-human-review-2",
                Sha256Digest::parse(digest('d')).expect("authorization digest"),
            )
            .expect("authorization reference"),
            request,
            submission,
            accepted_value,
            accepted_at: timestamp(8, 30),
        },
        expected_output: r#"{"approved":true,"note":"accepted"}"#.into(),
        expected_output_digest: output_digest,
    }
}

fn timestamp(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 9, hour, minute, 0)
        .single()
        .expect("timestamp")
}

fn form_timestamp(hour: u32, minute: u32) -> String {
    timestamp(hour, minute).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
