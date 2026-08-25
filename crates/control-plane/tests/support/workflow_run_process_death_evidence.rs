use super::fixture::Fixture;
use super::process::ProbeMode;
use super::TestResult;
use a3s_cloud_control_plane::modules::executions::{Execution, ExecutionStatus};
use a3s_cloud_control_plane::modules::workflow::domain::{
    WorkflowExecutionChildReferenceMetadata, WORKFLOW_EXECUTION_CHILD_REFERENCE_SCHEMA,
    WORKFLOW_EXECUTION_RESULT_SCHEMA,
};
use a3s_cloud_control_plane::modules::workflow::{
    IWorkflowRunRepository, WorkflowExecutionOutcome, WorkflowExecutionStepOutput,
    WorkflowRunInput, WorkflowRunRecord,
};
use a3s_flow::{FlowEngine, FlowEvent, FlowEventEnvelope, WorkflowRunSnapshot};
use std::sync::Arc;

pub(super) fn require_marker_identity(
    marker: &serde_json::Value,
    mode: ProbeMode,
    input: &WorkflowRunInput,
) -> TestResult {
    let expected_run = input.workflow_run_id.to_string();
    if marker["mode"].as_str() != Some(mode.as_str())
        || marker["workflowRunId"].as_str() != Some(expected_run.as_str())
        || marker["operationId"].as_str() != Some(expected_run.as_str())
        || marker["flowRunId"].as_str() != Some(expected_run.as_str())
        || marker["aggregateVersion"].as_u64().is_none()
    {
        return Err(format!(
            "WorkflowRun crash marker did not bind {} to its durable identities: {marker}",
            mode.as_str()
        )
        .into());
    }
    Ok(())
}

pub(super) fn require_run_identity(
    record: &WorkflowRunRecord,
    input: &WorkflowRunInput,
) -> TestResult {
    if record.run.id != input.workflow_run_id
        || record.run.operation_id.as_uuid() != input.workflow_run_id.as_uuid()
        || record.run.flow_run_id != input.workflow_run_id.to_string()
        || record.run.execution_input != *input
    {
        return Err("WorkflowRun, Operation, Flow, or immutable input identity drifted".into());
    }
    Ok(())
}

pub(super) fn require_execution_marker_identity(
    marker: &serde_json::Value,
    mode: ProbeMode,
    input: &WorkflowRunInput,
    execution: &Execution,
) -> TestResult {
    require_marker_identity(marker, mode, input)?;
    let binding = execution
        .workflow
        .as_ref()
        .ok_or("finite child Execution lost its Workflow binding")?;
    let expected_execution = execution.id.to_string();
    let expected_operation = execution.operation_id.to_string();
    let expected_template = binding.execution_template_id.to_string();
    let expected_revision = binding.execution_template_revision_id.to_string();
    let expected_definition_digest = binding.execution_template_digest.to_string();
    if marker["executionId"].as_str() != Some(expected_execution.as_str())
        || marker["executionOperationId"].as_str() != Some(expected_operation.as_str())
        || marker["executionStatus"].as_str() != Some(execution.status.as_str())
        || marker["executionAggregateVersion"].as_u64() != Some(execution.aggregate_version)
        || marker["executionTemplateId"].as_str() != Some(expected_template.as_str())
        || marker["executionTemplateRevisionId"].as_str() != Some(expected_revision.as_str())
        || marker["executionTemplateDigest"].as_str() != Some(expected_definition_digest.as_str())
        || marker["invocationTemplateDigest"].as_str() != Some(execution.template_digest.as_str())
    {
        return Err(format!(
            "finite child crash marker did not retain its exact authority: {marker}"
        )
        .into());
    }
    Ok(())
}

