use crate::modules::forms::domain::{AcceptedFormSubmission, FormSubmission};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, AuthorizationDecisionRef, FormSubmissionId, HumanTaskId,
    OntologyId, OntologyRevisionId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    Sha256Digest, WorkflowDefinitionId, WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use crate::modules::workflow::domain::entities::digest_payload_set;
use crate::modules::workflow::domain::{
    AssignmentPolicyRef, HumanTask, NewHumanTask, ResolvedWorkflowPayload, WorkflowBranchRoute,
    WorkflowDataSchema, WorkflowDataType, WorkflowEdgeSpec, WorkflowPayload,
    WorkflowPayloadContent, WorkflowPlan, WorkflowPlanStep, WorkflowRunInput,
    WorkflowStepConfiguration, WorkflowStepKind, WORKFLOW_PLAN_COMPILER_REVISION,
    WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_PLAN_SCHEMA, WORKFLOW_RUN_FLOW_NAME,
    WORKFLOW_RUN_FLOW_VERSION, WORKFLOW_RUN_INPUT_SCHEMA, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
};
use a3s_form_core::{
    digest_interaction_request, digest_interaction_value, parse_json, FormInteractionAssignment,
    FormInteractionOutcome, FormInteractionOutputMapping, FormInteractionRequest,
    FormInteractionSubmission, FormInteractionSubmissionAssignment, FormInteractionTaskBinding,
    FormReleaseMode, FormReleaseRef, WorkflowInteractionIdentity,
    FORM_INTERACTION_REQUEST_API_VERSION, FORM_INTERACTION_SUBMISSION_API_VERSION,
    FORM_RELEASE_REF_API_VERSION,
};
use chrono::{DateTime, TimeZone, Utc};

pub(crate) const TEST_HOOK_ID: &str = "human_review-2";

pub(crate) fn pending_task() -> (HumanTask, PrincipalId) {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let workflow_run_id = WorkflowRunId::new();
    let principal_id = PrincipalId::new();
    let task = HumanTask::create(NewHumanTask {
        organization_id,
        project_id,
        id: HumanTaskId::new(),
        workflow_run_id,
        step_id: "human_review".into(),
        step_attempt: 2,
        form_release: form_release(organization_id, project_id),
        assignment_policy: AssignmentPolicyRef::new(
            "approval-policy",
            4,
            Sha256Digest::parse(digest('b')).expect("policy digest"),
        )
        .expect("assignment policy"),
        flow_run_id: format!("flow-{workflow_run_id}"),
        flow_hook_id: TEST_HOOK_ID.into(),
        due_at: Some(timestamp(9, 0)),
        expires_at: Some(timestamp(10, 0)),
        created_at: timestamp(8, 0),
    })
    .expect("pending task");
    (task, principal_id)
}

pub(crate) fn claimed_task() -> (HumanTask, PrincipalId) {
    let (mut task, principal_id) = pending_task();
    task.activate(1, timestamp(8, 1)).expect("activation");
    task.claim(2, principal_id, timestamp(8, 2)).expect("claim");
    (task, principal_id)
}

pub(crate) fn accepted_submission(task: &HumanTask, principal_id: PrincipalId) -> FormSubmission {
    let id = FormSubmissionId::new();
    let identity = WorkflowInteractionIdentity {
        workflow_run_id: task.workflow_run_id.to_string(),
        flow_run_id: task.flow_run_id.clone(),
        step_id: task.step_id.clone(),
        step_attempt: task.step_attempt,
        human_task_id: task.id.to_string(),
        flow_hook_id: task.flow_hook_id.clone(),
    };
    let mut request = FormInteractionRequest {
        api_version: FORM_INTERACTION_REQUEST_API_VERSION.into(),
        request_id: format!("request-{}", task.id),
        identity: identity.clone(),
        form: task.form_release.clone(),
        assignment: FormInteractionAssignment {
            policy_id: task.assignment_policy.id.clone(),
            policy_revision: task.assignment_policy.revision,
            policy_digest: task.assignment_policy.digest.to_string(),
            claimed_principal_id: principal_id.to_string(),
        },
        task: FormInteractionTaskBinding {
            version: task.aggregate_version,
            created_at: form_timestamp(task.created_at),
            due_at: task.due_at.map(form_timestamp),
            expires_at: task.expires_at.map(form_timestamp),
        },
        allowed_outcomes: vec![
            FormInteractionOutcome::Approve,
            FormInteractionOutcome::Reject,
        ],
        output_mapping: FormInteractionOutputMapping::Identity,
        max_value_bytes: 4_096,
        initial_value: None,
        digest: digest('0'),
    };
    request.digest = digest_interaction_request(&request).expect("request digest");
    let value = parse_json(br#"{"approved":true,"note":"accepted"}"#).expect("value");
    let submission = FormInteractionSubmission {
        api_version: FORM_INTERACTION_SUBMISSION_API_VERSION.into(),
        submission_id: id.to_string(),
        request_id: request.request_id.clone(),
        request_digest: request.digest.clone(),
        identity,
        form: task.form_release.clone(),
        assignment: FormInteractionSubmissionAssignment {
            policy_id: task.assignment_policy.id.clone(),
            policy_revision: task.assignment_policy.revision,
            policy_digest: task.assignment_policy.digest.to_string(),
        },
        task_version: task.aggregate_version,
        principal_id: principal_id.to_string(),
        outcome: FormInteractionOutcome::Approve,
        idempotency_key: format!("approve-{}", task.id),
        submitted_at: form_timestamp(timestamp(8, 29)),
        value: value.clone(),
        value_digest: digest_interaction_value(&value).expect("value digest"),
    };
    FormSubmission::accept(AcceptedFormSubmission {
        organization_id: task.organization_id,
        project_id: task.project_id,
        id,
        workflow_run_id: task.workflow_run_id,
        human_task_id: task.id,
        principal_id,
        authorization_decision: authorization_reference(),
        request,
        submission,
        accepted_value: value,
        accepted_at: timestamp(8, 30),
    })
    .expect("accepted submission")
}

fn form_release(organization_id: OrganizationId, project_id: ProjectId) -> FormReleaseRef {
    FormReleaseRef {
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
    }
}

pub(crate) fn authorization_reference() -> AuthorizationDecisionRef {
    AuthorizationDecisionRef::new(
        "authorization-human-review-2",
        Sha256Digest::parse(digest('d')).expect("authorization digest"),
    )
    .expect("authorization reference")
}

pub(crate) fn timestamp(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 9, hour, minute, 0)
        .single()
        .expect("timestamp")
}

