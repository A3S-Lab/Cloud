use super::super::provider_workload::{
    compose_pinned_celld_service_process, validate_pinned_celld_provider_workload,
};
use super::*;
use crate::infrastructure::DisposableS3TestContext;
use crate::modules::data::{
    IObjectNamespace, ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
    ObjectNamespaceKey, ObjectNamespaceProviderProfile, ObjectNamespaceProviderProfileSpec,
    ObjectNamespaceRead,
};
use crate::modules::edge::infrastructure::gateway_http_upstream;
use crate::modules::edge::{
    DomainNamePattern, GatewayCertificateIssueRequest, GatewaySnapshotCompiler,
    GatewaySnapshotCompilerConfig, GatewaySnapshotMetadata, IGatewayCertificateAuthority,
    LocalGatewayCertificateAuthority, Route, RouteHostname, RoutePath, RoutePortName, RouteTarget,
};
use crate::modules::executions::project_execution_task;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeId, OrganizationId,
    ProjectId, RouteId, SecretId, SecretVersionReference, StorageNamespaceId, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::application::project_runtime_spec_with_digest;
use crate::modules::workloads::{
    HttpHealthCheck, OciArtifact, SecretBinding, SecretBindingTarget, ServicePort,
    ServiceResources, ServiceTemplate, WorkloadRevision,
};
use a3s_cloud_contracts::{
    artifact_uri, CloudSecretReference, GatewayAckState, GatewayCertificateSigningRequest,
    GatewayCertificateSigningResponse, GatewayManagementProtocolDiscovery, GatewaySnapshot,
    GatewaySnapshotObservationRequest, GatewaySnapshotObservationState,
    NodeArtifactDownloadRequest, NodeArtifactUploadRequest, NodeCommandAck, NodeCommandEnvelope,
    NodeCommandMetadata, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
    DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use a3s_cloud_node_agent::{
    build_box_runtime_provider, ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation,
    CommandExecutor, ControlPlaneConfig, DownloadedNodeArtifact, DurableGatewaySnapshotInstaller,
    FileCommandJournal, GatewayCertificateSigningTransport, GatewayControlConfig,
    GatewaySnapshotInstaller, LogShippingConfig, NodeAgentConfig, NodeArtifactManager,
    NodeArtifactTransport, NodeConfig, NodeControlClientError, NodeSecretTransport, SecretMaterial,
};
use a3s_runtime::contract::{
    NetworkMode, RestartPolicy, RuntimeActionRequest, RuntimeApplyRequest, RuntimeExecRequest,
    RuntimeHealthState, RuntimeInspection, RuntimeObservation, RuntimeServiceEndpoint,
    RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState, SecretReference, SecretTarget,
};
use a3s_runtime::RuntimeError;
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::error::Error;
use std::io::{self, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const GATE_ENV: &str = "A3S_CLOUD_TEST_CELL_BUNDLE_PUBLICATION";
const IMAGE_ENV: &str = "A3S_CLOUD_TEST_CELL_PROVIDER_IMAGE";
const GATEWAY_BINARY_ENV: &str = "A3S_CLOUD_TEST_GATEWAY_BIN";
const GATEWAY_TOKEN_ENV: &str = "A3S_GATEWAY_ADMIN_TOKEN";
const GATEWAY_HOSTNAME: &str = "cells.a3s.test";
const ACCESS_KEY_ENV: &str = "A3S_CLOUD_TEST_S3_ACCESS_KEY_ID";
const SECRET_KEY_ENV: &str = "A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY";
const SESSION_TOKEN_ENV: &str = "A3S_CLOUD_TEST_S3_SESSION_TOKEN";
const SCRIPT_NAME: &str = "a3s-cloud-cell-publication-gate";
const BOX_EXECUTION_GENERATION_CLAIM: &str = "a3s.box.execution-generation";
const PROVIDER_REVISION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/cell-conformance/celld-revision"
));
const GATEWAY_REVISION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/gateway-conformance/gateway-revision"
));
const WRANGLER_JSON: &[u8] = br#"{
  "name": "a3s-cloud-cell-publication-gate",
  "main": "worker.mjs",
  "no_bundle": true,
  "compatibility_date": "2026-08-16",
  "durable_objects": {
    "bindings": [
      { "name": "COUNTER", "class_name": "Counter" }
    ]
  },
  "migrations": [
    { "tag": "v1", "new_sqlite_classes": ["Counter"] }
  ]
}
"#;
const WORKER_MODULE: &[u8] = br#"export class Counter {
  constructor(state) {
    this.state = state;
  }

  async fetch(request) {
    const url = new URL(request.url);
    if (request.headers.get("Upgrade")?.toLowerCase() === "websocket") {
      const pair = new WebSocketPair();
      const server = pair[0];
      this.state.acceptWebSocket(server);
      return new Response(null, { status: 101, webSocket: pair[1] });
    }
    if (url.pathname === "/arm") {
      await this.state.storage.setAlarm(Date.now() + 2000);
      return new Response(JSON.stringify({
        armed: true,
        pendingAlarm: await this.state.storage.getAlarm(),
      }), { headers: { "content-type": "application/json" } });
    }
    if (url.pathname === "/alarm-status") {
      return new Response(JSON.stringify({
        fires: (await this.state.storage.get("alarmFires")) ?? 0,
        pendingAlarm: await this.state.storage.getAlarm(),
      }), { headers: { "content-type": "application/json" } });
    }
    const value = (await this.state.storage.get("value")) ?? 0;
    const next = value + 1;
    await this.state.storage.put("value", next);
    return new Response(JSON.stringify({ value: next, url: request.url }), {
      headers: { "content-type": "application/json" },
    });
  }

  async alarm() {
    const fires = (await this.state.storage.get("alarmFires")) ?? 0;
    await this.state.storage.put("alarmFires", fires + 1);
  }

  async webSocketMessage(webSocket, message) {
    const count = ((await this.state.storage.get("webSocketMessages")) ?? 0) + 1;
    await this.state.storage.put("webSocketMessages", count);
    webSocket.send(JSON.stringify({ echo: message, count }));
  }
}

export default {
  async fetch(request, env) {
    const name = new URL(request.url).searchParams.get("cell") ?? "primary";
    return env.COUNTER.get(env.COUNTER.idFromName(name)).fetch(request);
  },
};
"#;

type GateResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Retained CELL0.5-C3/C4 gate. It runs the exact publisher profile as an
/// ordinary node-bound Execution Task, resolves credentials through the sole
/// Cloud Secret adapter, materializes the typed bundle through the sole
/// Artifact adapter, observes and cleans the result through the production S0
/// object-namespace client, and routes the resulting Service through Edge's
/// complete snapshot plus the production Node Agent Gateway installer.
#[tokio::test]
#[ignore = "requires the dedicated Linux Box runner and disposable S3-compatible namespace"]
async fn real_celld_bundle_publication_uses_execution_box_secrets_artifacts_and_s0(
) -> GateResult<()> {
    if std::env::var(GATE_ENV).as_deref() != Ok("1") {
        return Err(invalid("dedicated gate did not enable real bundle publication").into());
    }
    let publisher = DurableCellPublisherProfile::pinned_celld_v0_2_1()?;
    publisher.validate()?;
    let expected_image = publisher
        .image_uri()
        .strip_prefix("oci://")
        .ok_or_else(|| invalid("publisher image URI is not OCI"))?;
    if std::env::var(IMAGE_ENV).as_deref() != Ok(expected_image) {
        return Err(invalid("real gate image differs from the publisher profile").into());
    }
    let revision = PROVIDER_REVISION.trim();
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("checked-in celld revision is invalid").into());
    }
    let gateway_revision = GATEWAY_REVISION.trim();
    if gateway_revision.len() != 40
        || !gateway_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid("checked-in Gateway revision is invalid").into());
    }

    let namespace_id = StorageNamespaceId::new();
    let storage = DisposableS3TestContext::from_environment_with_id(
        "cell-bundle-publication",
        namespace_id.as_uuid(),
    )?;
    if !storage.uses_secure_transport() || storage.virtual_hosted_style() {
        return Err(invalid("celld publication requires HTTPS path-style S0 storage").into());
    }
    let storage_profile =
        ObjectNamespaceProviderProfile::from_spec(ObjectNamespaceProviderProfileSpec {
            endpoint: storage.endpoint().into(),
            region: storage.region().into(),
            bucket: storage.bucket().into(),
            prefix: "a3s-cloud-tests/cell-bundle-publication".into(),
            virtual_hosted_style: storage.virtual_hosted_style(),
        })?;
    if storage_profile.namespace_prefix(namespace_id)? != storage.prefix() {
        return Err(invalid("S0 test namespace differs from product prefix semantics").into());
    }

    let publication =
        execute_publication(&storage, &storage_profile, namespace_id, &publisher).await;
    let cleanup = storage.remove_all().await;
    let outcome = publication?;
    let removed = cleanup?;
    if removed < outcome.publication_object_count {
        return Err(invalid("S0 cleanup omitted an object verified during publication").into());
    }

    println!(
        "A3S_CLOUD_CELL0_5_BUNDLE_PUBLICATION_CERTIFIED provider=celld revision={} image_digest={} publisher_profile_digest={} s0_profile_digest={} bundle_digest={} version={} task=succeeded replay=exact objects={} cleanup=verified secrets=ephemeral",
        revision,
        publisher.image_digest(),
        publisher.digest(),
        storage_profile.digest(),
        outcome.bundle_digest,
        outcome.version,
        outcome.publication_object_count,
    );
    println!(
        "A3S_CLOUD_CELL0_5_SINGLE_NODE_BEHAVIOR_CERTIFIED provider=celld revision={} service_profile_digest={} service_template_digest={} named_sqlite=verified idle_eviction=verified reactivation=verified alarms=verified websockets=verified process_death=verified rpo=0 box_generation_before={} box_generation_after={} fleet_replay=exact secrets=reauthorized cleanup=verified gateway=verified gateway_revision={} gateway_snapshot_digest={} gateway_http=verified gateway_websocket=verified gateway_fleet_replay=exact gateway_owner_lookup=absent",
        revision,
        outcome.behavior.service_profile_digest,
        outcome.behavior.service_template_digest,
        outcome.behavior.box_generation_before,
        outcome.behavior.box_generation_after,
        gateway_revision,
        outcome.behavior.gateway_snapshot_digest,
    );
    Ok(())
}

