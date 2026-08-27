use super::developer_workflow_route::is_developer_workflow_route;
use super::preview_management_components::accept_preview_policy_request_schema;
use crate::modules::developer_workflows::{
    DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT, MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT,
    MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER, PULL_REQUEST_PREVIEW_ITEM_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE, PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE,
    PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE,
};
use serde_json::{json, Value};

pub(super) fn is_preview_management_path(path: &str) -> bool {
    is_preview_policy_collection_path(path)
        || is_preview_policy_item_path(path)
        || is_preview_policy_revision_collection_path(path)
        || is_preview_policy_revision_item_path(path)
        || is_pull_request_preview_item_path(path)
}

pub(super) fn is_preview_policy_collection_path(path: &str) -> bool {
    is_developer_workflow_route(path, PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE)
}

pub(super) fn is_preview_policy_item_path(path: &str) -> bool {
    is_developer_workflow_route(path, PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE)
}

pub(super) fn is_preview_policy_revision_collection_path(path: &str) -> bool {
    is_developer_workflow_route(path, PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE)
}

pub(super) fn is_preview_policy_revision_item_path(path: &str) -> bool {
    is_developer_workflow_route(path, PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE)
}

pub(super) fn is_pull_request_preview_item_path(path: &str) -> bool {
    is_developer_workflow_route(path, PULL_REQUEST_PREVIEW_ITEM_ROUTE)
}

pub(super) fn request_schema(path: &str) -> Option<Value> {
    is_preview_policy_collection_path(path).then(accept_preview_policy_request_schema)
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method != "get" || !is_preview_policy_revision_collection_path(path) {
        return Vec::new();
    }
    vec![json!({
        "name": "limit",
        "in": "query",
        "required": false,
        "description": "Maximum number of immutable Preview Policy revisions returned in ascending revision order.",
        "schema": {
            "type": "integer",
            "minimum": 1,
            "maximum": MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT,
            "default": DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT
        }
    })]
}

pub(super) fn path_parameter_schema(path: &str, name: &str) -> Option<Value> {
    (is_pull_request_preview_item_path(path) && name == "pull_request_id").then(|| {
        json!({
            "type": "integer",
            "format": "int64",
            "minimum": 1,
            "maximum": MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER
        })
    })
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<String> {
    if method == "post" && is_preview_policy_collection_path(path) && matches!(status, 200 | 201) {
        Some(format!("PullRequestPreviewPolicyMutationSuccess{status}"))
    } else if method == "get"
        && (is_preview_policy_item_path(path) || is_preview_policy_revision_item_path(path))
        && status == 200
    {
        Some("AcceptedPullRequestPreviewPolicyRevisionSuccess200".into())
    } else if method == "get" && is_preview_policy_revision_collection_path(path) && status == 200 {
        Some("AcceptedPullRequestPreviewPolicyRevisionListSuccess200".into())
    } else if method == "get" && is_pull_request_preview_item_path(path) && status == 200 {
        Some("PullRequestPreviewSuccess200".into())
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
            PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE,
            PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE,
            PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE,
            PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE,
            PULL_REQUEST_PREVIEW_ITEM_ROUTE,
        ] {
            assert!(is_preview_management_path(&full_route(route)));
        }
        for path in [
            PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE,
            "/organizations/{organization_id}/pull-request-preview-policies",
            "/organizations/{organization_id}/projects/{project_id}/pull-request-previews/{source_subscription_id}",
        ] {
            assert!(!is_preview_management_path(path), "accepted foreign route {path}");
        }
    }

    #[test]
    fn request_parameters_and_responses_follow_exact_resource_semantics() {
        let collection = full_route(PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE);
        let current = full_route(PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE);
        let revisions = full_route(PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE);
        let revision = full_route(PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE);
        let preview = full_route(PULL_REQUEST_PREVIEW_ITEM_ROUTE);

        assert_eq!(
            request_schema(&collection).expect("request")["required"],
            json!(["sourceSubscriptionId", "policyAcl"])
        );
        assert!(request_schema(&current).is_none());
        let parameters = query_parameters("get", &revisions);
        assert_eq!(
            parameters[0]["schema"]["maximum"],
            MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT
        );
        assert_eq!(
            parameters[0]["schema"]["default"],
            DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT
        );
        assert_eq!(
            path_parameter_schema(&preview, "pull_request_id").expect("PR ID schema")["maximum"],
            MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER
        );
        for status in [200, 201] {
            assert_eq!(
                success_component("post", &collection, status),
                Some(format!("PullRequestPreviewPolicyMutationSuccess{status}"))
            );
        }
        for path in [current, revision] {
            assert_eq!(
                success_component("get", &path, 200).as_deref(),
                Some("AcceptedPullRequestPreviewPolicyRevisionSuccess200")
            );
        }
        assert_eq!(
            success_component("get", &revisions, 200).as_deref(),
            Some("AcceptedPullRequestPreviewPolicyRevisionListSuccess200")
        );
        assert_eq!(
            success_component("get", &preview, 200).as_deref(),
            Some("PullRequestPreviewSuccess200")
        );
    }
}