pub(crate) fn workflow_run_input() -> Result<WorkflowRunInput, String> {
    let goal_input = serde_json::json!({"ticketId": "T-42", "priority": "high"});
    let input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &goal_input,
        1024 * 1024,
        "WorkflowRun test input",
    )?))?;
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Input))?;
    let mut normalize = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    normalize.template = Some("{{input}}".into());
    let normalize_configuration = configuration(normalize)?;
    let mut branch = WorkflowStepConfiguration::empty(WorkflowStepKind::Branch);
    branch.selector = Some("current.priority".into());
    branch.default_handle = Some("normal".into());
    branch.routes = vec![
        WorkflowBranchRoute {
            handle: "high".into(),
            equals: "high".into(),
        },
        WorkflowBranchRoute {
            handle: "normal".into(),
            equals: "normal".into(),
        },
    ];
    let branch_configuration = configuration(branch)?;
    let mut high = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    high.template = Some("HIGH {{input.ticketId}}".into());
    let high_configuration = configuration(high)?;
    let mut normal = WorkflowStepConfiguration::empty(WorkflowStepKind::Transform);
    normal.template = Some("NORMAL {{input.ticketId}}".into());
    let normal_configuration = configuration(normal)?;
    let output_configuration =
        configuration(WorkflowStepConfiguration::empty(WorkflowStepKind::Output))?;

    let schema_digest = data_schema.digest().clone();
    let mut payloads = vec![
        data_schema,
        input_configuration.clone(),
        normalize_configuration.clone(),
        branch_configuration.clone(),
        high_configuration.clone(),
        normal_configuration.clone(),
        output_configuration.clone(),
    ];
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let workflow_payload_set_digest = digest_payload_set(&payloads)?;
    let plan = WorkflowPlan {
        schema: WORKFLOW_PLAN_SCHEMA.into(),
        compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION.into(),
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_digest: Sha256Digest::parse(digest('1'))?,
        workflow_payload_set_digest,
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: Sha256Digest::parse(digest('2'))?,
        environment_id: None,
        input_digest,
        steps: vec![
            plan_step(
                "input",
                WorkflowStepKind::Input,
                &input_configuration,
                &schema_digest,
            ),
            plan_step(
                "normalize",
                WorkflowStepKind::Transform,
                &normalize_configuration,
                &schema_digest,
            ),
            plan_step(
                "route",
                WorkflowStepKind::Branch,
                &branch_configuration,
                &schema_digest,
            ),
            plan_step(
                "high",
                WorkflowStepKind::Transform,
                &high_configuration,
                &schema_digest,
            ),
            plan_step(
                "normal",
                WorkflowStepKind::Transform,
                &normal_configuration,
                &schema_digest,
            ),
            plan_step(
                "output",
                WorkflowStepKind::Output,
                &output_configuration,
                &schema_digest,
            ),
        ],
        edges: vec![
            edge("input-normalize", "input", "normalize", None),
            edge("normalize-route", "normalize", "route", None),
            edge("route-high", "route", "high", Some("high")),
            edge("route-normal", "route", "normal", Some("normal")),
            edge("high-output", "high", "output", None),
            edge("normal-output", "normal", "output", None),
        ],
    };
    plan.validate()?;
    let plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "WorkflowRun test plan",
    )?))?;
    let input = WorkflowRunInput {
        schema: WORKFLOW_RUN_INPUT_SCHEMA.into(),
        runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
        flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
        flow_workflow_version: WORKFLOW_RUN_FLOW_VERSION.into(),
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        workflow_run_id: WorkflowRunId::new(),
        workflow_goal_id: WorkflowGoalId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest,
        plan,
        goal_input,
        payloads: payloads
            .iter()
            .map(ResolvedWorkflowPayload::from_payload)
            .collect(),
        requested_at: timestamp(8, 0),
        deadline_at: timestamp(9, 0),
    };
    input.validate()?;
    Ok(input)
}

fn configuration(value: WorkflowStepConfiguration) -> Result<WorkflowPayload, String> {
    WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(value))
}

fn plan_step(
    id: &str,
    kind: WorkflowStepKind,
    configuration: &WorkflowPayload,
    schema_digest: &Sha256Digest,
) -> WorkflowPlanStep {
    WorkflowPlanStep {
        id: id.into(),
        kind,
        configuration_digest: configuration.digest().clone(),
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest.clone(),
        policy_digest: None,
        capability: None,
    }
}

fn edge(id: &str, source: &str, target: &str, source_handle: Option<&str>) -> WorkflowEdgeSpec {
    WorkflowEdgeSpec {
        id: id.into(),
        source: source.into(),
        target: target.into(),
        source_handle: source_handle.map(str::to_owned),
    }
}

fn form_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