struct PublicationOutcome {
    bundle_digest: String,
    version: String,
    publication_object_count: usize,
    behavior: ServiceBehaviorOutcome,
}

struct ServiceBehaviorOutcome {
    service_profile_digest: String,
    service_template_digest: String,
    box_generation_before: u64,
    box_generation_after: u64,
    gateway_snapshot_digest: String,
}

struct ProcessDeathOutcome {
    box_generation_before: u64,
    box_generation_after: u64,
    provider_resource_id: String,
    provider_build: String,
}

struct GatewayConformance {
    _directory: tempfile::TempDir,
    _process: GatewayProcessGuard,
    gateway_id: NodeId,
    traffic_address: SocketAddr,
    management_address: SocketAddr,
    certificate_directory: PathBuf,
    managed_state_file: PathBuf,
    ca_bundle_file: PathBuf,
    installer: Arc<DurableGatewaySnapshotInstaller>,
}

impl GatewayConformance {
    async fn start(gateway_id: NodeId) -> GateResult<Self> {
        let binary = PathBuf::from(required_environment(GATEWAY_BINARY_ENV)?)
            .canonicalize()
            .map_err(|error| invalid(format!("Gateway binary is unavailable: {error}")))?;
        if !binary.is_file() {
            return Err(invalid("Gateway conformance binary is not a regular file").into());
        }
        required_environment(GATEWAY_TOKEN_ENV)?;
        let (traffic_address, management_address) = unused_loopback_addresses()?;
        let directory = tempfile::tempdir()?;
        let managed_state_file = directory.path().join("managed-snapshot.json");
        let certificate_directory = directory.path().join("certificates");
        let ca_directory = directory.path().join("ca");
        let ca_bundle_file = ca_directory.join("ca.pem");
        let config_file = directory.path().join("gateway.acl");
        std::fs::write(
            &config_file,
            gateway_bootstrap_acl(gateway_id, management_address, managed_state_file.as_path()),
        )?;
        let mut process = GatewayProcessGuard::start(&binary, &config_file)?;
        wait_for_gateway_management(process.child_mut(), management_address).await?;

        let authority = Arc::new(LocalGatewayCertificateAuthority::load_or_create(
            ca_directory,
        )?);
        let signing_transport: Arc<dyn GatewayCertificateSigningTransport> =
            Arc::new(LocalGatewaySigningTransport {
                gateway_id,
                dns_names: vec![GATEWAY_HOSTNAME.into()],
                authority,
            });
        let config = gateway_node_agent_config(
            directory.path(),
            management_address,
            certificate_directory.clone(),
        )?;
        let installer = Arc::new(DurableGatewaySnapshotInstaller::from_config(
            &config,
            gateway_id.as_uuid(),
            signing_transport,
        )?);
        Ok(Self {
            _directory: directory,
            _process: process,
            gateway_id,
            traffic_address,
            management_address,
            certificate_directory,
            managed_state_file,
            ca_bundle_file,
            installer,
        })
    }

    fn installer(&self) -> Arc<dyn GatewaySnapshotInstaller> {
        self.installer.clone()
    }

    fn compile_route_snapshot(
        &self,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        service_profile: &DurableCellServiceProfile,
        spec: &RuntimeUnitSpec,
        observation: &RuntimeObservation,
    ) -> GateResult<GatewaySnapshot> {
        observation.validate_against(spec).map_err(invalid)?;
        let public = RuntimeServiceEndpoint::from_observation(
            observation,
            &service_profile.spec().public_runtime_port,
        )
        .map_err(invalid)?;
        let internal = RuntimeServiceEndpoint::from_observation(
            observation,
            &service_profile.spec().internal_runtime_port,
        )
        .map_err(invalid)?;
        let observed_at = Utc::now() - ChronoDuration::milliseconds(1);
        let target = RouteTarget::new(
            workload_id,
            workload_revision_id,
            spec.unit_id.clone(),
            spec.generation,
            RoutePortName::parse(&service_profile.spec().public_runtime_port)?,
            gateway_http_upstream(&public)?,
            observed_at,
        )?;
        let certificate_id = GatewayCertificateId::new();
        let route = Route::create(
            RouteId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            GatewayScopeId::new(),
            self.gateway_id,
            RouteHostname::parse(GATEWAY_HOSTNAME)?,
            RoutePath::parse("/")?,
            DomainClaimId::new(),
            DomainNamePattern::parse(GATEWAY_HOSTNAME)?,
            certificate_id,
            workload_id,
            target,
            Utc::now(),
        )?;
        let issued_at = Utc::now();
        let snapshot = self.compiler()?.compile(
            GatewaySnapshotMetadata::new(
                self.gateway_id,
                1,
                None,
                issued_at,
                issued_at + ChronoDuration::minutes(15),
            ),
            certificate_id,
            &[route],
        )?;
        let public_origin = gateway_http_upstream(&public)?;
        if !snapshot.acl.contains(public_origin.as_str())
            || snapshot.acl.contains(&internal.socket_addr().to_string())
            || snapshot.acl.matches("routers \"").count() != 1
            || snapshot.acl.matches("services \"").count() != 1
            || snapshot.acl.matches("target = {").count() != 1
        {
            return Err(invalid(
                "Edge complete snapshot did not contain only the exact public Cell target",
            )
            .into());
        }
        Ok(snapshot)
    }

    fn compiler(&self) -> Result<GatewaySnapshotCompiler, String> {
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: self.traffic_address.to_string(),
            management_address: self.management_address.to_string(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: GATEWAY_TOKEN_ENV.into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: self.certificate_directory.to_string_lossy().into_owned(),
            managed_state_file: self.managed_state_file.to_string_lossy().into_owned(),
        })
    }

    fn public_access(
        &self,
        snapshot: &GatewaySnapshot,
        public: &RuntimeServiceEndpoint,
        internal: &RuntimeServiceEndpoint,
    ) -> GateResult<GatewayPublicAccess> {
        let ca_bundle = std::fs::read(&self.ca_bundle_file)?;
        let root = reqwest::Certificate::from_pem(&ca_bundle)?;
        let http_client = reqwest::Client::builder()
            .use_rustls_tls()
            .no_proxy()
            .tls_built_in_root_certs(false)
            .add_root_certificate(root)
            .resolve(GATEWAY_HOSTNAME, self.traffic_address)
            .timeout(Duration::from_secs(5))
            .build()?;
        let mut roots = rustls::RootCertStore::empty();
        let certificates = rustls_pemfile::certs(&mut BufReader::new(ca_bundle.as_slice()))
            .collect::<Result<Vec<_>, _>>()?;
        if certificates.is_empty() {
            return Err(invalid("Gateway CA bundle is empty").into());
        }
        for certificate in certificates {
            roots
                .add(certificate)
                .map_err(|error| invalid(format!("Gateway CA is invalid: {error}")))?;
        }
        let websocket_tls = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        Ok(GatewayPublicAccess {
            http_client,
            websocket_tls,
            traffic_address: self.traffic_address,
            snapshot_acl: snapshot.acl.clone(),
            public_origin: gateway_http_upstream(public)?.as_str().into(),
            internal_address: internal.socket_addr(),
        })
    }
}

