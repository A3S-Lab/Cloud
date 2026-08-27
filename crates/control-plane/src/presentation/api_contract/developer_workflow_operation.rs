use super::developer_workflow_components::{
    accept_build_plan_request_schema, detect_build_plans_request_schema,
};
use crate::modules::developer_workflows::{
    BUILD_PLAN_COLLECTION_ROUTE, BUILD_PLAN_DETECTION_ROUTE, BUILD_PLAN_ITEM_ROUTE,
    DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX,
};
use crate::modules::developer_workflows::{
    DEFAULT_BUILD_PLAN_LIST_LIMIT, MAXIMUM_BUILD_PLAN_LIST_LIMIT,
};
use serde_json::{json, Value};

pub(super) fn is_build_plan_path(path: &str) -> bool {
    is_build_plan_detection_path(path)
        || is_build_plan_collection_path(path)
        || is_build_plan_item_path(path)
}

pub(super) fn is_build_plan_detection_path(path: &str) -> bool {
    is_route(path, BUILD_PLAN_DETECTION_ROUTE)
}

pub(super) fn is_build_plan_collection_path(path: &str) -> bool {
    is_route(path, BUILD_PLAN_COLLECTION_ROUTE)
}

pub(super) fn is_build_plan_request_body_path(path: &str) -> bool {
    is_build_plan_detection_path(path) || is_build_plan_collection_path(path)
}

pub(super) fn request_schema(path: &str) -> Option<Value> {
    if is_build_plan_detection_path(path) {
        Some(detect_build_plans_request_schema())
    } else if is_build_plan_collection_path(path) {
        Some(accept_build_plan_request_schema())
    } else {
        None
    }
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method != "get" || !is_build_plan_collection_path(path) {
        return Vec::new();
    }
    vec![
        json!({
            "name": "sourceRevisionId",
            "in": "query",
            "required": true,
            "description": "Exact immutable SourceRevision whose accepted BuildPlans are requested.",
            "schema": { "type": "string", "format": "uuid" }
        }),
        json!({
            "name": "limit",
            "in": "query",
            "required": false,
            "schema": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_BUILD_PLAN_LIST_LIMIT,
                "default": DEFAULT_BUILD_PLAN_LIST_LIMIT
            }
        }),
    ]
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<String> {
    if method == "post" && is_build_plan_detection_path(path) && status == 200 {
        Some("BuildPlanDetectionSuccess200".into())
    } else if method == "post" && is_build_plan_collection_path(path) && matches!(status, 200 | 201)
    {
        Some(format!("BuildPlanMutationSuccess{status}"))
    } else if method == "get" && is_build_plan_collection_path(path) && status == 200 {
        Some("AcceptedBuildPlanListSuccess200".into())
    } else if method == "get" && is_build_plan_item_path(path) && status == 200 {
        Some("AcceptedBuildPlanSuccess200".into())
    } else {
        None
    }
}

fn is_route(path: &str, route: &str) -> bool {
    path.strip_prefix(DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX) == Some(route)
}

pub(super) fn is_build_plan_item_path(path: &str) -> bool {
    is_route(path, BUILD_PLAN_ITEM_ROUTE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_route(route: &str) -> String {
        format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}")
    }

    #[test]
    fn route_classification_is_exact_and_environment_bound() {
        for path in [
            full_route(BUILD_PLAN_DETECTION_ROUTE),
            full_route(BUILD_PLAN_COLLECTION_ROUTE),
            full_route(BUILD_PLAN_ITEM_ROUTE),
        ] {
            assert!(is_build_plan_path(&path), "missing BuildPlan route {path}");
        }
        for path in [
            BUILD_PLAN_DETECTION_ROUTE,
            "/organizations/{organization_id}/build-plans",
            "/organizations/{organization_id}/projects/{project_id}/build-plans",
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/build-plans/{other_id}",
        ] {
            assert!(!is_build_plan_path(path), "accepted foreign route {path}");
        }
    }

    #[test]
    fn collection_query_is_source_exact_and_bounded() {
        let parameters = query_parameters("get", &full_route(BUILD_PLAN_COLLECTION_ROUTE));
        assert_eq!(parameters.len(), 2);
        assert_eq!(parameters[0]["name"], "sourceRevisionId");
        assert_eq!(parameters[0]["required"], true);
        assert_eq!(parameters[0]["schema"]["format"], "uuid");
        assert_eq!(parameters[1]["name"], "limit");
        assert_eq!(
            parameters[1]["schema"]["maximum"],
            MAXIMUM_BUILD_PLAN_LIST_LIMIT
        );
        assert_eq!(
            parameters[1]["schema"]["default"],
            DEFAULT_BUILD_PLAN_LIST_LIMIT
        );
        assert!(query_parameters("post", &full_route(BUILD_PLAN_COLLECTION_ROUTE)).is_empty());
    }

    #[test]
    fn request_schemas_exist_only_for_the_two_json_post_routes() {
        let detection = request_schema(&full_route(BUILD_PLAN_DETECTION_ROUTE))
            .expect("detection request schema");
        assert_eq!(detection["required"], json!(["sourceRevisionId"]));
        let acceptance = request_schema(&full_route(BUILD_PLAN_COLLECTION_ROUTE))
            .expect("acceptance request schema");
        assert_eq!(
            acceptance["required"],
            json!(["sourceRevisionId", "proposalAcl"])
        );
        assert!(request_schema(&full_route(BUILD_PLAN_ITEM_ROUTE)).is_none());
    }

    #[test]
    fn response_components_follow_query_and_mutation_semantics() {
        let detection = full_route(BUILD_PLAN_DETECTION_ROUTE);
        let collection = full_route(BUILD_PLAN_COLLECTION_ROUTE);
        let item = full_route(BUILD_PLAN_ITEM_ROUTE);
        assert_eq!(
            success_component("post", &detection, 200).as_deref(),
            Some("BuildPlanDetectionSuccess200")
        );
        assert_eq!(
            success_component("get", &collection, 200).as_deref(),
            Some("AcceptedBuildPlanListSuccess200")
        );
        assert_eq!(
            success_component("get", &item, 200).as_deref(),
            Some("AcceptedBuildPlanSuccess200")
        );
        for status in [200, 201] {
            assert_eq!(
                success_component("post", &collection, status),
                Some(format!("BuildPlanMutationSuccess{status}"))
            );
        }
        assert_eq!(success_component("post", &detection, 201), None);
    }
}
