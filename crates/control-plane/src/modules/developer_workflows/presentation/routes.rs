pub(crate) const DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX: &str = "/organizations";

pub(crate) const BUILD_PLAN_DETECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plan-detections";

pub(crate) const BUILD_PLAN_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans";

pub(crate) const BUILD_PLAN_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans/{build_plan_id}";

pub(crate) const WORKLOAD_PROFILE_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles";

pub(crate) const WORKLOAD_PROFILE_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}";

pub(crate) const WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}/revisions";

pub(crate) const WORKLOAD_PROFILE_REVISION_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{workload_profile_id}/revisions/{workload_profile_revision_id}";

pub(crate) const PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies";

pub(crate) const PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies/{source_subscription_id}";

pub(crate) const PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies/{source_subscription_id}/revisions";

pub(crate) const PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-preview-policies/{source_subscription_id}/revisions/{preview_policy_revision_id}";

pub(crate) const PULL_REQUEST_PREVIEW_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/pull-request-previews/{source_subscription_id}/pull-requests/{pull_request_id}";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn developer_workflow_routes_share_one_environment_scoped_contract() {
        let expected_scope =
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/";

        for route in [
            BUILD_PLAN_DETECTION_ROUTE,
            BUILD_PLAN_COLLECTION_ROUTE,
            BUILD_PLAN_ITEM_ROUTE,
            WORKLOAD_PROFILE_COLLECTION_ROUTE,
            WORKLOAD_PROFILE_ITEM_ROUTE,
            WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE,
            WORKLOAD_PROFILE_REVISION_ITEM_ROUTE,
            PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE,
            PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE,
            PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE,
            PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE,
            PULL_REQUEST_PREVIEW_ITEM_ROUTE,
        ] {
            assert!(route.starts_with(expected_scope));
            assert!(!format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}").contains("//"));
        }
    }
}
