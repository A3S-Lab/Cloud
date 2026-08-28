use super::source_components::github_repository_url_schema;
use crate::modules::sources::{
    DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE, GITHUB_REPOSITORY_DISCOVERY_ROUTE,
    GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE, GITHUB_SOURCE_DISCOVERY_CURSOR_PATTERN,
    MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES, MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
    SOURCES_CONTROLLER_PREFIX,
};
use serde_json::{json, Value};

pub(super) fn is_repository_discovery_path(path: &str) -> bool {
    is_sources_route(path, GITHUB_REPOSITORY_DISCOVERY_ROUTE)
}

pub(super) fn is_reference_discovery_path(path: &str) -> bool {
    is_sources_route(path, GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE)
}

pub(super) fn is_source_discovery_path(path: &str) -> bool {
    is_repository_discovery_path(path) || is_reference_discovery_path(path)
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method != "get" || !is_source_discovery_path(path) {
        return Vec::new();
    }
    let mut parameters = Vec::new();
    if is_reference_discovery_path(path) {
        parameters.extend([
            json!({
                "name": "repositoryUrl",
                "in": "query",
                "required": true,
                "description": "Canonical GitHub repository URL admitted by the Sources repository policy.",
                "schema": github_repository_url_schema()
            }),
            json!({
                "name": "kind",
                "in": "query",
                "required": true,
                "description": "Exact Git reference collection to discover.",
                "schema": { "type": "string", "enum": ["branch", "tag"] }
            }),
        ]);
    }
    parameters.extend([
        json!({
            "name": "cursor",
            "in": "query",
            "required": false,
            "description": "Opaque connection-, query-, and page-size-bound continuation cursor.",
            "schema": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES,
                "x-a3s-max-utf8-bytes": MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES,
                "pattern": GITHUB_SOURCE_DISCOVERY_CURSOR_PATTERN
            }
        }),
        json!({
            "name": "limit",
            "in": "query",
            "required": false,
            "description": "Maximum provider projections inspected in this page before Sources policy filtering.",
            "schema": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
                "default": DEFAULT_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE
            }
        }),
    ]);
    parameters
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<&'static str> {
    if method != "get" || status != 200 {
        None
    } else if is_repository_discovery_path(path) {
        Some("GithubRepositoryDiscoveryPageSuccess200")
    } else if is_reference_discovery_path(path) {
        Some("GithubRepositoryReferenceDiscoveryPageSuccess200")
    } else {
        None
    }
}

fn is_sources_route(path: &str, route: &str) -> bool {
    path.strip_prefix(SOURCES_CONTROLLER_PREFIX) == Some(route)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_route(route: &str) -> String {
        format!("{SOURCES_CONTROLLER_PREFIX}{route}")
    }

    #[test]
    fn source_discovery_routes_are_exact_and_have_bounded_queries() {
        let repositories = full_route(GITHUB_REPOSITORY_DISCOVERY_ROUTE);
        let references = full_route(GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE);
        assert!(is_repository_discovery_path(&repositories));
        assert!(is_reference_discovery_path(&references));
        assert!(!is_source_discovery_path(GITHUB_REPOSITORY_DISCOVERY_ROUTE));
        assert!(!is_source_discovery_path(
            "/organizations/{organization_id}/source-connections/github/foreign"
        ));

        let repository_parameters = query_parameters("get", &repositories);
        assert_eq!(repository_parameters.len(), 2);
        assert_eq!(repository_parameters[0]["name"], "cursor");
        assert_eq!(
            repository_parameters[1]["schema"]["maximum"],
            MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE
        );
        let reference_parameters = query_parameters("get", &references);
        assert_eq!(reference_parameters.len(), 4);
        assert_eq!(reference_parameters[0]["name"], "repositoryUrl");
        assert_eq!(reference_parameters[1]["name"], "kind");
        assert_eq!(reference_parameters[2]["name"], "cursor");
        assert_eq!(reference_parameters[3]["name"], "limit");
        assert!(query_parameters("post", &references).is_empty());
    }

    #[test]
    fn source_discovery_success_components_are_route_specific() {
        assert_eq!(
            success_component("get", &full_route(GITHUB_REPOSITORY_DISCOVERY_ROUTE), 200,),
            Some("GithubRepositoryDiscoveryPageSuccess200")
        );
        assert_eq!(
            success_component(
                "get",
                &full_route(GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE),
                200,
            ),
            Some("GithubRepositoryReferenceDiscoveryPageSuccess200")
        );
        assert_eq!(
            success_component("post", &full_route(GITHUB_REPOSITORY_DISCOVERY_ROUTE), 200,),
            None
        );
    }
}