struct GatewayProcessGuard {
    child: Child,
}

impl GatewayProcessGuard {
    fn start(binary: &Path, config_file: &Path) -> io::Result<Self> {
        Ok(Self {
            child: ProcessCommand::new(binary)
                .arg("--config")
                .arg(config_file)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()?,
        })
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for GatewayProcessGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct GatewayPublicAccess {
    http_client: reqwest::Client,
    websocket_tls: Arc<rustls::ClientConfig>,
    traffic_address: SocketAddr,
    snapshot_acl: String,
    public_origin: String,
    internal_address: SocketAddr,
}

impl GatewayPublicAccess {
    fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    fn url(&self, path: &str) -> String {
        format!(
            "https://{}:{}{}",
            GATEWAY_HOSTNAME,
            self.traffic_address.port(),
            path
        )
    }

    async fn connect_websocket(
        &self,
        path: &str,
    ) -> GateResult<(
        tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    )> {
        let stream = tokio::net::TcpStream::connect(self.traffic_address).await?;
        let server_name = rustls::pki_types::ServerName::try_from(GATEWAY_HOSTNAME)
            .map_err(|_| invalid("Gateway TLS hostname is invalid"))?
            .to_owned();
        let stream = tokio_rustls::TlsConnector::from(self.websocket_tls.clone())
            .connect(server_name, stream)
            .await?;
        tokio_tungstenite::client_async(self.url(path).replacen("https://", "wss://", 1), stream)
            .await
            .map_err(Into::into)
    }

    fn require_exact_public_endpoint(
        &self,
        public: &RuntimeServiceEndpoint,
        internal: &RuntimeServiceEndpoint,
    ) -> GateResult<()> {
        if gateway_http_upstream(public)?.as_str() != self.public_origin
            || internal.socket_addr() != self.internal_address
        {
            return Err(invalid(
                "Box recovery changed an endpoint behind the applied Gateway snapshot",
            )
            .into());
        }
        self.require_unmapped_internal_endpoint()
    }

    fn require_unmapped_internal_endpoint(&self) -> GateResult<()> {
        if self
            .snapshot_acl
            .contains(&self.internal_address.to_string())
        {
            return Err(invalid("Gateway snapshot exposed the Cell operator endpoint").into());
        }
        Ok(())
    }
}

struct LocalGatewaySigningTransport {
    gateway_id: NodeId,
    dns_names: Vec<String>,
    authority: Arc<LocalGatewayCertificateAuthority>,
}

#[async_trait]
impl GatewayCertificateSigningTransport for LocalGatewaySigningTransport {
    async fn sign_gateway_certificate(
        &self,
        request: &GatewayCertificateSigningRequest,
    ) -> Result<GatewayCertificateSigningResponse, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request.node_id != self.gateway_id.as_uuid() {
            return Err(NodeControlClientError::Invalid(
                "Gateway signing request changed its node identity".into(),
            ));
        }
        let material = self
            .authority
            .issue(GatewayCertificateIssueRequest {
                certificate_id: GatewayCertificateId::from_uuid(request.certificate_id),
                node_id: self.gateway_id,
                dns_names: self.dns_names.clone(),
                csr_pem: request.csr_pem.clone(),
                issued_at: request.requested_at,
                expires_at: request.requested_at + ChronoDuration::minutes(30),
            })
            .await
            .map_err(|error| NodeControlClientError::Invalid(error.to_string()))?;
        let response = GatewayCertificateSigningResponse {
            schema: GatewayCertificateSigningResponse::SCHEMA.into(),
            certificate_id: request.certificate_id,
            node_id: request.node_id,
            dns_names: self.dns_names.clone(),
            serial_number: material.serial_number,
            fingerprint: material.fingerprint,
            certificate_pem: material.certificate_pem,
            ca_bundle_pem: material.ca_bundle_pem,
            issued_at: material.issued_at,
            expires_at: material.expires_at,
        };
        response
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        Ok(response)
    }
}

fn unused_loopback_addresses() -> io::Result<(SocketAddr, SocketAddr)> {
    let traffic = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let management = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let addresses = (traffic.local_addr()?, management.local_addr()?);
    drop((traffic, management));
    Ok(addresses)
}

fn gateway_bootstrap_acl(
    gateway_id: NodeId,
    management_address: SocketAddr,
    managed_state_file: &Path,
) -> String {
    format!(
        "mode {{ kind = \"cloud-managed\" }}\n\n\
         managed {{\n  gateway_id = \"{gateway_id}\"\n  state_file = \"{}\"\n}}\n\n\
         management {{\n  enabled = true\n  address = \"{management_address}\"\n  path_prefix = \"/api/gateway\"\n  auth_token_env = \"{GATEWAY_TOKEN_ENV}\"\n  allowed_ips = [\"127.0.0.1\"]\n}}\n",
        managed_state_file.display(),
    )
}

async fn wait_for_gateway_management(
    child: &mut Child,
    management_address: SocketAddr,
) -> GateResult<()> {
    let token = required_environment(GATEWAY_TOKEN_ENV)?;
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?;
    let url = format!("http://{management_address}/api/gateway/version");
    for _ in 0..200 {
        if child.try_wait()?.is_some() {
            return Err(invalid("A3S Gateway exited before its management API was ready").into());
        }
        if client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(invalid("A3S Gateway management API did not become ready").into())
}

fn gateway_node_agent_config(
    root: &Path,
    management_address: SocketAddr,
    certificate_directory: PathBuf,
) -> GateResult<NodeAgentConfig> {
    Ok(NodeAgentConfig {
        control_plane: ControlPlaneConfig {
            enrollment_url: url::Url::parse("https://127.0.0.1/v1/nodes:enroll")?,
            node_control_url: url::Url::parse("https://127.0.0.1/v1/node-control")?,
            enrollment_token_env: "A3S_UNUSED_ENROLLMENT_TOKEN".into(),
            server_ca_file: root.join("unused-control-plane-ca.pem"),
            max_response_bytes: 1024 * 1024,
            connect_timeout_ms: 2_000,
            request_timeout_ms: 5_000,
            artifact_transfer_timeout_ms: 5_000,
            long_poll_margin_ms: 1_000,
            retry_initial_ms: 100,
            retry_max_ms: 1_000,
        },
        artifacts: ArtifactConfig {
            max_blob_bytes: 4 * 1024 * 1024,
            max_entries: 100,
            max_file_bytes: 2 * 1024 * 1024,
            max_expanded_bytes: 8 * 1024 * 1024,
        },
        node: NodeConfig {
            name: "cell-gateway-conformance".into(),
            state_dir: root.join("node-state"),
        },
        logs: LogShippingConfig {
            poll_interval_ms: 1_000,
            max_batch_chunks: 10,
            max_batch_bytes: 64 * 1024,
        },
        box_runtime: BoxRuntimeConfig {
            home_dir: root.join("unused-box-home"),
            secret_root: root.join("unused-box-secrets"),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
            sev_snp: None,
        },
        gateway: GatewayControlConfig {
            management_url: url::Url::parse(&format!("http://{management_address}/api/gateway"))?,
            auth_token_env: GATEWAY_TOKEN_ENV.into(),
            certificate_directory,
            connect_timeout_ms: 2_000,
            apply_timeout_ms: 5_000,
            readiness_timeout_ms: 10_000,
        },
    })
}

fn expect_gateway_applied(
    acknowledgement: &NodeCommandAck,
    command: &NodeCommandEnvelope,
    snapshot: &GatewaySnapshot,
) -> GateResult<()> {
    acknowledgement.validate_against(command).map_err(invalid)?;
    let NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } =
        succeeded_result(acknowledgement)?
    else {
        return Err(invalid("Fleet did not install the Durable Cell Gateway snapshot").into());
    };
    acknowledgement
        .validate_for(command.command_id, command.node_id, snapshot)
        .map_err(invalid)?;
    if acknowledgement.state != GatewayAckState::Applied
        || !acknowledgement.ready
        || acknowledgement.message.is_some()
        || acknowledgement
            .management_protocol
            .as_ref()
            .is_none_or(|protocol| {
                protocol.discovery != GatewayManagementProtocolDiscovery::Advertised
            })
    {
        return Err(invalid("pinned Gateway did not report exact ready-applied state").into());
    }
    Ok(())
}

fn verify_gateway_observation(
    acknowledgement: &NodeCommandAck,
    command: &NodeCommandEnvelope,
    request: &GatewaySnapshotObservationRequest,
    snapshot: &GatewaySnapshot,
) -> GateResult<()> {
    acknowledgement.validate_against(command).map_err(invalid)?;
    let NodeCommandResult::GatewaySnapshotObserved { observation } =
        succeeded_result(acknowledgement)?
    else {
        return Err(
            invalid("Fleet did not observe the applied Durable Cell Gateway snapshot").into(),
        );
    };
    observation
        .validate_for(command.command_id, command.node_id, request)
        .map_err(invalid)?;
    if request.gateway_id != snapshot.gateway_id
        || request.revision != snapshot.revision
        || request.snapshot_digest != snapshot.snapshot_digest
        || observation.state != GatewaySnapshotObservationState::Applied
        || !observation.ready
        || observation
            .applied
            .as_ref()
            .is_none_or(|applied| !applied.matches(request))
        || observation.management_protocol.discovery
            != GatewayManagementProtocolDiscovery::Advertised
    {
        return Err(invalid("Gateway observation changed the exact applied snapshot").into());
    }
    Ok(())
}

async fn execute_publication(
    storage: &DisposableS3TestContext,
    storage_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: StorageNamespaceId,
    publisher: &DurableCellPublisherProfile,
) -> GateResult<PublicationOutcome> {
    let bundle_bytes = directory_archive(&[
        ("worker.mjs", WORKER_MODULE),
        ("wrangler.json", WRANGLER_JSON),
    ])?;
    let bundle_digest = format!("sha256:{:x}", Sha256::digest(&bundle_bytes));
    let bundle = a3s_runtime::contract::ArtifactRef {
        uri: artifact_uri(&bundle_digest).map_err(invalid)?,
        digest: bundle_digest.clone(),
        media_type: DURABLE_CELL_BUNDLE_MEDIA_TYPE.into(),
    };
    let node_id = NodeId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let subject_id = workload_revision_id.as_uuid();
    let (secrets, secret_transport) = publication_secrets(subject_id, publisher)?;
    let service_secrets = secrets.clone();
    let artifact_transport = Arc::new(PublicationArtifactTransport {
        artifact: bundle.clone(),
        bytes: bundle_bytes,
        downloads: AtomicUsize::new(0),
    });
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let artifacts = Arc::new(NodeArtifactManager::new(
        node_state.path().join("artifacts"),
        ArtifactConfig {
            max_blob_bytes: 4 * 1024 * 1024,
            max_entries: 100,
            max_file_bytes: 2 * 1024 * 1024,
            max_expanded_bytes: 8 * 1024 * 1024,
        },
        node_id.as_uuid(),
        artifact_transport.clone(),
    )?);
    let home = PathBuf::from(std::env::var("A3S_HOME")?).canonicalize()?;
    let secret_root = home.join("runtime-secrets").canonicalize()?;
    let provider = build_box_runtime_provider(
        &BoxRuntimeConfig {
            home_dir: home,
            secret_root: secret_root.clone(),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
            sev_snp: None,
        },
        runtime_state.path(),
    )?;
    let secret_binding: Arc<dyn NodeSecretTransport> = secret_transport.clone();
    let runtime = provider
        .into_bound_client(secret_binding, artifacts.clone())
        .await?;
    let execution = publication_execution(
        node_id,
        subject_id,
        storage_namespace_id,
        storage_profile,
        publisher,
        bundle,
        secrets,
    )?;
    let spec = project_execution_task(&execution)?;
    if spec.class != RuntimeUnitClass::Task || spec.network.mode != NetworkMode::Outbound {
        return Err(invalid("publication Execution did not project to one outbound Task").into());
    }
    require_runtime_support(runtime.as_ref(), &spec).await?;
    let gateway = GatewayConformance::start(node_id).await?;
    let executor = CommandExecutor::new(
        FileCommandJournal::new(node_state.path().join("journal"), node_id.as_uuid())?,
        runtime.clone(),
        gateway.installer(),
    )
    .with_artifacts(artifacts);
    let apply = command(
        node_id,
        execution.id.as_uuid(),
        1,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("cell-publication-apply-{}", execution.id),
                deadline_at_ms: None,
                spec: spec.clone(),
            }),
            resource_claim: None,
        },
    )?;
    let applied = executor.execute(apply.clone()).await?;
    let observation = applied_observation(&applied)?;
    if observation.state != RuntimeUnitState::Succeeded {
        return Err(invalid(format!(
            "celld publication Task did not succeed: {:?}",
            observation.state
        ))
        .into());
    }
    let mut replay = apply;
    replay.lease_id = Uuid::now_v7();
    if executor.execute(replay).await?.outcome != applied.outcome {
        return Err(invalid("Fleet journal changed publication Task replay").into());
    }
    let removed = executor
        .execute(command(
            node_id,
            execution.id.as_uuid(),
            2,
            NodeCommandPayload::RuntimeRemove {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: format!("cell-publication-remove-{}", execution.id),
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                    deadline_at_ms: None,
                },
            },
        )?)
        .await?;
    expect_removed(&removed, &spec.unit_id)?;
    if !matches!(
        runtime.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid("removed publication Task remained inspectable").into());
    }
    if artifact_transport.downloads.load(Ordering::SeqCst) != 1
        || secret_transport.total_calls()? != secret_transport.material_count()
        || directory_has_entries(&secret_root)?
    {
        return Err(invalid(
            "publication replay or cleanup changed Artifact/Secret materialization",
        )
        .into());
    }

    let namespace: Arc<dyn IObjectNamespace> = Arc::new(storage.client());
    let version = verify_publication(namespace.as_ref()).await?;
    let behavior = verify_service_behavior(
        node_id,
        workload_revision_id,
        storage_namespace_id,
        storage_profile,
        publisher,
        service_secrets,
        &executor,
        runtime.as_ref(),
        &secret_root,
        artifact_transport.as_ref(),
        secret_transport.as_ref(),
        &gateway,
    )
    .await?;
    Ok(PublicationOutcome {
        bundle_digest,
        version,
        publication_object_count: 4,
        behavior,
    })
}