pub(super) fn require_execution_authority(
    input: &WorkflowRunInput,
    execution: &Execution,
) -> TestResult {
    let capability = input
        .plan
        .steps
        .iter()
        .find(|step| {
            step.kind == a3s_cloud_control_plane::modules::workflow::WorkflowStepKind::Execution
        })
        .and_then(|step| step.capability.as_ref())
        .ok_or("finite Workflow plan lost its ExecutionTemplate capability")?;
    let binding = execution
        .workflow
        .as_ref()
        .ok_or("finite child Execution lost its Workflow binding")?;
    if execution.organization_id != input.organization_id
        || execution.project_id != input.project_id
        || Some(execution.environment_id) != input.plan.environment_id
        || execution.operation_id.as_uuid() != execution.id.as_uuid()
        || binding.workflow_run_id != input.workflow_run_id
        || binding.plan_revision_id != input.plan_revision_id
        || binding.plan_digest != input.plan_digest
        || binding.step_id != super::fixture::EXECUTION_STEP_ID
        || binding.step_attempt != 1
        || binding.execution_template_id.as_uuid() != capability.resource_id
        || binding.execution_template_revision_id.to_string() != capability.revision
        || binding.execution_template_digest != capability.digest
    {
        return Err("finite child Execution authority drifted from its immutable plan".into());
    }
    Ok(())
}

pub(super) fn require_execution_child_reference(
    input: &WorkflowRunInput,
    execution: &Execution,
    snapshot: &WorkflowRunSnapshot,
) -> TestResult {
    require_execution_authority(input, execution)?;
    if snapshot.child_operations.len() != 1 {
        return Err(format!(
            "finite Workflow parent retained {} child references instead of one",
            snapshot.child_operations.len()
        )
        .into());
    }
    let expected_reference = format!(
        "workflow-execution:{}:{}",
        super::fixture::EXECUTION_STEP_ID,
        1
    );
    let child = snapshot
        .child_operations
        .get(&expected_reference)
        .ok_or("finite Workflow parent lost its authority-bound child reference")?;
    if child.reference_id != expected_reference
        || child.kind != "execution"
        || child.operation_id != execution.operation_id.to_string()
        || child.flow_run_id.as_deref() != Some(child.operation_id.as_str())
    {
        return Err("finite Workflow child reference identity drifted".into());
    }
    let metadata: WorkflowExecutionChildReferenceMetadata =
        serde_json::from_value(child.metadata.clone())?;
    let binding = execution
        .workflow
        .as_ref()
        .ok_or("finite child Execution lost its Workflow binding")?;
    if metadata.schema != WORKFLOW_EXECUTION_CHILD_REFERENCE_SCHEMA
        || metadata.organization_id != input.organization_id
        || metadata.project_id != input.project_id
        || metadata.workflow_run_id != input.workflow_run_id
        || metadata.plan_revision_id != input.plan_revision_id
        || metadata.plan_digest != input.plan_digest
        || metadata.step_id != super::fixture::EXECUTION_STEP_ID
        || metadata.step_attempt != 1
        || metadata.execution_template_id != binding.execution_template_id
        || metadata.execution_template_revision_id != binding.execution_template_revision_id
        || metadata.execution_template_digest != binding.execution_template_digest
        || metadata.invocation_template_digest.to_string() != execution.template_digest
    {
        return Err("finite Workflow child reference metadata drifted".into());
    }
    Ok(())
}

pub(super) fn require_execution_output(
    input: &WorkflowRunInput,
    execution: &Execution,
    output: &WorkflowExecutionStepOutput,
) -> TestResult {
    require_execution_authority(input, execution)?;
    let binding = execution
        .workflow
        .as_ref()
        .ok_or("finite child Execution lost its Workflow binding")?;
    if execution.status != ExecutionStatus::Succeeded
        || output.schema != WORKFLOW_EXECUTION_RESULT_SCHEMA
        || output.execution_id != execution.id
        || output.operation_id != execution.operation_id
        || output.execution_template_id != binding.execution_template_id
        || output.execution_template_revision_id != binding.execution_template_revision_id
        || output.execution_template_digest != binding.execution_template_digest
        || output.invocation_template_digest.to_string() != execution.template_digest
        || output.outcome != (WorkflowExecutionOutcome::Succeeded { exit_code: 0 })
        || Some(output.finished_at) != execution.finished_at
    {
        return Err("finite Workflow output drifted from its terminal child authority".into());
    }
    Ok(())
}

