use crate::modules::edge::domain::{Route, RouteState};
use crate::modules::shared_kernel::domain::{GatewayCertificateId, NodeId};
use a3s_cloud_contracts::{GatewayCertificateRequest, GatewaySnapshot};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewaySnapshotCompilerConfig {
    pub entrypoint_address: String,
    pub management_address: String,
    pub management_path_prefix: String,
    pub management_auth_token_env: String,
    pub upstream_request_timeout_ms: u64,
    pub certificate_directory: String,
    pub managed_state_file: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewaySnapshotMetadata {
    pub node_id: NodeId,
    pub revision: u64,
    pub expected_revision: Option<u64>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl GatewaySnapshotMetadata {
    pub const fn new(
        node_id: NodeId,
        revision: u64,
        expected_revision: Option<u64>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            node_id,
            revision,
            expected_revision,
            issued_at,
            expires_at,
        }
    }
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
            || !valid_certificate_directory(&config.certificate_directory)
            || !valid_absolute_file(&config.managed_state_file)
        {
            return Err("Gateway snapshot compiler configuration is invalid".into());
        }
        Ok(Self { config })
    }

    pub fn compile(
        &self,
        metadata: GatewaySnapshotMetadata,
        certificate_id: GatewayCertificateId,
        routes: &[Route],
    ) -> Result<GatewaySnapshot, String> {
        self.compile_snapshot(metadata, Some(certificate_id), routes, true)
    }

    pub fn compile_certificate_convergence(
        &self,
        metadata: GatewaySnapshotMetadata,
        certificate_id: Option<GatewayCertificateId>,
        routes: &[Route],
    ) -> Result<GatewaySnapshot, String> {
        if routes.is_empty() != certificate_id.is_none() {
            return Err(
                "Gateway certificate convergence requires one certificate for non-empty routes"
                    .into(),
            );
        }
        self.compile_snapshot(metadata, certificate_id, routes, false)
    }

    fn compile_snapshot(
        &self,
        metadata: GatewaySnapshotMetadata,
        certificate_id: Option<GatewayCertificateId>,
        routes: &[Route],
        require_pending_route: bool,
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
        let mut dns_names = BTreeSet::new();
        let mut pending_routes = 0_usize;
        for route in &routes {
            if route.gateway_node_id != metadata.node_id {
                return Err("complete Gateway snapshot contains a route from another scope".into());
            }
            let state_is_eligible = if require_pending_route {
                matches!(route.state, RouteState::Pending | RouteState::Active)
            } else {
                route.state == RouteState::Active
            };
            if !state_is_eligible {
                return Err("complete Gateway snapshot contains an ineligible route state".into());
            }
            if !ownership.insert((route.hostname.as_str(), route.path_prefix.as_str())) {
                return Err("Gateway route ownership is not unique within the scope".into());
            }
            let Some(pattern) = route.domain_pattern.as_ref() else {
                return Err(
                    "complete Gateway snapshot contains a route without domain proof".into(),
                );
            };
            if route.domain_claim_id.is_none() || route.gateway_certificate_id.is_none() {
                return Err("complete Gateway snapshot contains incomplete TLS ownership".into());
            }
            if !pattern.covers(&route.hostname) {
                return Err("Gateway route hostname is outside its verified domain pattern".into());
            }
            dns_names.insert(pattern.as_str().to_owned());
            if route.state == RouteState::Pending {
                pending_routes += 1;
                if route.gateway_certificate_id != certificate_id {
                    return Err(
                        "pending Gateway route does not reference the snapshot certificate".into(),
                    );
                }
            }
        }
        if require_pending_route && pending_routes == 0 {
            return Err("complete Gateway publication must contain a pending route".into());
        }

        let certificate_request = certificate_id
            .map(|certificate_id| {
                let certificate_root =
                    Path::new(&self.config.certificate_directory).join(certificate_id.to_string());
                let certificate_file = certificate_root
                    .join("certificate.pem")
                    .to_string_lossy()
                    .into_owned();
                let private_key_file = certificate_root
                    .join("private-key.pem")
                    .to_string_lossy()
                    .into_owned();
                GatewayCertificateRequest::new(
                    certificate_id.as_uuid(),
                    dns_names.into_iter().collect(),
                    certificate_file,
                    private_key_file,
                )
            })
            .transpose()?;
        let mut acl = format!(
            "# a3s-cloud complete Gateway snapshot {revision}\n\
             mode {{ kind = \"cloud-managed\" }}\n\n\
             managed {{\n  gateway_id = {}\n  state_file = {}\n}}\n\n",
            acl_string(&metadata.node_id.to_string()),
            acl_string(&self.config.managed_state_file),
            revision = metadata.revision,
        );
        if let Some(certificate_request) = &certificate_request {
            acl.push_str(&format!(
                "entrypoints \"a3s-cloud-https\" {{\n  address = {}\n  tls {{\n    cert_file = {}\n    key_file = {}\n    min_version = \"1.2\"\n  }}\n}}\n\n",
                acl_string(&self.config.entrypoint_address),
                acl_string(&certificate_request.certificate_file),
                acl_string(&certificate_request.private_key_file),
            ));
        }
        for route in &routes {
            let name = format!("route-{}", route.id.as_uuid().simple());
            acl.push_str(&format!(
                "routers \"{name}\" {{\n  rule = {}\n  service = \"{name}\"\n  entrypoints = [\"a3s-cloud-https\"]\n}}\n\nservices \"{name}\" {{\n  load_balancer {{\n    strategy = \"round-robin\"\n    request_timeout = {}\n    servers = [{{ url = {} }}]\n  }}\n}}\n\n",
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
        GatewaySnapshot::new_with_certificate(
            metadata.node_id.as_uuid(),
            metadata.revision,
            metadata.expected_revision,
            metadata.issued_at,
            metadata.expires_at,
            acl,
            certificate_request,
        )
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

fn valid_certificate_directory(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

fn valid_absolute_file(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && path.is_absolute()
        && path.file_name().is_some()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{
        DomainNamePattern, RouteHostname, RoutePath, RoutePortName, UpstreamEndpoint,
    };
    use crate::modules::shared_kernel::domain::{
        DomainClaimId, EnvironmentId, GatewayCertificateId, OrganizationId, ProjectId, RouteId,
        WorkloadId, WorkloadRevisionId,
    };
    use chrono::{Duration, Utc};

    fn compiler() -> GatewaySnapshotCompiler {
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
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
            DomainClaimId::new(),
            DomainNamePattern::parse(hostname).expect("domain pattern"),
            GatewayCertificateId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}")).expect("upstream"),
            Utc::now(),
        )
        .expect("route")
    }

    #[test]
    fn compiles_every_owned_route_into_one_deterministic_snapshot() {
        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let mut first = route(node_id, "z.example.com", "/", 49152);
        first.state = RouteState::Active;
        let mut second = route(node_id, "api.example.com", "/v1", 49153);
        second.gateway_certificate_id = Some(certificate_id);
        let issued_at = Utc::now();
        let expires_at = issued_at + Duration::minutes(10);
        let forward = compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
                certificate_id,
                &[first.clone(), second.clone()],
            )
            .expect("snapshot");
        let reverse = compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
                certificate_id,
                &[second, first],
            )
            .expect("snapshot");
        assert_eq!(forward, reverse);
        assert_eq!(forward.acl.matches("routers \"").count(), 2);
        assert_eq!(forward.acl.matches("services \"").count(), 2);
        assert!(forward
            .acl
            .contains("Host(`api.example.com`) && PathPrefix(`/v1`)"));
        assert!(forward.acl.contains("http://127.0.0.1:49152/"));
        assert!(forward.acl.contains("mode { kind = \"cloud-managed\" }"));
        assert!(forward.acl.contains(&node_id.to_string()));
    }

    #[test]
    fn compiles_certificate_convergence_without_mutating_active_routes() {
        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let mut active = route(node_id, "api.example.com", "/", 49152);
        let previous_certificate_id = active.gateway_certificate_id.expect("previous certificate");
        active.state = RouteState::Active;
        let issued_at = Utc::now();

        let snapshot = compiler()
            .compile_certificate_convergence(
                GatewaySnapshotMetadata::new(
                    node_id,
                    2,
                    Some(1),
                    issued_at,
                    issued_at + Duration::minutes(10),
                ),
                Some(certificate_id),
                std::slice::from_ref(&active),
            )
            .expect("certificate convergence snapshot");

        assert_eq!(
            active.gateway_certificate_id,
            Some(previous_certificate_id),
            "the replacement is not authoritative before acknowledgement"
        );
        assert_eq!(
            snapshot
                .certificate_request
                .as_ref()
                .map(|request| request.certificate_id),
            Some(certificate_id.as_uuid())
        );
        assert!(snapshot.acl.contains("api.example.com"));
    }

    #[test]
    fn compiles_route_less_revocation_snapshot_without_a_certificate() {
        let node_id = NodeId::new();
        let issued_at = Utc::now();
        let snapshot = compiler()
            .compile_certificate_convergence(
                GatewaySnapshotMetadata::new(
                    node_id,
                    2,
                    Some(1),
                    issued_at,
                    issued_at + Duration::minutes(10),
                ),
                None,
                &[],
            )
            .expect("route-less revocation snapshot");

        assert!(snapshot.certificate_request.is_none());
        assert!(!snapshot.acl.contains("entrypoints \"a3s-cloud-https\""));
        assert!(snapshot.acl.contains("management {"));
    }

    #[test]
    fn rejects_cross_scope_and_duplicate_route_ownership() {
        let node_id = NodeId::new();
        let first = route(node_id, "api.example.com", "/v1", 49152);
        let duplicate = route(node_id, "api.example.com", "/v1", 49153);
        let issued_at = Utc::now();
        let expires_at = issued_at + Duration::minutes(10);
        assert!(compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 1, None, issued_at, expires_at),
                GatewayCertificateId::new(),
                &[first, duplicate],
            )
            .is_err());
        let foreign = route(NodeId::new(), "other.example.com", "/", 49154);
        assert!(compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 1, None, issued_at, expires_at),
                GatewayCertificateId::new(),
                &[foreign],
            )
            .is_err());
    }

    #[test]
    fn installed_gateway_validates_compiled_snapshot() {
        let Ok(binary) = std::env::var("A3S_CLOUD_TEST_GATEWAY_BIN") else {
            return;
        };
        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let mut route = route(node_id, "api.example.com", "/v1", 49152);
        route.gateway_certificate_id = Some(certificate_id);
        let issued_at = Utc::now();
        let snapshot = compiler()
            .compile(
                GatewaySnapshotMetadata::new(
                    node_id,
                    1,
                    None,
                    issued_at,
                    issued_at + Duration::minutes(10),
                ),
                certificate_id,
                &[route],
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