#[allow(clippy::too_many_arguments)]
async fn verify_service_behavior(
    node_id: NodeId,
    workload_revision_id: WorkloadRevisionId,
    storage_namespace_id: StorageNamespaceId,
    storage_profile: &ObjectNamespaceProviderProfile,
    publisher: &DurableCellPublisherProfile,
    secrets: Vec<SecretReference>,
    executor: &CommandExecutor,
    runtime: &dyn a3s_runtime::RuntimeClient,
    secret_root: &Path,
    artifact_transport: &PublicationArtifactTransport,
    secret_transport: &PublicationSecretTransport,
    gateway: &GatewayConformance,
) -> GateResult<ServiceBehaviorOutcome> {
    let service_profile = DurableCellServiceProfile::pinned_celld_v0_2_1()?;
    let credentials = service_credentials(
        workload_revision_id,
        storage_namespace_id,
        storage_profile,
        &secrets,
    )?;
    let template = service_template(
        storage_namespace_id,
        storage_profile,
        publisher,
        &service_profile,
        &secrets,
    )?;
    validate_pinned_celld_provider_workload(
        &credentials,
        storage_profile,
        &service_profile,
        &template,
        publisher,
    )?;
    let service_template_digest = template.digest()?;
    let workload_id = WorkloadId::new();
    let revision =
        WorkloadRevision::create(workload_revision_id, workload_id, 1, template, Utc::now())?;
    let spec =
        project_runtime_spec_with_digest(&revision, Some(service_profile.digest().as_str()))?;
    if spec.class != RuntimeUnitClass::Service
        || spec.network.mode != NetworkMode::Service
        || spec.restart != RestartPolicy::Always
        || spec.semantics_profile_digest.as_deref() != Some(service_profile.digest().as_str())
    {
        return Err(
            invalid("Durable Cell Workload did not project to the exact Runtime Service").into(),
        );
    }
    require_runtime_support(runtime, &spec).await?;
    let applied = executor
        .execute(command(
            node_id,
            workload_id.as_uuid(),
            3,
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("cell-service-apply-{workload_revision_id}"),
                    deadline_at_ms: None,
                    spec: spec.clone(),
                }),
                resource_claim: None,
            },
        )?)
        .await?;
    let initial_observation = applied_observation(&applied)?;
    let gateway_snapshot = gateway.compile_route_snapshot(
        workload_id,
        workload_revision_id,
        &service_profile,
        &spec,
        initial_observation,
    )?;
    let gateway_install = command_issued_at(
        node_id,
        gateway_snapshot.gateway_id,
        4,
        gateway_snapshot.issued_at,
        NodeCommandPayload::GatewaySnapshotInstall {
            snapshot: Box::new(gateway_snapshot.clone()),
        },
    )?;
    let installed = executor.execute(gateway_install.clone()).await;
    let replayed_install = match &installed {
        Ok(acknowledgement) => {
            let expected = acknowledgement.clone();
            let mut replay = gateway_install.clone();
            replay.lease_id = Uuid::now_v7();
            let replayed = executor.execute(replay.clone()).await;
            Some((expected, replay, replayed))
        }
        Err(_) => None,
    };
    let verification = async {
        expect_gateway_applied(
            installed
                .as_ref()
                .map_err(|error| invalid(format!("Gateway install failed: {error}")))?,
            &gateway_install,
            &gateway_snapshot,
        )?;
        match replayed_install.as_ref() {
            Some((expected, replay, Ok(actual))) => {
                verify_exact_command_replay(expected, replay, actual, "Gateway install")?;
            }
            _ => return Err(invalid("Fleet journal changed Gateway install replay").into()),
        }
        let initial_public = RuntimeServiceEndpoint::from_observation(
            initial_observation,
            &service_profile.spec().public_runtime_port,
        )
        .map_err(invalid)?;
        let initial_internal = RuntimeServiceEndpoint::from_observation(
            initial_observation,
            &service_profile.spec().internal_runtime_port,
        )
        .map_err(invalid)?;
        let access =
            gateway.public_access(&gateway_snapshot, &initial_public, &initial_internal)?;
        let recovery =
            verify_running_service_behavior(&applied, &service_profile, runtime, &spec, &access)
                .await?;
        Ok::<_, Box<dyn Error + Send + Sync>>((recovery, access))
    }
    .await;
    let inspect = command(
        node_id,
        workload_id.as_uuid(),
        5,
        NodeCommandPayload::RuntimeInspect {
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
        },
    )?;
    let inspected = executor.execute(inspect.clone()).await;
    let replayed = match &inspected {
        Ok(acknowledgement) => {
            let expected = acknowledgement.outcome.clone();
            let mut replay = inspect;
            replay.lease_id = Uuid::now_v7();
            Some(
                executor
                    .execute(replay)
                    .await
                    .map(|acknowledgement| (expected, acknowledgement.outcome)),
            )
        }
        Err(_) => None,
    };
    let gateway_observation_request = GatewaySnapshotObservationRequest::new(
        gateway_snapshot.gateway_id,
        gateway_snapshot.revision,
        gateway_snapshot.snapshot_digest.clone(),
    )?;
    let gateway_observe = command(
        node_id,
        gateway_snapshot.gateway_id,
        6,
        NodeCommandPayload::GatewaySnapshotObserve {
            request: gateway_observation_request.clone(),
        },
    )?;
    let gateway_observed = executor.execute(gateway_observe.clone()).await;
    let replayed_gateway_observation = match &gateway_observed {
        Ok(acknowledgement) => {
            let expected = acknowledgement.clone();
            let mut replay = gateway_observe.clone();
            replay.lease_id = Uuid::now_v7();
            let replayed = executor.execute(replay.clone()).await;
            Some((expected, replay, replayed))
        }
        Err(_) => None,
    };
    let removed = executor
        .execute(command(
            node_id,
            workload_id.as_uuid(),
            7,
            NodeCommandPayload::RuntimeRemove {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: format!("cell-service-remove-{workload_revision_id}"),
                    unit_id: spec.unit_id.clone(),
                    generation: spec.generation,
                    deadline_at_ms: None,
                },
            },
        )?)
        .await;
    let (verification, gateway_access) = verification?;
    verify_recovery_inspection(&inspected?, &spec, &verification)?;
    match replayed {
        Some(Ok((expected, actual))) if expected == actual => {}
        _ => {
            return Err(invalid("Fleet journal changed recovered Service inspection replay").into())
        }
    }
    verify_gateway_observation(
        &gateway_observed?,
        &gateway_observe,
        &gateway_observation_request,
        &gateway_snapshot,
    )?;
    match replayed_gateway_observation {
        Some((expected, replay, Ok(actual))) => {
            verify_exact_command_replay(&expected, &replay, &actual, "Gateway observation")?;
        }
        _ => return Err(invalid("Fleet journal changed Gateway observation replay").into()),
    }
    gateway_access.require_unmapped_internal_endpoint()?;
    expect_removed(&removed?, &spec.unit_id)?;
    if !matches!(
        runtime.inspect(&spec.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid("removed Durable Cell Workload Service remained inspectable").into());
    }
    let expected_secret_calls = secret_transport
        .material_count()
        .checked_mul(3)
        .ok_or_else(|| invalid("Durable Cell Secret call expectation overflowed"))?;
    if artifact_transport.downloads.load(Ordering::SeqCst) != 1
        || directory_has_entries(secret_root)?
        || secret_transport.total_calls()? != expected_secret_calls
    {
        return Err(invalid("Service behavior changed Artifact or Secret cleanup").into());
    }
    Ok(ServiceBehaviorOutcome {
        service_profile_digest: service_profile.digest().to_string(),
        service_template_digest,
        box_generation_before: verification.box_generation_before,
        box_generation_after: verification.box_generation_after,
        gateway_snapshot_digest: gateway_snapshot.snapshot_digest,
    })
}