pub(super) fn require_completed_history(history: &[FlowEventEnvelope]) -> TestResult {
    let created = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCreated { .. }))
        .count();
    let started = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunStarted))
        .count();
    let completed = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCompleted { .. }))
        .count();
    if (created, started, completed) != (1, 1, 1) {
        return Err(format!(
            "completed Flow history was duplicated: created={created}, started={started}, completed={completed}"
        )
        .into());
    }
    Ok(())
}

pub(super) fn require_cancellation_history(history: &[FlowEventEnvelope]) -> TestResult {
    let requested = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCancellationRequested { .. }))
        .count();
    let cancelled = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCancelled { .. }))
        .count();
    if (requested, cancelled) != (1, 1) {
        return Err(format!(
            "Flow cancellation history was not exact once: requested={requested}, cancelled={cancelled}"
        )
        .into());
    }
    Ok(())
}

pub(super) async fn require_history_unchanged(
    engine: &FlowEngine,
    input: &WorkflowRunInput,
    before: &[FlowEventEnvelope],
    label: &str,
) -> TestResult {
    let after = engine.history(&input.workflow_run_id.to_string()).await?;
    if after != before {
        return Err(format!("{label} appended duplicate Flow history").into());
    }
    Ok(())
}

pub(super) async fn require_child_history_unchanged(
    engine: &FlowEngine,
    execution: &Execution,
    before: &[FlowEventEnvelope],
    label: &str,
) -> TestResult {
    let after = engine.history(&execution.operation_id.to_string()).await?;
    if after != before {
        return Err(format!("{label} appended duplicate child Flow history").into());
    }
    Ok(())
}

pub(super) async fn require_run_version(
    repository: &Arc<dyn IWorkflowRunRepository>,
    input: &WorkflowRunInput,
    expected: u64,
    label: &str,
) -> TestResult {
    let actual = repository
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or_else(|| format!("WorkflowRun disappeared during {label}"))?
        .run
        .aggregate_version;
    if actual != expected {
        return Err(format!(
            "WorkflowRun aggregate version changed during {label}: expected {expected}, got {actual}"
        )
        .into());
    }
    Ok(())
}

