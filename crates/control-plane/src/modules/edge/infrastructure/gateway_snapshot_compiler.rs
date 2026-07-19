use crate::modules::edge::domain::{Route, RouteState};
use crate::modules::shared_kernel::domain::NodeId;
use a3s_cloud_contracts::GatewaySnapshot;
use std::collections::BTreeSet;
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewaySnapshotCompilerConfig {
    pub entrypoint_address: String,
    pub management_address: String,
    pub management_path_prefix: String,
    pub management_auth_token_env: String,
    pub upstream_request_timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct GatewaySnapshotCompiler {
    config: GatewaySnapshotCompilerConfig,
}

impl GatewaySnapshotCompiler {
    pub fn new(config: GatewaySnapshotCompilerConfig) -> Result<Self, String> {
        let entrypoint = config
            .entrypoint_address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid Gateway entrypoint address: {error}"))?;
        let management = config
            .management_address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid Gateway management address: {error}"))?;
        if entrypoint.port() == 0
            || management.port() == 0
            || !management.ip().is_loopback()
            || !valid_path_prefix(&config.management_path_prefix)
            || !valid_environment_name(&config.management_auth_token_env)
            || config.upstream_request_timeout_ms == 0
            || config.upstream_request_timeout_ms > 3_600_000
        {
            return Err("Gateway snapshot compiler configuration is invalid".into());
        }
        Ok(Self { config })
    }

    pub fn compile(
        &self,
        node_id: NodeId,
        revision: u64,
        expected_revision: Option<u64>,
        routes: &[Route],
    ) -> Result<GatewaySnapshot, String> {
        let mut routes = routes.iter().collect::<Vec<_>>();
        routes.sort_by(|left, right| {
            (left.hostname.as_str(), left.path_prefix.as_str(), left.id).cmp(&(
                right.hostname.as_str(),
                right.path_prefix.as_str(),
                right.id,
            ))
        });
        let mut ownership = BTreeSet::new();
        for route in &routes {
            if route.gateway_node_id != node_id {
                return Err("complete Gateway snapshot contains a route from another scope".into());
            }
            if !matches!(route.state, RouteState::Pending | RouteState::Active) {
                return Err("complete Gateway snapshot contains an ineligible route state".into());
            }
            if !ownership.insert((route.hostname.as_str(), route.path_prefix.as_str())) {
                return Err("Gateway route ownership is not unique within the scope".into());
            }
        }

        let mut acl = format!(
            "# a3s-cloud complete Gateway snapshot {revision}\nentrypoints \"a3s-cloud-http\" {{\n  address = {}\n}}\n\n",
            acl_string(&self.config.entrypoint_address)
        );
        for route in &routes {
            let name = format!("route-{}", route.id.as_uuid().simple());
            acl.push_str(&format!(
                "routers \"{name}\" {{\n  rule = {}\n  service = \"{name}\"\n  entrypoints = [\"a3s-cloud-http\"]\n}}\n\nservices \"{name}\" {{\n  load_balancer {{\n    strategy = \"round-robin\"\n    request_timeout = {}\n    servers = [{{ url = {} }}]\n  }}\n}}\n\n",
                acl_string(&format!(
                    "Host(`{}`) && PathPrefix(`{}`)",
                    route.hostname.as_str(),
                    route.path_prefix.as_str()
                )),
                acl_string(&duration(self.config.upstream_request_timeout_ms)),
                acl_string(route.upstream.as_str()),
            ));
        }
        acl.push_str(&format!(
            "management {{\n  enabled = true\n  address = {}\n  path_prefix = {}\n  auth_token_env = {}\n  allowed_ips = [\"127.0.0.1\", \"::1\"]\n}}\n",
            acl_string(&self.config.management_address),
            acl_string(&self.config.management_path_prefix),
            acl_string(&self.config.management_auth_token_env),
        ));
        GatewaySnapshot::new(revision, expected_revision, acl)
    }
}

fn acl_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn duration(milliseconds: u64) -> String {
    if milliseconds % 1_000 == 0 {
        format!("{}s", milliseconds / 1_000)
    } else {
        format!("{milliseconds}ms")
    }
}

fn valid_path_prefix(value: &str) -> bool {
    value.starts_with('/') && value.len() <= 255 && !value.contains(['\0', '\r', '\n', '?', '#'])
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || index > 0 && byte.is_ascii_digit()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{RouteHostname, RoutePath, RoutePortName, UpstreamEndpoint};
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use chrono::Utc;

    fn compiler() -> GatewaySnapshotCompiler {
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
        })
        .expect("compiler")
    }

    fn route(node_id: NodeId, hostname: &str, path: &str, port: u16) -> Route {
        Route::create(
            RouteId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            node_id,
            RouteHostname::parse(hostname).expect("hostname"),
            RoutePath::parse(path).expect("path"),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}")).expect("upstream"),
            Utc::now(),
        )
    }

    #[test]
    fn compiles_every_owned_route_into_one_deterministic_snapshot() {
        let node_id = NodeId::new();
        let first = route(node_id, "z.example.com", "/", 49152);
        let second = route(node_id, "api.example.com", "/v1", 49153);
        let forward = compiler()
            .compile(node_id, 2, Some(1), &[first.clone(), second.clone()])
            .expect("snapshot");
        let reverse = compiler()
            .compile(node_id, 2, Some(1), &[second, first])
            .expect("snapshot");
        assert_eq!(forward, reverse);
        assert_eq!(forward.acl.matches("routers \"").count(), 2);
        assert_eq!(forward.acl.matches("services \"").count(), 2);
        assert!(forward
            .acl
            .contains("Host(`api.example.com`) && PathPrefix(`/v1`)"));
        assert!(forward.acl.contains("http://127.0.0.1:49152/"));
    }

    #[test]
    fn rejects_cross_scope_and_duplicate_route_ownership() {
        let node_id = NodeId::new();
        let first = route(node_id, "api.example.com", "/v1", 49152);
        let duplicate = route(node_id, "api.example.com", "/v1", 49153);
        assert!(compiler()
            .compile(node_id, 1, None, &[first, duplicate])
            .is_err());
        let foreign = route(NodeId::new(), "other.example.com", "/", 49154);
        assert!(compiler().compile(node_id, 1, None, &[foreign]).is_err());
    }

    #[test]
    fn installed_gateway_validates_compiled_snapshot() {
        let Ok(binary) = std::env::var("A3S_CLOUD_TEST_GATEWAY_BIN") else {
            return;
        };
        let node_id = NodeId::new();
        let snapshot = compiler()
            .compile(
                node_id,
                1,
                None,
                &[route(node_id, "api.example.com", "/v1", 49152)],
            )
            .expect("snapshot");
        let directory = tempfile::tempdir().expect("Gateway validation directory");
        let path = directory.path().join("gateway.acl");
        std::fs::write(&path, snapshot.acl).expect("write compiled Gateway snapshot");
        let output = std::process::Command::new(binary)
            .arg("validate")
            .arg("--config")
            .arg(path)
            .output()
            .expect("run installed Gateway validator");
        assert!(
            output.status.success(),
            "installed Gateway rejected compiled snapshot: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