async fn verify_running_service_behavior(
    applied: &NodeCommandAck,
    service_profile: &DurableCellServiceProfile,
    runtime: &dyn a3s_runtime::RuntimeClient,
    spec: &RuntimeUnitSpec,
    gateway: &GatewayPublicAccess,
) -> GateResult<ProcessDeathOutcome> {
    let observation = applied_observation(applied)?;
    observation.validate_against(spec).map_err(invalid)?;
    if observation.state != RuntimeUnitState::Running
        || observation
            .health
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
    {
        return Err(invalid("S0-backed celld Workload Service did not become healthy").into());
    }
    let public = RuntimeServiceEndpoint::from_observation(
        observation,
        &service_profile.spec().public_runtime_port,
    )
    .map_err(invalid)?;
    let internal = RuntimeServiceEndpoint::from_observation(
        observation,
        &service_profile.spec().internal_runtime_port,
    )
    .map_err(invalid)?;
    if public.socket_addr() == internal.socket_addr() {
        return Err(invalid("Durable Cell public and internal Runtime endpoints overlap").into());
    }
    gateway.require_exact_public_endpoint(&public, &internal)?;
    verify_single_node_behavior(gateway, &internal).await?;
    verify_provider_process_death(runtime, spec, service_profile, observation, gateway).await
}

async fn verify_provider_process_death(
    runtime: &dyn a3s_runtime::RuntimeClient,
    spec: &RuntimeUnitSpec,
    service_profile: &DurableCellServiceProfile,
    initial: &RuntimeObservation,
    gateway: &GatewayPublicAccess,
) -> GateResult<ProcessDeathOutcome> {
    let box_generation_before = box_execution_generation(initial)?;
    let expected_generation = box_generation_before
        .checked_add(1)
        .ok_or_else(|| invalid("Box execution generation overflowed"))?;
    let provider_resource_id = initial
        .provider_resource_id
        .clone()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("Box observation omitted its provider resource identity"))?;
    let provider_build = initial
        .provider_build
        .clone()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid("Box observation omitted its provider build"))?;

    inject_celld_process_death(runtime, spec).await?;
    let recovered = wait_for_provider_restart(
        runtime,
        spec,
        expected_generation,
        &provider_resource_id,
        &provider_build,
    )
    .await?;
    let public = RuntimeServiceEndpoint::from_observation(
        &recovered,
        &service_profile.spec().public_runtime_port,
    )
    .map_err(invalid)?;
    let internal = RuntimeServiceEndpoint::from_observation(
        &recovered,
        &service_profile.spec().internal_runtime_port,
    )
    .map_err(invalid)?;
    if public.socket_addr() == internal.socket_addr() {
        return Err(invalid("recovered Durable Cell Runtime endpoints overlap").into());
    }
    gateway.require_exact_public_endpoint(&public, &internal)?;
    verify_state_after_process_death(gateway).await?;

    Ok(ProcessDeathOutcome {
        box_generation_before,
        box_generation_after: box_execution_generation(&recovered)?,
        provider_resource_id,
        provider_build,
    })
}

