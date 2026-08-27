pub(crate) const DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX: &str = "/organizations";

pub(crate) const BUILD_PLAN_DETECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plan-detections";

pub(crate) const BUILD_PLAN_COLLECTION_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans";

pub(crate) const BUILD_PLAN_ITEM_ROUTE: &str =
    "/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans/{build_plan_id}";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_plan_routes_share_one_environment_scoped_contract() {
        let expected_scope =
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/";

        for route in [
            BUILD_PLAN_DETECTION_ROUTE,
            BUILD_PLAN_COLLECTION_ROUTE,
            BUILD_PLAN_ITEM_ROUTE,
        ] {
            assert!(route.starts_with(expected_scope));
            assert!(!format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}").contains("//"));
        }
    }
}
