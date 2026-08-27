use super::preview_management_operation::{
    is_preview_policy_collection_path, is_preview_policy_item_path,
    is_preview_policy_revision_collection_path, is_preview_policy_revision_item_path,
    is_pull_request_preview_item_path,
};

pub(super) fn component_description(name: &str) -> Option<&'static str> {
    match name {
        "PreviewGitRepository" => Some(
            "Canonical public GitHub repository identity; it contains no installation credential or checkout state.",
        ),
        "PreviewQuota" => Some(
            "Closed bounded Workload count, CPU, memory, and ephemeral-storage intent for one Preview.",
        ),
        "PullRequestPreviewPolicy" => Some(
            "Behavioral pull-request Preview policy bound to one Sources subscription and immutable accepted revision.",
        ),
        "AcceptedPullRequestPreviewPolicyRevision" => Some(
            "Immutable Developer Workflows-owned Preview Policy revision with canonical A3S ACL, digest, actor, and acceptance time.",
        ),
        "AcceptedPullRequestPreviewPolicyRevisionList" => Some(
            "Bounded canonical ascending history of immutable Preview Policy revisions for one Sources subscription.",
        ),
        "PullRequestPreviewPolicyMutation" => Some(
            "Accepted immutable Preview Policy revision plus caller-owned idempotency replay state.",
        ),
        "PullRequestPreview" => Some(
            "Current Developer Workflows-owned behavioral lifecycle projection for one pull request under its immutable policy authority.",
        ),
        "AcceptedPullRequestPreviewPolicyRevisionSuccessResponse" => Some(
            "Standard success envelope containing one immutable accepted Preview Policy revision.",
        ),
        "AcceptedPullRequestPreviewPolicyRevisionListSuccessResponse" => Some(
            "Standard success envelope containing one bounded canonical Preview Policy revision history.",
        ),
        "PullRequestPreviewPolicyMutationSuccessResponse" => Some(
            "Standard success envelope containing Preview Policy acceptance and replay state.",
        ),
        "PullRequestPreviewSuccessResponse" => Some(
            "Standard success envelope containing one current pull-request Preview projection.",
        ),
        _ => None,
    }
}

pub(super) fn operation_summary(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_preview_policy_collection_path(path) {
        Some("Accept a pull-request Preview Policy revision")
    } else if method == "get" && is_preview_policy_item_path(path) {
        Some("Get the current pull-request Preview Policy revision")
    } else if method == "get" && is_preview_policy_revision_collection_path(path) {
        Some("List pull-request Preview Policy revisions")
    } else if method == "get" && is_preview_policy_revision_item_path(path) {
        Some("Get a pull-request Preview Policy revision")
    } else if method == "get" && is_pull_request_preview_item_path(path) {
        Some("Get a pull-request Preview")
    } else {
        None
    }
}

pub(super) fn operation_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_preview_policy_collection_path(path) {
        Some(
            "Accepts one canonical `a3s.cloud.pull-request-preview-policy.v1` A3S ACL after exact Sources subscription binding and environment authorization. It persists one immutable policy revision idempotently without creating a webhook, queue, Preview Environment, BuildRun, Workload, Route, or cleanup lifecycle.",
        )
    } else if method == "get" && is_preview_policy_item_path(path) {
        Some(
            "Returns the current immutable Preview Policy revision for one exact Sources subscription through the sole Developer Workflows policy read authority.",
        )
    } else if method == "get" && is_preview_policy_revision_collection_path(path) {
        Some(
            "Lists a bounded canonical ascending history of immutable Preview Policy revisions through the sole Developer Workflows policy read authority.",
        )
    } else if method == "get" && is_preview_policy_revision_item_path(path) {
        Some(
            "Returns one exact immutable Preview Policy revision after exact tenant, project, source-environment, and subscription authorization.",
        )
    } else if method == "get" && is_pull_request_preview_item_path(path) {
        Some(
            "Returns the current behavioral Preview projection for one exact Sources subscription and pull-request identity. Credentials, webhook signatures, delivery payloads, and runtime internals remain outside this interface.",
        )
    } else {
        None
    }
}

pub(super) fn response_data_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_preview_policy_collection_path(path) {
        Some("The accepted immutable Preview Policy revision and caller-owned replay state.")
    } else if method == "get" && is_preview_policy_item_path(path) {
        Some("The authoritative current immutable Preview Policy revision.")
    } else if method == "get" && is_preview_policy_revision_collection_path(path) {
        Some("The bounded canonical ascending Preview Policy revision history.")
    } else if method == "get" && is_preview_policy_revision_item_path(path) {
        Some("The authoritative exact immutable Preview Policy revision.")
    } else if method == "get" && is_pull_request_preview_item_path(path) {
        Some("The authoritative current behavioral pull-request Preview projection.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::{
        DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX, PULL_REQUEST_PREVIEW_ITEM_ROUTE,
        PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE, PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE,
        PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE,
        PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE,
    };

    fn full_route(route: &str) -> String {
        format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}")
    }

    #[test]
    fn every_preview_management_operation_has_domain_specific_documentation() {
        for (method, route) in [
            ("post", PULL_REQUEST_PREVIEW_POLICY_COLLECTION_ROUTE),
            ("get", PULL_REQUEST_PREVIEW_POLICY_ITEM_ROUTE),
            ("get", PULL_REQUEST_PREVIEW_POLICY_REVISION_COLLECTION_ROUTE),
            ("get", PULL_REQUEST_PREVIEW_POLICY_REVISION_ITEM_ROUTE),
            ("get", PULL_REQUEST_PREVIEW_ITEM_ROUTE),
        ] {
            let path = full_route(route);
            assert!(operation_summary(method, &path).is_some());
            assert!(operation_description(method, &path).is_some());
            assert!(response_data_description(method, &path).is_some());
        }
    }

    #[test]
    fn every_preview_management_component_has_a_domain_description() {
        for name in [
            "PreviewGitRepository",
            "PreviewQuota",
            "PullRequestPreviewPolicy",
            "AcceptedPullRequestPreviewPolicyRevision",
            "AcceptedPullRequestPreviewPolicyRevisionList",
            "PullRequestPreviewPolicyMutation",
            "PullRequestPreview",
            "AcceptedPullRequestPreviewPolicyRevisionSuccessResponse",
            "AcceptedPullRequestPreviewPolicyRevisionListSuccessResponse",
            "PullRequestPreviewPolicyMutationSuccessResponse",
            "PullRequestPreviewSuccessResponse",
        ] {
            assert!(component_description(name).is_some(), "missing {name}");
        }
    }
}