async fn inject_celld_process_death(
    runtime: &dyn a3s_runtime::RuntimeClient,
    spec: &RuntimeUnitSpec,
) -> GateResult<()> {
    let request_id = format!("cell-service-process-death-{}", Uuid::now_v7());
    let result = runtime
        .exec(&RuntimeExecRequest {
            schema: RuntimeExecRequest::SCHEMA.into(),
            request_id: request_id.clone(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                concat!(
                    "for process in /proc/[0-9]*/comm; do ",
                    "IFS= read -r name < \"$process\" || continue; ",
                    "[ \"$name\" = celld ] || continue; ",
                    "pid=${process#/proc/}; pid=${pid%/comm}; ",
                    "kill -KILL \"$pid\"; exit 0; ",
                    "done; exit 44"
                )
                .into(),
            ],
            timeout_ms: 5_000,
            deadline_at_ms: None,
        })
        .await;
    match result {
        Ok(result) => {
            result.validate().map_err(invalid)?;
            if result.request_id != request_id || !matches!(result.exit_code, -1 | 0 | 137) {
                return Err(invalid(format!(
                    "celld process-death injection did not reach the provider: exit {}",
                    result.exit_code
                ))
                .into());
            }
        }
        Err(
            RuntimeError::NotFound { .. }
            | RuntimeError::DeadlineExceeded(_)
            | RuntimeError::ProviderUnavailable(_)
            | RuntimeError::Transport(_),
        ) => {
            // Killing the Service's primary process may tear down the exec
            // transport before it can return. Recovery generation and durable
            // application state below are the authoritative fault evidence.
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

async fn wait_for_provider_restart(
    runtime: &dyn a3s_runtime::RuntimeClient,
    spec: &RuntimeUnitSpec,
    expected_box_generation: u64,
    provider_resource_id: &str,
    provider_build: &str,
) -> GateResult<RuntimeObservation> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        match runtime.inspect(&spec.unit_id).await {
            Ok(RuntimeInspection::Found { observation, .. }) => {
                observation.validate_against(spec).map_err(invalid)?;
                let box_generation = box_execution_generation(&observation)?;
                if box_generation > expected_box_generation {
                    return Err(
                        invalid("Box restarted celld more than once after one fault").into(),
                    );
                }
                if box_generation == expected_box_generation
                    && observation.state == RuntimeUnitState::Running
                    && observation
                        .health
                        .as_ref()
                        .is_some_and(|health| health.state == RuntimeHealthState::Healthy)
                {
                    if observation.provider_resource_id.as_deref() != Some(provider_resource_id)
                        || observation.provider_build.as_deref() != Some(provider_build)
                    {
                        return Err(invalid(
                            "Box recovery changed the exact provider resource or build",
                        )
                        .into());
                    }
                    return Ok(*observation);
                }
            }
            Ok(RuntimeInspection::NotFound { .. }) => {
                return Err(
                    invalid("Box lost the Durable Cell Service during process death").into(),
                )
            }
            Err(RuntimeError::ProviderUnavailable(_) | RuntimeError::Transport(_)) => {}
            Err(error) => return Err(error.into()),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(invalid("Box did not recover the killed celld process in time").into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn verify_recovery_inspection(
    acknowledgement: &NodeCommandAck,
    spec: &RuntimeUnitSpec,
    recovery: &ProcessDeathOutcome,
) -> GateResult<()> {
    let NodeCommandResult::RuntimeInspected {
        inspection: RuntimeInspection::Found { observation, .. },
    } = succeeded_result(acknowledgement)?
    else {
        return Err(invalid("Fleet did not journal the recovered Durable Cell Service").into());
    };
    observation.validate_against(spec).map_err(invalid)?;
    if observation.state != RuntimeUnitState::Running
        || observation
            .health
            .as_ref()
            .is_none_or(|health| health.state != RuntimeHealthState::Healthy)
        || observation.provider_resource_id.as_deref() != Some(&recovery.provider_resource_id)
        || observation.provider_build.as_deref() != Some(&recovery.provider_build)
        || box_execution_generation(observation)? != recovery.box_generation_after
    {
        return Err(
            invalid("Fleet recovery receipt changed the exact healthy Box generation").into(),
        );
    }
    Ok(())
}

fn box_execution_generation(observation: &RuntimeObservation) -> GateResult<u64> {
    observation
        .evidence
        .as_ref()
        .and_then(|evidence| evidence.claims.get(BOX_EXECUTION_GENERATION_CLAIM))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            invalid("Box observation omitted valid execution-generation evidence").into()
        })
}

async fn require_runtime_support(
    runtime: &dyn a3s_runtime::RuntimeClient,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> GateResult<()> {
    let missing = runtime.capabilities().await?.missing_for(spec)?;
    if !missing.is_empty() {
        return Err(invalid(format!(
            "pinned Box cannot run the Durable Cell conformance projection: {}",
            missing.join(", ")
        ))
        .into());
    }
    Ok(())
}

fn service_credentials(
    workload_revision_id: WorkloadRevisionId,
    storage_namespace_id: StorageNamespaceId,
    storage_profile: &ObjectNamespaceProviderProfile,
    secrets: &[SecretReference],
) -> GateResult<ObjectNamespaceCredentialBinding> {
    let access_key = service_secret_reference(secrets, "s0-access-key-id")?;
    let secret_access_key = service_secret_reference(secrets, "s0-secret-access-key")?;
    let session_token = service_secret_reference_optional(secrets, "s0-session-token")?;
    for reference in [Some(access_key), Some(secret_access_key), session_token]
        .into_iter()
        .flatten()
    {
        if reference.workload_revision_id != workload_revision_id.as_uuid() {
            return Err(invalid("Service Secret reference changed its Workload revision").into());
        }
    }
    ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        environment_id: EnvironmentId::new(),
        namespace_id: storage_namespace_id,
        generation: 1,
        provider_profile_digest: storage_profile.digest().clone(),
        access_key_id: secret_version_reference(access_key)?,
        secret_access_key: secret_version_reference(secret_access_key)?,
        session_token: session_token.map(secret_version_reference).transpose()?,
    })
    .map_err(|error| invalid(error).into())
}

fn service_template(
    storage_namespace_id: StorageNamespaceId,
    storage_profile: &ObjectNamespaceProviderProfile,
    publisher: &DurableCellPublisherProfile,
    service_profile: &DurableCellServiceProfile,
    secrets: &[SecretReference],
) -> GateResult<ServiceTemplate> {
    let template = ServiceTemplate {
        artifact: OciArtifact {
            uri: publisher.image_uri().into(),
            digest: publisher.image_digest().to_string(),
            media_type: OCI_IMAGE_INDEX_MEDIA_TYPE.into(),
        },
        process: compose_pinned_celld_service_process(
            storage_profile,
            storage_namespace_id,
            8080,
            8081,
            publisher,
        )?,
        secrets: secrets
            .iter()
            .map(service_secret_binding)
            .collect::<Result<Vec<_>, _>>()?,
        resources: ServiceResources {
            cpu_millis: 1_000,
            memory_bytes: 512 * 1024 * 1024,
            pids: 256,
            ephemeral_storage_bytes: None,
        },
        ports: vec![
            ServicePort {
                name: service_profile.spec().public_runtime_port.clone(),
                container_port: 8080,
            },
            ServicePort {
                name: service_profile.spec().internal_runtime_port.clone(),
                container_port: 8081,
            },
        ],
        health: Some(HttpHealthCheck {
            port_name: service_profile.spec().public_runtime_port.clone(),
            path: service_profile.spec().health_path.clone(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 5_000,
        }),
    };
    template.validate().map_err(invalid)?;
    Ok(template)
}

fn service_secret_binding(reference: &SecretReference) -> Result<SecretBinding, io::Error> {
    let parsed = CloudSecretReference::parse(&reference.reference).map_err(invalid)?;
    let target = match &reference.target {
        SecretTarget::Environment { variable } => SecretBindingTarget::Environment {
            variable: variable.clone(),
        },
        _ => {
            return Err(invalid(
                "celld Service Secret target is not an environment variable",
            ))
        }
    };
    Ok(SecretBinding {
        name: reference.name.clone(),
        secret_id: SecretId::from_uuid(parsed.secret_id),
        version: parsed.version,
        target,
    })
}

fn service_secret_reference(
    secrets: &[SecretReference],
    name: &str,
) -> GateResult<CloudSecretReference> {
    service_secret_reference_optional(secrets, name)?
        .ok_or_else(|| invalid(format!("Service omitted Secret binding {name}")).into())
}

fn service_secret_reference_optional(
    secrets: &[SecretReference],
    name: &str,
) -> GateResult<Option<CloudSecretReference>> {
    let matches = secrets
        .iter()
        .filter(|reference| reference.name == name)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [reference] => CloudSecretReference::parse(&reference.reference)
            .map(Some)
            .map_err(|error| invalid(error).into()),
        _ => Err(invalid(format!("Service duplicated Secret binding {name}")).into()),
    }
}

fn secret_version_reference(
    reference: CloudSecretReference,
) -> Result<SecretVersionReference, io::Error> {
    SecretVersionReference::new(SecretId::from_uuid(reference.secret_id), reference.version)
        .map_err(invalid)
}

async fn verify_single_node_behavior(
    gateway: &GatewayPublicAccess,
    internal: &RuntimeServiceEndpoint,
) -> GateResult<()> {
    let client = gateway.http_client();
    let url = gateway.url("/?cell=retained-counter");
    require_counter_value(client, &url, 1).await?;
    require_counter_value(client, &url, 2).await?;
    verify_alarm_delivery(client, gateway).await?;
    let state_url = format!("http://{}/state", internal.socket_addr());
    if operator_occupied(client, &state_url).await? == 0 {
        return Err(invalid("named Cell was not resident before idle eviction").into());
    }
    verify_hibernatable_websocket(client, gateway, &state_url).await?;
    wait_until_unoccupied(client, &state_url, "named Cell idle eviction").await?;
    require_counter_value(client, &url, 3).await
}

async fn verify_state_after_process_death(gateway: &GatewayPublicAccess) -> GateResult<()> {
    let counter_url = gateway.url("/?cell=retained-counter");
    require_counter_value(gateway.http_client(), &counter_url, 4).await?;
    require_persisted_alarm_delivery(gateway.http_client(), gateway).await?;
    let (mut socket, response) = gateway.connect_websocket("/?cell=retained-counter").await?;
    if response.status() != 101 {
        return Err(invalid("recovered Durable Cell WebSocket upgrade was not accepted").into());
    }
    require_websocket_echo(&mut socket, "after-provider-process-death", 3).await?;
    socket.close(None).await?;
    Ok(())
}

async fn verify_alarm_delivery(
    client: &reqwest::Client,
    gateway: &GatewayPublicAccess,
) -> GateResult<()> {
    let arm_url = gateway.url("/arm?cell=retained-counter");
    let armed = get_json(client, &arm_url).await?;
    if armed.get("armed").and_then(serde_json::Value::as_bool) != Some(true)
        || armed
            .get("pendingAlarm")
            .and_then(serde_json::Value::as_u64)
            .is_none()
    {
        return Err(invalid("named Durable Cell did not persist its alarm deadline").into());
    }
    let status_url = gateway.url("/alarm-status?cell=retained-counter");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let status = get_json(client, &status_url).await?;
        if alarm_was_delivered(&status) {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let stable = get_json(client, &status_url).await?;
            if alarm_was_delivered(&stable) {
                return Ok(());
            }
            return Err(invalid("named Durable Cell alarm was delivered more than once").into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(invalid("named Durable Cell alarm did not fire exactly once").into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn require_persisted_alarm_delivery(
    client: &reqwest::Client,
    gateway: &GatewayPublicAccess,
) -> GateResult<()> {
    let status_url = gateway.url("/alarm-status?cell=retained-counter");
    for sample in 0..2 {
        let status = get_json(client, &status_url).await?;
        if !alarm_was_delivered(&status) {
            return Err(invalid(
                "named Durable Cell lost or repeated its alarm across provider process death",
            )
            .into());
        }
        if sample == 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Ok(())
}

fn alarm_was_delivered(status: &serde_json::Value) -> bool {
    status.get("fires").and_then(serde_json::Value::as_u64) == Some(1)
        && status
            .get("pendingAlarm")
            .is_some_and(serde_json::Value::is_null)
}

async fn verify_hibernatable_websocket(
    client: &reqwest::Client,
    gateway: &GatewayPublicAccess,
    state_url: &str,
) -> GateResult<()> {
    let (mut socket, response) = gateway.connect_websocket("/?cell=retained-counter").await?;
    if response.status() != 101 {
        return Err(invalid("Durable Cell WebSocket upgrade was not accepted").into());
    }
    require_websocket_echo(&mut socket, "before-hibernation", 1).await?;
    wait_until_unoccupied(client, state_url, "hibernatable WebSocket").await?;
    require_websocket_echo(&mut socket, "after-hibernation", 2).await?;
    socket.close(None).await?;
    Ok(())
}

async fn require_websocket_echo<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    message: &str,
    expected_count: u64,
) -> GateResult<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            message.to_owned().into(),
        ))
        .await?;
    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(message @ tokio_tungstenite::tungstenite::Message::Text(_))) => {
                    return Ok::<_, Box<dyn Error + Send + Sync>>(message);
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {
                    return Err(invalid("Durable Cell WebSocket closed before its reply").into())
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
            }
        }
    })
    .await
    .map_err(|_| invalid("Durable Cell WebSocket reply timed out"))??;
    let value: serde_json::Value = serde_json::from_str(reply.to_text()?)?;
    if value.get("echo").and_then(serde_json::Value::as_str) != Some(message)
        || value.get("count").and_then(serde_json::Value::as_u64) != Some(expected_count)
    {
        return Err(invalid("hibernatable WebSocket changed its echoed durable state").into());
    }
    Ok(())
}

