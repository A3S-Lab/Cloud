use super::developer_workflow_route::is_developer_workflow_route;
use super::workload_profile_components::accept_workload_profile_request_schema;
use crate::modules::developer_workflows::{
    DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT, MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
    WORKLOAD_PROFILE_COLLECTION_ROUTE, WORKLOAD_PROFILE_ITEM_ROUTE,
    WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE, WORKLOAD_PROFILE_REVISION_ITEM_ROUTE,
};
use serde_json::{json, Value};

pub(super) fn is_workload_profile_path(path: &str) -> bool {
    is_workload_profile_collection_path(path)
        || is_workload_profile_item_path(path)
        || is_workload_profile_revision_collection_path(path)
        || is_workload_profile_revision_item_path(path)
}

pub(super) fn is_workload_profile_collection_path(path: &str) -> bool {
    is_developer_workflow_route(path, WORKLOAD_PROFILE_COLLECTION_ROUTE)
}

pub(super) fn is_workload_profile_item_path(path: &str) -> bool {
    is_developer_workflow_route(path, WORKLOAD_PROFILE_ITEM_ROUTE)
}

pub(super) fn is_workload_profile_revision_collection_path(path: &str) -> bool {
    is_developer_workflow_route(path, WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE)
}

pub(super) fn is_workload_profile_revision_item_path(path: &str) -> bool {
    is_developer_workflow_route(path, WORKLOAD_PROFILE_REVISION_ITEM_ROUTE)
}

pub(super) fn is_workload_profile_request_body_path(path: &str) -> bool {
    is_workload_profile_collection_path(path)
}

pub(super) fn request_schema(path: &str) -> Option<Value> {
    is_workload_profile_collection_path(path).then(accept_workload_profile_request_schema)
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method != "get" || !is_workload_profile_revision_collection_path(path) {
        return Vec::new();
    }
    vec![json!({
        "name": "limit",
        "in": "query",
        "required": false,
        "description": "Maximum number of immutable WorkloadProfile revisions returned in ascending revision order.",
        "schema": {
            "type": "integer",
            "minimum": 1,
            "maximum": MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
            "default": DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT
        }
    })]
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<String> {
    if method == "post" && is_workload_profile_collection_path(path) && matches!(status, 200 | 201)
    {
        Some(format!("WorkloadProfileMutationSuccess{status}"))
    } else if method == "get"
        && (is_workload_profile_item_path(path) || is_workload_profile_revision_item_path(path))
        && status == 200
    {
        Some("AcceptedWorkloadProfileRevisionSuccess200".into())
    } else if method == "get" && is_workload_profile_revision_collection_path(path) && status == 200
    {
        Some("AcceptedWorkloadProfileRevisionListSuccess200".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX;

    fn full_route(route: &str) -> String {
        format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}")
    }

    #[test]
    fn route_classification_is_exact_and_environment_bound() {
        for route in [
            WORKLOAD_PROFILE_COLLECTION_ROUTE,
            WORKLOAD_PROFILE_ITEM_ROUTE,
            WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE,
            WORKLOAD_PROFILE_REVISION_ITEM_ROUTE,
        ] {
            assert!(is_workload_profile_path(&full_route(route)));
        }
        for path in [
            WORKLOAD_PROFILE_COLLECTION_ROUTE,
            "/organizations/{organization_id}/workload-profiles",
            "/organizations/{organization_id}/projects/{project_id}/workload-profiles",
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/workload-profiles/{other_id}/revisions/{revision_id}",
        ] {
            assert!(!is_workload_profile_path(path), "accepted foreign route {path}");
        }
    }

    #[test]
    fn revision_history_query_is_bounded() {
        let parameters = query_parameters(
            "get",
            &full_route(WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE),
        );
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0]["name"], "limit");
        assert_eq!(
            parameters[0]["schema"]["maximum"],
            MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT
        );
        assert_eq!(
            parameters[0]["schema"]["default"],
            DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT
        );
        assert!(query_parameters("get", &full_route(WORKLOAD_PROFILE_ITEM_ROUTE)).is_empty());
    }

    #[test]
    fn request_and_response_components_follow_revision_semantics() {
        let collection = full_route(WORKLOAD_PROFILE_COLLECTION_ROUTE);
        let current = full_route(WORKLOAD_PROFILE_ITEM_ROUTE);
        let revisions = full_route(WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE);
        let revision = full_route(WORKLOAD_PROFILE_REVISION_ITEM_ROUTE);
        let request = request_schema(&collection).expect("acceptance request schema");
        assert_eq!(request["required"], json!(["buildPlanId", "profileAcl"]));
        assert!(request_schema(&current).is_none());
        for status in [200, 201] {
            assert_eq!(
                success_component("post", &collection, status),
                Some(format!("WorkloadProfileMutationSuccess{status}"))
            );
        }
        for path in [current, revision] {
            assert_eq!(
                success_component("get", &path, 200).as_deref(),
                Some("AcceptedWorkloadProfileRevisionSuccess200")
            );
        }
        assert_eq!(
            success_component("get", &revisions, 200).as_deref(),
            Some("AcceptedWorkloadProfileRevisionListSuccess200")
        );
    }
}
