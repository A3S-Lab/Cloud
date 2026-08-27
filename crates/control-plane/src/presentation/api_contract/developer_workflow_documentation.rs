use super::developer_workflow_operation::{
    is_build_plan_collection_path, is_build_plan_detection_path, is_build_plan_item_path,
};

pub(super) fn component_description(name: &str) -> Option<&'static str> {
    match name {
        "BuildPlanSource" => Some(
            "Immutable source identity evidence acquired from the canonical Sources boundary.",
        ),
        "BuildRecipe" => Some(
            "Canonical Dockerfile build recipe published by Sources and embedded as typed evidence.",
        ),
        "BuildPlanProposal" => Some(
            "Deterministic reviewable BuildPlan proposal with canonical A3S ACL and detector evidence.",
        ),
        "BuildPlanDetectionDiagnostic" => Some(
            "Stable bounded diagnostic emitted while detecting BuildPlan proposals.",
        ),
        "BuildPlanDetection" => Some(
            "Canonical bounded proposal and diagnostic set for one exact immutable source layout.",
        ),
        "AcceptedBuildPlan" => Some(
            "Immutable accepted BuildPlan contract owned by Developer Workflows, with canonical ACL and typed evidence.",
        ),
        "AcceptedBuildPlanList" => Some(
            "Canonical bounded list of accepted BuildPlans for one exact SourceRevision.",
        ),
        "BuildPlanMutation" => Some(
            "Accepted immutable BuildPlan plus caller-owned idempotency replay state.",
        ),
        "BuildPlanDetectionSuccessResponse" => Some(
            "Standard success envelope containing a deterministic BuildPlan detection result.",
        ),
        "AcceptedBuildPlanSuccessResponse" => Some(
            "Standard success envelope containing one accepted immutable BuildPlan.",
        ),
        "AcceptedBuildPlanListSuccessResponse" => Some(
            "Standard success envelope containing a canonical bounded BuildPlan list.",
        ),
        "BuildPlanMutationSuccessResponse" => Some(
            "Standard success envelope containing BuildPlan acceptance and replay state.",
        ),
        _ => None,
    }
}

pub(super) fn operation_summary(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_build_plan_detection_path(path) {
        Some("Detect BuildPlan proposals")
    } else if method == "post" && is_build_plan_collection_path(path) {
        Some("Accept a BuildPlan proposal")
    } else if method == "get" && is_build_plan_collection_path(path) {
        Some("List accepted BuildPlans")
    } else if method == "get" && is_build_plan_item_path(path) {
        Some("Get an accepted BuildPlan")
    } else {
        None
    }
}

pub(super) fn operation_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_build_plan_detection_path(path) {
        Some(
            "Runs the built-in bounded deterministic detector set over the trusted layout of one immutable SourceRevision. The query returns canonical proposal ACL plus typed evidence and does not accept a plan or start a build.",
        )
    } else if method == "post" && is_build_plan_collection_path(path) {
        Some(
            "Accepts one caller-selected canonical BuildPlan proposal ACL after exact SourceRevision evidence and environment authorization checks. The immutable contract is persisted idempotently without starting a BuildRun, Workload, Route, or scheduler lifecycle.",
        )
    } else if method == "get" && is_build_plan_collection_path(path) {
        Some(
            "Lists a bounded canonical projection of immutable accepted BuildPlans for one exact SourceRevision through the Developer Workflows application read authority.",
        )
    } else if method == "get" && is_build_plan_item_path(path) {
        Some(
            "Returns one immutable accepted BuildPlan through the Developer Workflows application read authority after exact tenant, project, and environment authorization.",
        )
    } else {
        None
    }
}

pub(super) fn response_data_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_build_plan_detection_path(path) {
        Some("Canonical bounded proposals and diagnostics for the exact immutable source identity.")
    } else if method == "post" && is_build_plan_collection_path(path) {
        Some("The accepted immutable BuildPlan and caller-owned replay state.")
    } else if method == "get" && is_build_plan_collection_path(path) {
        Some("Canonical bounded accepted BuildPlans for the requested SourceRevision.")
    } else if method == "get" && is_build_plan_item_path(path) {
        Some("The authoritative immutable accepted BuildPlan projection.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::{
        BUILD_PLAN_COLLECTION_ROUTE, BUILD_PLAN_DETECTION_ROUTE, BUILD_PLAN_ITEM_ROUTE,
        DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX,
    };

    fn full_route(route: &str) -> String {
        format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}")
    }

    #[test]
    fn every_build_plan_operation_has_domain_specific_documentation() {
        for (method, path) in [
            ("post", full_route(BUILD_PLAN_DETECTION_ROUTE)),
            ("post", full_route(BUILD_PLAN_COLLECTION_ROUTE)),
            ("get", full_route(BUILD_PLAN_COLLECTION_ROUTE)),
            ("get", full_route(BUILD_PLAN_ITEM_ROUTE)),
        ] {
            assert!(operation_summary(method, &path).is_some());
            assert!(operation_description(method, &path).is_some());
            assert!(response_data_description(method, &path).is_some());
        }
    }

    #[test]
    fn every_build_plan_component_has_a_domain_description() {
        for name in [
            "BuildPlanSource",
            "BuildRecipe",
            "BuildPlanProposal",
            "BuildPlanDetectionDiagnostic",
            "BuildPlanDetection",
            "AcceptedBuildPlan",
            "AcceptedBuildPlanList",
            "BuildPlanMutation",
            "BuildPlanDetectionSuccessResponse",
            "AcceptedBuildPlanSuccessResponse",
            "AcceptedBuildPlanListSuccessResponse",
            "BuildPlanMutationSuccessResponse",
        ] {
            assert!(component_description(name).is_some(), "missing {name}");
        }
    }
}
