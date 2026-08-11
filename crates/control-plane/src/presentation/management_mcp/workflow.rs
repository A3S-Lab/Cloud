use super::tool_result;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PlanRevisionId, PrincipalId, ProjectId, WorkflowDefinitionId, WorkflowGoalId,
    WorkflowRevisionId, WorkflowRunId,
};
use crate::modules::workflow::presentation::{
    PlanRevisionResponse, WorkflowDefinitionMutationResponse, WorkflowDefinitionResponse,
    WorkflowGoalMutationResponse, WorkflowGoalResponse, WorkflowRevisionResponse,
    WorkflowRevisionSummaryResponse, WorkflowRunMutationResponse, WorkflowRunOutputResponse,
    WorkflowRunResponse,
};
use crate::modules::workflow::{
    CancelWorkflowRun, CreateWorkflowDefinition, CreateWorkflowGoal, GetPlanRevision,
    GetWorkflowDefinition, GetWorkflowGoal, GetWorkflowRevision, GetWorkflowRun,
    GetWorkflowRunHistory, GetWorkflowRunOutput, ListWorkflowDefinitions, ListWorkflowGoals,
    ListWorkflowRevisions, ListWorkflowRuns, ReviseWorkflowDefinition, StartWorkflowRun,
    WaitWorkflowRun, WorkflowPayloadAcl, WorkflowPayloadKind, WORKFLOW_RUN_HISTORY_MAX_LIMIT,
    WORKFLOW_RUN_LIST_MAX_LIMIT, WORKFLOW_RUN_MAX_TIMEOUT_SECONDS, WORKFLOW_RUN_WAIT_MAX_TIMEOUT,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowDefinitionArguments {
    workflow_definition_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowRevisionArguments {
    workflow_definition_id: Uuid,
    workflow_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowGoalArguments {
    workflow_goal_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowPlanRevisionArguments {
    workflow_goal_id: Uuid,
    plan_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListProjectWorkflowArguments {
    project_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowPayloadArguments {
    kind: WorkflowPayloadKind,
    acl: String,
}

impl From<WorkflowPayloadArguments> for WorkflowPayloadAcl {
    fn from(value: WorkflowPayloadArguments) -> Self {
        Self {
            kind: value.kind,
            acl: value.acl,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkflowDefinitionArguments {
    project_id: Uuid,
    definition_acl: String,
    payloads: Vec<WorkflowPayloadArguments>,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseWorkflowDefinitionArguments {
    workflow_definition_id: Uuid,
    definition_acl: String,
    payloads: Vec<WorkflowPayloadArguments>,
    expected_version: u64,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkflowGoalArguments {
    project_id: Uuid,
    acl: String,
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowRunArguments {
    workflow_run_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartWorkflowRunArguments {
    project_id: Uuid,
    workflow_goal_id: Uuid,
    plan_revision_id: Uuid,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_workflow_run_timeout"
    )]
    timeout_seconds: Option<u64>,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelWorkflowRunArguments {
    workflow_run_id: Uuid,
    #[serde(default, deserialize_with = "deserialize_optional_cancellation_reason")]
    reason: Option<String>,
    #[serde(deserialize_with = "super::arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListWorkflowRunsArguments {
    project_id: Uuid,
    #[serde(default, deserialize_with = "deserialize_optional_workflow_run_limit")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitWorkflowRunArguments {
    workflow_run_id: Uuid,
    #[serde(default, deserialize_with = "deserialize_optional_workflow_run_wait")]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowRunHistoryArguments {
    workflow_run_id: Uuid,
    #[serde(default, deserialize_with = "deserialize_optional_history_sequence")]
    after_sequence: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_history_limit")]
    limit: Option<usize>,
}

fn deserialize_optional_workflow_run_timeout<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if value == 0 || value > WORKFLOW_RUN_MAX_TIMEOUT_SECONDS {
        return Err(D::Error::custom(format!(
            "WorkflowRun timeout must be between 1 and {WORKFLOW_RUN_MAX_TIMEOUT_SECONDS} seconds"
        )));
    }
    Ok(Some(value))
}

fn deserialize_optional_cancellation_reason<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        return Err(D::Error::custom(
            "WorkflowRun cancellation reason is invalid",
        ));
    }
    Ok(Some(value))
}

fn deserialize_optional_workflow_run_limit<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value == 0 || value > WORKFLOW_RUN_LIST_MAX_LIMIT {
        return Err(D::Error::custom(format!(
            "WorkflowRun list limit must be between 1 and {WORKFLOW_RUN_LIST_MAX_LIMIT}"
        )));
    }
    Ok(Some(value))
}

fn deserialize_optional_workflow_run_wait<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if value > WORKFLOW_RUN_WAIT_MAX_TIMEOUT.as_secs() {
        return Err(D::Error::custom(format!(
            "WorkflowRun wait timeout cannot exceed {} seconds",
            WORKFLOW_RUN_WAIT_MAX_TIMEOUT.as_secs()
        )));
    }
    Ok(Some(value))
}

fn deserialize_optional_history_sequence<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    u64::deserialize(deserializer).map(Some)
}

fn deserialize_optional_history_limit<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value == 0 || value > WORKFLOW_RUN_HISTORY_MAX_LIMIT {
        return Err(D::Error::custom(format!(
            "WorkflowRun history limit must be between 1 and {WORKFLOW_RUN_HISTORY_MAX_LIMIT}"
        )));
    }
    Ok(Some(value))
}

pub async fn create_definition(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateWorkflowDefinitionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateWorkflowDefinition {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            definition_acl: arguments.definition_acl,
            payloads: arguments
                .payloads
                .into_iter()
                .map(WorkflowPayloadAcl::from)
                .collect(),
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            WorkflowDefinitionMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revise_definition(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ReviseWorkflowDefinitionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ReviseWorkflowDefinition {
            organization_id,
            workflow_definition_id: WorkflowDefinitionId::from_uuid(
                arguments.workflow_definition_id,
            ),
            expected_version: arguments.expected_version,
            definition_acl: arguments.definition_acl,
            payloads: arguments
                .payloads
                .into_iter()
                .map(WorkflowPayloadAcl::from)
                .collect(),
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            WorkflowDefinitionMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_goal(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CreateWorkflowGoalArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreateWorkflowGoal {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            goal_acl: arguments.acl,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            WorkflowGoalMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_definitions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListProjectWorkflowArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListWorkflowDefinitions {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
        })
        .await?
    {
        Ok(values) => tool_result::success(
            200,
            values
                .into_iter()
                .map(WorkflowDefinitionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_definition(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowDefinitionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkflowDefinition {
            organization_id,
            workflow_definition_id: WorkflowDefinitionId::from_uuid(
                arguments.workflow_definition_id,
            ),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, WorkflowDefinitionResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_revisions(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowDefinitionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListWorkflowRevisions {
            organization_id,
            workflow_definition_id: WorkflowDefinitionId::from_uuid(
                arguments.workflow_definition_id,
            ),
        })
        .await?
    {
        Ok(values) => tool_result::success(
            200,
            values
                .into_iter()
                .map(WorkflowRevisionSummaryResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkflowRevision {
            organization_id,
            workflow_definition_id: WorkflowDefinitionId::from_uuid(
                arguments.workflow_definition_id,
            ),
            workflow_revision_id: WorkflowRevisionId::from_uuid(arguments.workflow_revision_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, WorkflowRevisionResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_goals(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListProjectWorkflowArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListWorkflowGoals {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
        })
        .await?
    {
        Ok(values) => tool_result::success(
            200,
            values
                .into_iter()
                .map(WorkflowGoalResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_goal(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowGoalArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkflowGoal {
            organization_id,
            workflow_goal_id: WorkflowGoalId::from_uuid(arguments.workflow_goal_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, WorkflowGoalResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_plan_revision(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowPlanRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPlanRevision {
            organization_id,
            workflow_goal_id: WorkflowGoalId::from_uuid(arguments.workflow_goal_id),
            plan_revision_id: PlanRevisionId::from_uuid(arguments.plan_revision_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, PlanRevisionResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn start_run(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: StartWorkflowRunArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(StartWorkflowRun {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            workflow_goal_id: WorkflowGoalId::from_uuid(arguments.workflow_goal_id),
            plan_revision_id: PlanRevisionId::from_uuid(arguments.plan_revision_id),
            timeout_seconds: arguments.timeout_seconds,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
            requested_at: chrono::Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 202 },
            WorkflowRunMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn cancel_run(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: CancelWorkflowRunArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CancelWorkflowRun {
            organization_id,
            workflow_run_id: WorkflowRunId::from_uuid(arguments.workflow_run_id),
            reason: arguments.reason,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
            requested_at: chrono::Utc::now(),
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 202 },
            WorkflowRunMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_runs(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: ListWorkflowRunsArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListWorkflowRuns {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            limit: arguments.limit.unwrap_or(100),
        })
        .await?
    {
        Ok(values) => tool_result::success(
            200,
            values
                .into_iter()
                .map(WorkflowRunResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_run(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowRunArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkflowRun {
            organization_id,
            workflow_run_id: WorkflowRunId::from_uuid(arguments.workflow_run_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, WorkflowRunResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn wait_run(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WaitWorkflowRunArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(WaitWorkflowRun {
            organization_id,
            workflow_run_id: WorkflowRunId::from_uuid(arguments.workflow_run_id),
            timeout: std::time::Duration::from_secs(arguments.timeout_seconds.unwrap_or(30)),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, WorkflowRunResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_run_output(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowRunArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkflowRunOutput {
            organization_id,
            workflow_run_id: WorkflowRunId::from_uuid(arguments.workflow_run_id),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, WorkflowRunOutputResponse::from(value), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_run_history(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: WorkflowRunHistoryArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkflowRunHistory {
            organization_id,
            workflow_run_id: WorkflowRunId::from_uuid(arguments.workflow_run_id),
            after_sequence: arguments.after_sequence.unwrap_or(0),
            limit: arguments.limit.unwrap_or(100),
        })
        .await?
    {
        Ok(value) => tool_result::success(200, value, request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
