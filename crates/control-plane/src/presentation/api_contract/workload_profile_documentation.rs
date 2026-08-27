use super::workload_profile_operation::{
    is_workload_profile_collection_path, is_workload_profile_item_path,
    is_workload_profile_revision_collection_path, is_workload_profile_revision_item_path,
};

pub(super) fn component_description(name: &str) -> Option<&'static str> {
    match name {
        "WorkloadProcess" => Some(
            "Developer Workflows-owned process intent preserved independently of Workloads and Executions admission models.",
        ),
        "WorkloadSecretEnvironmentTarget" => Some(
            "Secret reference target that injects a version-pinned Secret into one environment variable without exposing its value.",
        ),
        "WorkloadSecretFileTarget" => Some(
            "Secret reference target that materializes a version-pinned Secret at one absolute path and file mode without exposing its value.",
        ),
        "WorkloadSecretRegistryCredentialTarget" => Some(
            "Secret reference target used only as an opaque registry credential binding.",
        ),
        "WorkloadSecretTarget" => Some(
            "Closed Secret target union discriminated by `kind`; it contains references and placement intent only.",
        ),
        "WorkloadSecretBinding" => Some(
            "Version-pinned Secret reference and target owned by the accepted WorkloadProfile contract; Secret material is never returned.",
        ),
        "WorkloadProfileResources" => Some(
            "Closed CPU, memory, process, ephemeral-storage, and optional scheduled-execution timeout intent.",
        ),
        "WorkloadServicePort" => Some(
            "Named container port intent used by service health and optional public-port selection.",
        ),
        "WorkloadHttpHealthCheck" => Some(
            "Bounded HTTP health intent over one named WorkloadProfile service port.",
        ),
        "ScheduledTaskRetryPolicy" => Some(
            "Bounded retry intent for a scheduled Task; it is not a scheduler or retry lifecycle.",
        ),
        "ScheduledTaskHistoryPolicy" => Some(
            "Bounded successful, failed, and maximum-age retention intent for scheduled Task history.",
        ),
        "ScheduledTaskSchedule" => Some(
            "Canonical seven-field cron, IANA timezone, catch-up, concurrency, misfire, retry, and history intent.",
        ),
        "WorkloadProfileSpec" => Some(
            "Closed typed `web`, `worker`, or `scheduled_task` intent embedded in an immutable accepted revision.",
        ),
        "AcceptedWorkloadProfileRevision" => Some(
            "Immutable Developer Workflows-owned revision bound to one accepted BuildPlan, SourceRevision, canonical A3S ACL, actor, and acceptance time.",
        ),
        "AcceptedWorkloadProfileRevisionList" => Some(
            "Bounded canonical ascending history of immutable revisions for one logical WorkloadProfile.",
        ),
        "WorkloadProfileMutation" => Some(
            "Accepted immutable WorkloadProfile revision plus caller-owned idempotency replay state.",
        ),
        "AcceptedWorkloadProfileRevisionSuccessResponse" => Some(
            "Standard success envelope containing one immutable accepted WorkloadProfile revision.",
        ),
        "AcceptedWorkloadProfileRevisionListSuccessResponse" => Some(
            "Standard success envelope containing one bounded canonical WorkloadProfile revision history.",
        ),
        "WorkloadProfileMutationSuccessResponse" => Some(
            "Standard success envelope containing WorkloadProfile acceptance and replay state.",
        ),
        _ => None,
    }
}

pub(super) fn operation_summary(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_workload_profile_collection_path(path) {
        Some("Accept a WorkloadProfile revision")
    } else if method == "get" && is_workload_profile_item_path(path) {
        Some("Get the current WorkloadProfile revision")
    } else if method == "get" && is_workload_profile_revision_collection_path(path) {
        Some("List WorkloadProfile revisions")
    } else if method == "get" && is_workload_profile_revision_item_path(path) {
        Some("Get a WorkloadProfile revision")
    } else {
        None
    }
}

