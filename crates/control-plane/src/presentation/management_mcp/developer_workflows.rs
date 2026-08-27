use super::arguments::{deserialize_idempotency_key, deserialize_list_limit};
use super::tool_result;
use crate::modules::developer_workflows::{
    AcceptBuildPlan, DetectBuildPlanProposals, GetAcceptedBuildPlan, ListAcceptedBuildPlans,
    DEFAULT_BUILD_PLAN_LIST_LIMIT,
};
use crate::modules::developer_workflows::{
    AcceptedBuildPlanResponse, BuildPlanDetectionResponse, BuildPlanMutationResponse,
};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, OrganizationId, PrincipalId, ProjectId, SourceRevisionId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetectBuildPlansArguments {
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub source_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptBuildPlanArguments {
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub source_revision_id: Uuid,
    pub proposal_acl: String,
    #[serde(deserialize_with = "deserialize_idempotency_key")]
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListAcceptedBuildPlansArguments {
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub source_revision_id: Uuid,
    #[serde(
        default = "default_build_plan_list_limit",
        deserialize_with = "deserialize_list_limit"
    )]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetAcceptedBuildPlanArguments {
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub build_plan_id: Uuid,
}

const fn default_build_plan_list_limit() -> usize {
    DEFAULT_BUILD_PLAN_LIST_LIMIT
}

pub async fn detect_build_plans(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: DetectBuildPlansArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(DetectBuildPlanProposals {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            source_revision_id: SourceRevisionId::from_uuid(arguments.source_revision_id),
            principal_id: actor_principal_id,
        })
        .await?
    {
        Ok(detection) => {
            tool_result::success(200, BuildPlanDetectionResponse::from(detection), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn accept_build_plan(
    bus: Arc<CommandBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: AcceptBuildPlanArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(AcceptBuildPlan {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            source_revision_id: SourceRevisionId::from_uuid(arguments.source_revision_id),
            proposal_acl: arguments.proposal_acl,
            actor_principal_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => {
            let status = if result.replayed { 200 } else { 201 };
            tool_result::success(status, BuildPlanMutationResponse::from(result), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_build_plans(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: ListAcceptedBuildPlansArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListAcceptedBuildPlans {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            source_revision_id: SourceRevisionId::from_uuid(arguments.source_revision_id),
            limit: arguments.limit,
            principal_id: actor_principal_id,
        })
        .await?
    {
        Ok(plans) => tool_result::success(
            200,
            plans
                .into_iter()
                .map(AcceptedBuildPlanResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_build_plan(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    arguments: GetAcceptedBuildPlanArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetAcceptedBuildPlan {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
            build_plan_id: BuildPlanId::from_uuid(arguments.build_plan_id),
            principal_id: actor_principal_id,
        })
        .await?
    {
        Ok(plan) => tool_result::success(200, AcceptedBuildPlanResponse::from(plan), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::MAXIMUM_BUILD_PLAN_LIST_LIMIT;
    use serde_json::json;

    fn scope() -> (Uuid, Uuid, Uuid) {
        (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7())
    }

    #[test]
    fn build_plan_arguments_are_closed_and_acl_only() {
        let (project_id, environment_id, source_revision_id) = scope();
        let acceptance = serde_json::from_value::<AcceptBuildPlanArguments>(json!({
            "projectId": project_id,
            "environmentId": environment_id,
            "sourceRevisionId": source_revision_id,
            "proposalAcl": "build_plan {}\n",
            "idempotencyKey": "accept-build-plan"
        }))
        .expect("closed ACL-only acceptance arguments");
        assert_eq!(acceptance.proposal_acl, "build_plan {}\n");

        assert!(serde_json::from_value::<AcceptBuildPlanArguments>(json!({
            "projectId": project_id,
            "environmentId": environment_id,
            "sourceRevisionId": source_revision_id,
            "proposalAcl": "build_plan {}\n",
            "proposal": {},
            "idempotencyKey": "accept-build-plan"
        }))
        .is_err());
        assert!(serde_json::from_value::<DetectBuildPlansArguments>(json!({
            "projectId": project_id,
            "environmentId": environment_id,
            "sourceRevisionId": source_revision_id,
            "sourceBytes": []
        }))
        .is_err());
    }

    #[test]
    fn build_plan_list_arguments_share_the_application_page_bound() {
        let (project_id, environment_id, source_revision_id) = scope();
        let defaulted = serde_json::from_value::<ListAcceptedBuildPlansArguments>(json!({
            "projectId": project_id,
            "environmentId": environment_id,
            "sourceRevisionId": source_revision_id
        }))
        .expect("default list limit");
        assert_eq!(defaulted.limit, DEFAULT_BUILD_PLAN_LIST_LIMIT);

        for limit in [0, MAXIMUM_BUILD_PLAN_LIST_LIMIT + 1] {
            assert!(
                serde_json::from_value::<ListAcceptedBuildPlansArguments>(json!({
                    "projectId": project_id,
                    "environmentId": environment_id,
                    "sourceRevisionId": source_revision_id,
                    "limit": limit
                }))
                .is_err()
            );
        }
    }
}