pub(super) async fn verify_database_evidence(
    fixture: &Fixture,
    composite_child_run_ids: &[String],
) -> TestResult {
    let connection = fixture.executor.pool().get().await?;
    let execution_run_id = fixture.document.execution_input.workflow_run_id.as_uuid();
    let binding = connection
        .query_one(
            "select id, operation_id, workflow_plan_revision_id, workflow_plan_digest, workflow_step_id, workflow_step_attempt, execution_template_id, execution_template_revision_id, execution_template_definition_digest, template_digest, status, aggregate_version \
             from executions where organization_id = $1 and workflow_run_id = $2",
            &[
                &fixture.document.execution_input.organization_id.as_uuid(),
                &execution_run_id,
            ],
        )
        .await?;
    let child_id = binding.get::<_, uuid::Uuid>(0);
    let child_operation_id = binding.get::<_, uuid::Uuid>(1);
    let template_id = binding.get::<_, uuid::Uuid>(6);
    let template_revision_id = binding.get::<_, uuid::Uuid>(7);
    let template_digest = binding.get::<_, String>(8);
    let capability = fixture
        .document
        .execution_input
        .plan
        .steps
        .iter()
        .find(|step| {
            step.kind == a3s_cloud_control_plane::modules::workflow::WorkflowStepKind::Execution
        })
        .and_then(|step| step.capability.as_ref())
        .ok_or("process-death evidence lost its ExecutionTemplate capability")?;
    let stored_binding = (
        child_id,
        child_operation_id,
        binding.get::<_, uuid::Uuid>(2),
        binding.get::<_, String>(3),
        binding.get::<_, String>(4),
        binding.get::<_, i64>(5),
        template_id,
        template_revision_id,
        template_digest.clone(),
        binding.get::<_, String>(10),
        binding.get::<_, i64>(11),
    );
    let expected_binding = (
        child_id,
        child_id,
        fixture.document.execution_input.plan_revision_id.as_uuid(),
        fixture.document.execution_input.plan_digest.to_string(),
        super::fixture::EXECUTION_STEP_ID.to_owned(),
        1_i64,
        capability.resource_id,
        uuid::Uuid::parse_str(&capability.revision)?,
        capability.digest.to_string(),
        "succeeded".to_owned(),
        3_i64,
    );
    if stored_binding != expected_binding || binding.get::<_, String>(9).is_empty() {
        return Err(
            format!("finite child relational authority was not exact: {stored_binding:?}").into(),
        );
    }
    let [loop_child, iteration_child_a, iteration_child_b] = composite_child_run_ids else {
        return Err(format!(
            "WorkflowRun process-death evidence expected three composite children, got {composite_child_run_ids:?}"
        )
        .into());
    };
    let loop_child = uuid::Uuid::parse_str(loop_child)?;
    let iteration_child_a = uuid::Uuid::parse_str(iteration_child_a)?;
    let iteration_child_b = uuid::Uuid::parse_str(iteration_child_b)?;
    let organization_id = fixture.document.terminal_input.organization_id.as_uuid();
    let row = connection
        .query_one(
            "select \
                (select count(*) from workflow_runs where organization_id = $1), \
                (select count(*) from operation_requests where organization_id = $1), \
                (select count(*) from operation_projections projection join operation_requests request using (operation_id) where request.organization_id = $1), \
                (select count(*) from workflow_goals where organization_id = $1), \
                (select count(*) from workflow_plan_revisions where organization_id = $1), \
                (select count(*) from workflow_definitions where organization_id = $1), \
                (select count(*) from workflow_revisions where organization_id = $1), \
                (select count(*) from ontologies where organization_id = $1), \
                (select count(*) from ontology_revisions where organization_id = $1), \
                (select count(*) from outbox_events where organization_id = $1 and event_key = 'workflow.run.requested'), \
                (select count(*) from outbox_events where organization_id = $1 and event_key = 'workflow.run.cancellation.requested'), \
                (select count(*) from outbox_events where organization_id = $1 and event_key = 'workflow.goal.compiled'), \
                (select count(*) from outbox_events where organization_id = $1 and event_key = 'workflow.definition.created'), \
                (select count(*) from outbox_events where organization_id = $1 and event_key = 'workflow.ontology.created'), \
                (select count(*) from executions where organization_id = $1), \
                (select count(*) from outbox_events where organization_id = $1 and event_key = 'execution.run.requested'), \
                (select count(*) from execution_template_revisions where template_id = $2 and revision_id = $3 and definition_digest = $4), \
                (select count(*) from outbox_events where aggregate_id = $2 and event_key = 'execution.template.published'), \
                (select count(*) from audit_records where aggregate_id = $2 and action = 'execution.template.published'), \
                (select count(*) from workflow_runs where id in ($5, $6, $7)), \
                (select count(*) from operation_requests where operation_id in ($5, $6, $7)), \
                (select count(*) from idempotency_records)",
            &[
                &organization_id,
                &template_id,
                &template_revision_id,
                &template_digest,
                &loop_child,
                &iteration_child_a,
                &iteration_child_b,
            ],
        )
        .await?;
    let evidence = [
        row.get::<_, i64>(0),
        row.get::<_, i64>(1),
        row.get::<_, i64>(2),
        row.get::<_, i64>(3),
        row.get::<_, i64>(4),
        row.get::<_, i64>(5),
        row.get::<_, i64>(6),
        row.get::<_, i64>(7),
        row.get::<_, i64>(8),
        row.get::<_, i64>(9),
        row.get::<_, i64>(10),
        row.get::<_, i64>(11),
        row.get::<_, i64>(12),
        row.get::<_, i64>(13),
        row.get::<_, i64>(14),
        row.get::<_, i64>(15),
        row.get::<_, i64>(16),
        row.get::<_, i64>(17),
        row.get::<_, i64>(18),
        row.get::<_, i64>(19),
        row.get::<_, i64>(20),
        row.get::<_, i64>(21),
    ];
    if evidence
        != [
            8, 9, 9, 8, 8, 6, 6, 4, 4, 8, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 3, 16,
        ]
    {
        return Err(format!(
            "WorkflowRun process-death relational evidence was not exact once: {evidence:?}"
        )
        .into());
    }
    Ok(())
}