async fn wait_until_unoccupied(
    client: &reqwest::Client,
    state_url: &str,
    behavior: &str,
) -> GateResult<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(75);
    loop {
        if operator_occupied(client, state_url).await? == 0 {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(invalid(format!(
                "named Cell did not become inactive during {behavior}"
            ))
            .into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn require_counter_value(
    client: &reqwest::Client,
    url: &str,
    expected: u64,
) -> GateResult<()> {
    let value = get_json(client, url).await?;
    if value.get("value").and_then(serde_json::Value::as_u64) != Some(expected) {
        return Err(invalid(format!(
            "named Durable Cell returned {:?} instead of counter {expected}",
            value.get("value")
        ))
        .into());
    }
    Ok(())
}

async fn get_json(client: &reqwest::Client, url: &str) -> GateResult<serde_json::Value> {
    let response = client.get(url).send().await?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|bytes| bytes > 64 * 1024)
    {
        return Err(invalid(format!(
            "Durable Cell request returned bounded HTTP {}",
            response.status()
        ))
        .into());
    }
    let bytes = response.bytes().await?;
    if bytes.len() > 64 * 1024 {
        return Err(invalid("Durable Cell JSON response exceeded its bound").into());
    }
    serde_json::from_slice(&bytes).map_err(Into::into)
}

async fn operator_occupied(client: &reqwest::Client, url: &str) -> GateResult<u64> {
    let response = client.get(url).send().await?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|bytes| bytes > 64 * 1024)
    {
        return Err(invalid("celld operator state was unavailable or unbounded").into());
    }
    let bytes = response.bytes().await?;
    if bytes.len() > 64 * 1024 {
        return Err(invalid("celld operator state exceeded its response bound").into());
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)?
        .get("occupied")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| invalid("celld operator state omitted occupied").into())
}

fn publication_execution(
    node_id: NodeId,
    subject_id: Uuid,
    storage_namespace_id: StorageNamespaceId,
    storage_profile: &ObjectNamespaceProviderProfile,
    publisher: &DurableCellPublisherProfile,
    bundle: a3s_runtime::contract::ArtifactRef,
    secrets: Vec<SecretReference>,
) -> Result<Execution, String> {
    let execution_id = ExecutionId::new();
    let definition = build_publication_task_definition(
        storage_profile,
        publisher,
        PublicationTaskDefinitionInput {
            node_id,
            storage_namespace_id,
            image_media_type: OCI_IMAGE_INDEX_MEDIA_TYPE.into(),
            authority: ExecutionTaskAuthority {
                kind: PUBLICATION_AUTHORITY_KIND.into(),
                subject_id,
                digest: Sha256Digest::from_bytes(
                    format!(
                        "cell0.5-c3:{}:{}:{}",
                        publisher.digest(),
                        storage_profile.digest(),
                        bundle.digest
                    )
                    .as_bytes(),
                ),
            },
            input: serde_json::json!({
                "schema": PUBLICATION_INPUT_SCHEMA,
                "conformance": "cell0.5-c3",
            }),
            bundle,
            secrets,
        },
    )?;
    Execution::create_bound_task(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        execution_id,
        definition.template,
        node_id,
        definition.task_policy,
        Utc::now() - ChronoDuration::seconds(1),
    )
}

fn publication_secrets(
    subject_id: Uuid,
    publisher: &DurableCellPublisherProfile,
) -> GateResult<(Vec<SecretReference>, Arc<PublicationSecretTransport>)> {
    let mut bindings = Vec::new();
    let mut materials = HashMap::new();
    for (name, variable, environment) in [
        (
            "s0-access-key-id",
            publisher.access_key_environment(),
            ACCESS_KEY_ENV,
        ),
        (
            "s0-secret-access-key",
            publisher.secret_access_key_environment(),
            SECRET_KEY_ENV,
        ),
    ] {
        add_secret(
            subject_id,
            name,
            variable,
            required_environment(environment)?,
            &mut bindings,
            &mut materials,
        )?;
    }
    if let Some(value) = optional_environment(SESSION_TOKEN_ENV)? {
        add_secret(
            subject_id,
            "s0-session-token",
            publisher.session_token_environment(),
            value,
            &mut bindings,
            &mut materials,
        )?;
    }
    Ok((
        bindings,
        Arc::new(PublicationSecretTransport {
            materials,
            calls: Mutex::new(HashMap::new()),
        }),
    ))
}

