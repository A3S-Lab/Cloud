use super::tool_result;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PlanRevisionId, PrincipalId, ProjectId, WorkflowDefinitionId, WorkflowGoalId,
    WorkflowRevisionId,
};
use crate::modules::workflow::presentation::{
    PlanRevisionResponse, WorkflowDefinitionMutationResponse, WorkflowDefinitionResponse,
    WorkflowGoalMutationResponse, WorkflowGoalResponse, WorkflowRevisionResponse,
    WorkflowRevisionSummaryResponse,
};
use crate::modules::workflow::{
    CreateWorkflowDefinition, CreateWorkflowGoal, GetPlanRevision, GetWorkflowDefinition,
    GetWorkflowGoal, GetWorkflowRevision, ListWorkflowDefinitions, ListWorkflowGoals,
    ListWorkflowRevisions, ReviseWorkflowDefinition, WorkflowPayloadAcl, WorkflowPayloadKind,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
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
