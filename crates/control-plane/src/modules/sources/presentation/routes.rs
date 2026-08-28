pub(crate) const SOURCES_CONTROLLER_PREFIX: &str = "/organizations";

pub(crate) const GITHUB_SOURCE_CONNECTION_ROUTE: &str =
    "/{organization_id}/source-connections/github";

pub(crate) const GITHUB_REPOSITORY_DISCOVERY_ROUTE: &str =
    "/{organization_id}/source-connections/github/repositories";

pub(crate) const GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE: &str =
    "/{organization_id}/source-connections/github/repository-references";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_source_discovery_routes_share_the_connection_boundary() {
        for route in [
            GITHUB_REPOSITORY_DISCOVERY_ROUTE,
            GITHUB_REPOSITORY_REFERENCE_DISCOVERY_ROUTE,
        ] {
            assert!(route.starts_with(GITHUB_SOURCE_CONNECTION_ROUTE));
            assert!(!format!("{SOURCES_CONTROLLER_PREFIX}{route}").contains("//"));
        }
    }
}