fn add_secret(
    subject_id: Uuid,
    name: &str,
    variable: &str,
    value: String,
    bindings: &mut Vec<SecretReference>,
    materials: &mut HashMap<String, Vec<u8>>,
) -> Result<(), String> {
    let reference = CloudSecretReference::new(subject_id, SecretId::new().as_uuid(), 1)?;
    materials.insert(reference.to_string(), value.into_bytes());
    bindings.push(SecretReference {
        name: name.into(),
        reference: reference.to_string(),
        target: SecretTarget::Environment {
            variable: variable.into(),
        },
    });
    Ok(())
}

struct PublicationSecretTransport {
    materials: HashMap<String, Vec<u8>>,
    calls: Mutex<HashMap<String, usize>>,
}

impl PublicationSecretTransport {
    fn material_count(&self) -> usize {
        self.materials.len()
    }

    fn total_calls(&self) -> Result<usize, String> {
        self.calls
            .lock()
            .map(|calls| calls.values().sum())
            .map_err(|_| "publication Secret call lock poisoned".into())
    }
}

#[async_trait]
impl NodeSecretTransport for PublicationSecretTransport {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        let reference = reference.to_string();
        let material = self.materials.get(&reference).cloned().ok_or_else(|| {
            NodeControlClientError::Invalid("publication Secret reference is unknown".into())
        })?;
        *self
            .calls
            .lock()
            .map_err(|_| NodeControlClientError::Transport("Secret call lock poisoned".into()))?
            .entry(reference)
            .or_default() += 1;
        SecretMaterial::new(material).map_err(NodeControlClientError::Invalid)
    }
}

struct PublicationArtifactTransport {
    artifact: a3s_runtime::contract::ArtifactRef,
    bytes: Vec<u8>,
    downloads: AtomicUsize,
}

#[async_trait]
impl NodeArtifactTransport for PublicationArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request
            .artifact()
            .map_err(NodeControlClientError::Invalid)?
            != self.artifact
            || self.bytes.len() as u64 > maximum_bytes
        {
            return Err(NodeControlClientError::Invalid(
                "publication Task requested an unexpected bundle Artifact".into(),
            ));
        }
        tokio::fs::write(destination, &self.bytes)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(DownloadedNodeArtifact {
            size_bytes: self.bytes.len() as u64,
        })
    }

    async fn upload(
        &self,
        _request: &NodeArtifactUploadRequest,
        _source: &Path,
    ) -> Result<a3s_cloud_contracts::NodeArtifactUploadReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "publication Task has no output Artifact authority".into(),
        ))
    }
}

async fn verify_publication(namespace: &dyn IObjectNamespace) -> GateResult<String> {
    let pointer = read_required(namespace, "deploy/current.json", 64 * 1024).await?;
    let named_pointer = read_required(
        namespace,
        &format!("deploy/{SCRIPT_NAME}/current.json"),
        64 * 1024,
    )
    .await?;
    if pointer != named_pointer {
        return Err(invalid("celld named and fleet deployment pointers differ").into());
    }
    let pointer: serde_json::Value = serde_json::from_slice(&pointer)?;
    let version = pointer["version"]
        .as_str()
        .filter(|value| {
            value.len() == 16
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| invalid("celld deployment pointer version is invalid"))?;
    let expected_prefix = format!("deploy/{SCRIPT_NAME}/{version}");
    if pointer["script_name"].as_str() != Some(SCRIPT_NAME)
        || pointer["prefix"].as_str() != Some(expected_prefix.as_str())
        || pointer["rollout"]["percent"].as_u64() != Some(100)
    {
        return Err(invalid("celld deployment pointer changed its committed identity").into());
    }
    let manifest = read_required(
        namespace,
        &format!("{expected_prefix}/manifest.json"),
        256 * 1024,
    )
    .await?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest)?;
    if manifest["schema_version"].as_u64() != Some(1)
        || manifest["version"].as_str() != Some(version)
        || manifest["script_name"].as_str() != Some(SCRIPT_NAME)
        || manifest["main_module"].as_str() != Some("index.js")
        || manifest["do_classes"] != serde_json::json!(["Counter"])
        || manifest["sqlite_classes"] != serde_json::json!(["Counter"])
        || manifest["modules"]
            .as_array()
            .is_none_or(|modules| modules.len() != 1)
        || manifest["modules"][0]["name"].as_str() != Some("index.js")
        || manifest["modules"][0]["bytes"].as_u64() != Some(WORKER_MODULE.len() as u64)
    {
        return Err(invalid("celld deployment manifest changed the typed test bundle").into());
    }
    let module = read_required(
        namespace,
        &format!("{expected_prefix}/index.js"),
        2 * 1024 * 1024,
    )
    .await?;
    let module_digest = format!("{:x}", Sha256::digest(&module));
    if module != WORKER_MODULE
        || manifest["modules"][0]["sha256"].as_str() != Some(&module_digest[..16])
    {
        return Err(invalid("celld published module bytes or digest changed").into());
    }
    Ok(version.into())
}

async fn read_required(
    namespace: &dyn IObjectNamespace,
    key: &str,
    maximum_bytes: u64,
) -> GateResult<Vec<u8>> {
    match namespace
        .read(&ObjectNamespaceKey::parse(key)?, maximum_bytes)
        .await?
    {
        ObjectNamespaceRead::Found { body, .. } => Ok(body),
        ObjectNamespaceRead::Missing => {
            Err(invalid(format!("published object {key} is missing")).into())
        }
        ObjectNamespaceRead::Corrupt => {
            Err(invalid(format!("published object {key} is corrupt")).into())
        }
    }
}

fn directory_archive(entries: &[(&str, &[u8])]) -> GateResult<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, path, *bytes)?;
    }
    builder.finish()?;
    Ok(builder.into_inner()?)
}

fn command(
    node_id: NodeId,
    aggregate_id: Uuid,
    sequence: u64,
    payload: NodeCommandPayload,
) -> GateResult<NodeCommandEnvelope> {
    let issued_at = Utc::now() - ChronoDuration::seconds(1);
    command_issued_at(node_id, aggregate_id, sequence, issued_at, payload)
}

fn command_issued_at(
    node_id: NodeId,
    aggregate_id: Uuid,
    sequence: u64,
    issued_at: chrono::DateTime<Utc>,
    payload: NodeCommandPayload,
) -> GateResult<NodeCommandEnvelope> {
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id: node_id.as_uuid(),
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(15),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
    .map_err(|error| invalid(error).into())
}

fn verify_exact_command_replay(
    expected: &NodeCommandAck,
    replay_command: &NodeCommandEnvelope,
    actual: &NodeCommandAck,
    behavior: &str,
) -> GateResult<()> {
    actual.validate_against(replay_command).map_err(invalid)?;
    let mut expected = expected.clone();
    expected.lease_id = replay_command.lease_id;
    if &expected != actual {
        return Err(invalid(format!("Fleet journal changed {behavior} replay")).into());
    }
    Ok(())
}

fn applied_observation(
    acknowledgement: &NodeCommandAck,
) -> GateResult<&a3s_runtime::contract::RuntimeObservation> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
        _ => Err(invalid("Durable Cell Runtime apply returned an unexpected result").into()),
    }
}

fn expect_removed(acknowledgement: &NodeCommandAck, unit_id: &str) -> GateResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeRemoved { removal } if removal.unit_id == unit_id => Ok(()),
        _ => Err(invalid("Durable Cell Runtime remove returned an unexpected result").into()),
    }
}

fn succeeded_result(acknowledgement: &NodeCommandAck) -> GateResult<&NodeCommandResult> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => Ok(result),
        _ => Err(invalid("Durable Cell node command did not succeed").into()),
    }
}

fn required_environment(name: &str) -> Result<String, String> {
    let value = std::env::var(name).map_err(|_| format!("{name} is required"))?;
    if value.is_empty() || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{name} is invalid"));
    }
    Ok(value)
}

fn optional_environment(name: &str) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) if value.contains(['\0', '\r', '\n']) => Err(format!("{name} is invalid")),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} is invalid")),
    }
}

fn directory_has_entries(path: &Path) -> io::Result<bool> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().transpose()?.is_some()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
