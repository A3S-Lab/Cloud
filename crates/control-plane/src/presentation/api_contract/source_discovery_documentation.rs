use super::source_discovery_operation::{
    is_reference_discovery_path, is_repository_discovery_path,
};

pub(super) fn component_description(name: &str) -> Option<&'static str> {
    match name {
        "GithubSourceRepository" => Some(
            "Canonical GitHub repository identity owned by Sources; it contains no provider credential.",
        ),
        "GithubDiscoveredRepository" => Some(
            "Transient installation-accessible GitHub repository projection revalidated by Sources.",
        ),
        "GithubRepositoryDiscoveryPage" => Some(
            "Bounded policy- and value-filtered page of repositories visible to the authoritative GitHub App installation.",
        ),
        "GithubDiscoveredBranch" => Some(
            "Transient GitHub branch projection with exact commit identity and protection state.",
        ),
        "GithubDiscoveredTag" => Some(
            "Transient GitHub tag projection with exact commit identity and an explicitly null protection state.",
        ),
        "GithubDiscoveredReference" => Some(
            "Closed branch-or-tag discovery projection discriminated by reference kind.",
        ),
        "GithubRepositoryReferenceDiscoveryPage" => Some(
            "Bounded page of one repository's branches or tags admitted by the existing Sources reference rules.",
        ),
        "GithubRepositoryDiscoveryPageSuccessResponse" => Some(
            "Standard success envelope containing a policy-filtered GitHub repository discovery page.",
        ),
        "GithubRepositoryReferenceDiscoveryPageSuccessResponse" => Some(
            "Standard success envelope containing a GitHub branch or tag discovery page.",
        ),
        _ => None,
    }
}

pub(super) fn operation_summary(method: &str, path: &str) -> Option<&'static str> {
    if method == "get" && is_repository_discovery_path(path) {
        Some("List installation repositories")
    } else if method == "get" && is_reference_discovery_path(path) {
        Some("List repository branches or tags")
    } else {
        None
    }
}

pub(super) fn operation_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "get" && is_repository_discovery_path(path) {
        Some(
            "Lists a bounded transient projection of repositories visible to the organization's authoritative GitHub App installation. Sources revalidates installation authority, silently removes repositories denied by policy or incompatible with its existing repository/reference value objects, and never returns or persists the short-lived provider token.",
        )
    } else if method == "get" && is_reference_discovery_path(path) {
        Some(
            "Lists a bounded transient branch or tag projection for one canonical policy-admitted GitHub repository. Sources revalidates installation authority and provider identities, excludes names that its existing reference value object cannot accept, and keeps the repository-scoped token inside the infrastructure adapter.",
        )
    } else {
        None
    }
}

pub(super) fn response_data_description(method: &str, path: &str) -> Option<&'static str> {
    if method == "get" && is_repository_discovery_path(path) {
        Some(
            "A bounded policy-filtered repository page and opaque scope-bound continuation cursor.",
        )
    } else if method == "get" && is_reference_discovery_path(path) {
        Some("A bounded branch or tag page for the exact canonical repository and an opaque scope-bound continuation cursor.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::sources::{
        GITHUB_REPOSITORY_DISCOVERY_ROUTE, GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE,
        SOURCES_CONTROLLER_PREFIX,
    };

    #[test]
    fn every_source_discovery_operation_and_component_is_domain_documented() {
        for path in [
            format!("{SOURCES_CONTROLLER_PREFIX}{GITHUB_REPOSITORY_DISCOVERY_ROUTE}"),
            format!("{SOURCES_CONTROLLER_PREFIX}{GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE}"),
        ] {
            assert!(operation_summary("get", &path).is_some());
            assert!(operation_description("get", &path).is_some());
            assert!(response_data_description("get", &path).is_some());
        }
        for name in [
            "GithubSourceRepository",
            "GithubDiscoveredRepository",
            "GithubRepositoryDiscoveryPage",
            "GithubDiscoveredBranch",
            "GithubDiscoveredTag",
            "GithubDiscoveredReference",
            "GithubRepositoryReferenceDiscoveryPage",
            "GithubRepositoryDiscoveryPageSuccessResponse",
            "GithubRepositoryReferenceDiscoveryPageSuccessResponse",
        ] {
            assert!(component_description(name).is_some(), "missing {name}");
        }
    }
}