pub(super) fn operation_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_workload_profile_collection_path(path) {
        Some(
            "Accepts one canonical `a3s.cloud.workload-profile.v1` A3S ACL after exact BuildPlan binding and environment authorization. The immutable revision is persisted idempotently without starting a BuildRun, Workload, Execution, Route, Operation, or scheduler lifecycle.",
        )
    } else if method == "get" && is_workload_profile_item_path(path) {
        Some(
            "Returns the current immutable revision of one logical WorkloadProfile through the sole Developer Workflows application read authority after exact tenant, project, and environment authorization.",
        )
    } else if method == "get" && is_workload_profile_revision_collection_path(path) {
        Some(
            "Lists a bounded canonical ascending history of immutable accepted revisions for one logical WorkloadProfile through the sole application read authority.",
        )
    } else if method == "get" && is_workload_profile_revision_item_path(path) {
        Some(
            "Returns one exact immutable WorkloadProfile revision through the sole application read authority after exact scope authorization.",
        )
    } else {
        None
    }
}

pub(super) fn response_data_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "post" && is_workload_profile_collection_path(path) {
        Some("The accepted immutable WorkloadProfile revision and caller-owned replay state.")
    } else if method == "get" && is_workload_profile_item_path(path) {
        Some("The authoritative current immutable WorkloadProfile revision.")
    } else if method == "get" && is_workload_profile_revision_collection_path(path) {
        Some("The bounded canonical ascending WorkloadProfile revision history.")
    } else if method == "get" && is_workload_profile_revision_item_path(path) {
        Some("The authoritative exact immutable WorkloadProfile revision.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::{
        DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX, WORKLOAD_PROFILE_COLLECTION_ROUTE,
        WORKLOAD_PROFILE_ITEM_ROUTE, WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE,
        WORKLOAD_PROFILE_REVISION_ITEM_ROUTE,
    };

    fn full_route(route: &str) -> String {
        format!("{DEVELOPER_WORKFLOWS_CONTROLLER_PREFIX}{route}")
    }

    #[test]
    fn every_workload_profile_operation_has_domain_specific_documentation() {
        for (method, path) in [
            ("post", full_route(WORKLOAD_PROFILE_COLLECTION_ROUTE)),
            ("get", full_route(WORKLOAD_PROFILE_ITEM_ROUTE)),
            (
                "get",
                full_route(WORKLOAD_PROFILE_REVISION_COLLECTION_ROUTE),
            ),
            ("get", full_route(WORKLOAD_PROFILE_REVISION_ITEM_ROUTE)),
        ] {
            assert!(operation_summary(method, &path).is_some());
            assert!(operation_description(method, &path).is_some());
            assert!(response_data_description(method, &path).is_some());
        }
    }

    #[test]
    fn every_workload_profile_component_has_a_domain_description() {
        for name in [
            "WorkloadProcess",
            "WorkloadSecretEnvironmentTarget",
            "WorkloadSecretFileTarget",
            "WorkloadSecretRegistryCredentialTarget",
            "WorkloadSecretTarget",
            "WorkloadSecretBinding",
            "WorkloadProfileResources",
            "WorkloadServicePort",
            "WorkloadHttpHealthCheck",
            "ScheduledTaskRetryPolicy",
            "ScheduledTaskHistoryPolicy",
            "ScheduledTaskSchedule",
            "WorkloadProfileSpec",
            "AcceptedWorkloadProfileRevision",
            "AcceptedWorkloadProfileRevisionList",
            "WorkloadProfileMutation",
            "AcceptedWorkloadProfileRevisionSuccessResponse",
            "AcceptedWorkloadProfileRevisionListSuccessResponse",
            "WorkloadProfileMutationSuccessResponse",
        ] {
            assert!(component_description(name).is_some(), "missing {name}");
        }
    }
}
