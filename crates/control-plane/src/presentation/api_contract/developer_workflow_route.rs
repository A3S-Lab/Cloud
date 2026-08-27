use crate::modules::developer_workflows::DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX;

pub(super) fn is_developer_workflow_route(path: &str, route: &str) -> bool {
    path.strip_prefix(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX) == Some(route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::WORKLOAD_PROFILE_COLLECTION_ROUTE;

    #[test]
    fn route_matching_requires_the_exact_controller_prefix_and_relative_contract() {
        let route =
            format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{WORKLOAD_PROFILE_COLLECTION_ROUTE}");
        assert!(is_developer_workflow_route(
            &route,
            WORKLOAD_PROFILE_COLLECTION_ROUTE
        ));
        assert!(!is_developer_workflow_route(
            WORKLOAD_PROFILE_COLLECTION_ROUTE,
            WORKLOAD_PROFILE_COLLECTION_ROUTE
        ));
    }
}
