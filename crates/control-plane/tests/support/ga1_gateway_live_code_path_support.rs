//! GA-1 Gateway LIVE Code-path support.
//!
//! Phase 1 reuses the A0.4 published Agent release exercise (deploy, recover).
//! Phase 2 — while the recovered Agent is still Running — installs a
//! pin-matched `a3s-gateway` snapshot via `DurableGatewaySnapshotInstaller`
//! and proves public HTTP traffic to Agent `/health/ready`.
//!
//! Conversations/events on the management plane against the same release remain
//! required for the CERTIFIED marker; refuse CERTIFIED without both.
//!
//! This module is compiled as part of the `ga1_gateway_live_code_path` test
//! binary (sibling to `a0_4_real_box_release_support`).

#[cfg(target_os = "linux")]
use a3s_cloud_contracts::GatewaySnapshot;
#[cfg(target_os = "linux")]
use a3s_cloud_node_agent::{
    ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation, ControlPlaneConfig,
    DurableGatewaySnapshotInstaller, GatewayControlConfig, GatewaySnapshotInstallOutcome,
    GatewaySnapshotInstaller, LogShippingConfig, NodeAgentConfig, NodeConfig,
};
#[cfg(target_os = "linux")]
use a3s_runtime::contract::{RuntimeObservation, RuntimeServiceEndpoint, RuntimeUnitSpec};
#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const CERTIFIED_MARKER: &str = "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED";
#[cfg(target_os = "linux")]
const GATEWAY_PUBLIC_MARKER: &str = "A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN";
#[cfg(target_os = "linux")]
const GATEWAY_TOKEN: &str = "a3s-cloud-ga1-gateway-live-token";
#[cfg(target_os = "linux")]
const GATEWAY_TOKEN_ENV: &str = "A3S_GATEWAY_ADMIN_TOKEN";
#[cfg(target_os = "linux")]
const AGENT_PORT_NAME: &str = "agent";
#[cfg(target_os = "linux")]
const READY_PATH: &str = "/health/ready";

#[cfg(target_os = "linux")]
pub async fn exercise_ga1_gateway_live_code_path(
    postgres_url: String,
    gateway_bin: &str,
) -> TestResult {
    refuse_placeholder(gateway_bin)?;
    let gateway_revision = read_sidecar_revision(gateway_bin)?;
    let want_gateway = std::fs::read_to_string(cloud_pin("tools/gateway-conformance/gateway-revision"))
        .unwrap_or_default()
        .trim()
        .to_owned();
    if !want_gateway.is_empty() && gateway_revision != want_gateway {
        return Err(format!(
            "gateway pin mismatch want={want_gateway} got={gateway_revision}"
        )
        .into());
    }

    let box_revision = std::fs::read_to_string(cloud_pin("tools/box-conformance/box-revision"))
        .unwrap_or_default()
        .trim()
        .to_owned();
    let artifact = std::env::var("A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE").map_err(|_| {
        "A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE required (source a0-4-image.env)"
    })?;
    refuse_placeholder(&artifact)?;

    let gateway_bin = gateway_bin.to_owned();
    let gateway_revision_for_probe = gateway_revision.clone();
    let box_revision_for_probe = box_revision.clone();
    let artifact_for_probe = artifact.clone();

    let management_counts = Arc::new(std::sync::Mutex::new(
        None::<crate::ga1_management_plane_support::ManagementPlaneProof>,
    ));
    let management_counts_for_probe = Arc::clone(&management_counts);

    let live: crate::a0_4_real_box_release_support::LiveAgentProbe =
        Box::new(move |window| {
            let gateway_bin = gateway_bin.clone();
            let gateway_revision = gateway_revision_for_probe.clone();
            let box_revision = box_revision_for_probe.clone();
            let artifact = artifact_for_probe.clone();
            let management_counts = Arc::clone(&management_counts_for_probe);
            Box::pin(async move {
                prove_gateway_public_traffic(
                    &gateway_bin,
                    &window.observation,
                    &window.spec,
                    &gateway_revision,
                    &box_revision,
                    &artifact,
                )
                .await?;
                let management = crate::ga1_management_plane_support::prove_management_plane_conversation_and_events(
                    window.organization_id,
                    window.project_id,
                    window.environment_id,
                    window.asset_id,
                    window.asset_release_id,
                    window.executor,
                )
                .await?;
                *management_counts
                    .lock()
                    .map_err(|_| "GA-1 management plane counts lock poisoned")? =
                    Some(management);
                Ok(())
            })
        });

    crate::a0_4_real_box_release_support::exercise_published_agent_release_real_box_with_live_probe(
        postgres_url,
        live,
    )
    .await?;

    let management = management_counts
        .lock()
        .map_err(|_| "GA-1 management plane counts lock poisoned")?
        .clone()
        .ok_or("GA-1 management plane proof missing after live probe")?;
    println!(
        "{CERTIFIED_MARKER} gateway_public=traffic conversations=1 executions=1 events={} \
         head_sequence={} box={box_revision} gateway={gateway_revision} artifact={artifact}",
        management.event_count, management.head_sequence
    );
    Ok(())
}

#[cfg(target_os = "linux")]
async fn prove_gateway_public_traffic(
    gateway_bin: &str,
    observation: &RuntimeObservation,
    spec: &RuntimeUnitSpec,
    gateway_revision: &str,
    box_revision: &str,
    artifact: &str,
) -> TestResult {
    observation.validate_against(spec)?;
    let endpoint = RuntimeServiceEndpoint::from_observation(observation, AGENT_PORT_NAME)
        .map_err(|e| format!("Agent Runtime endpoint missing for port {AGENT_PORT_NAME}: {e}"))?;
    let upstream = endpoint.socket_addr();

    let direct_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let direct_url = format!("http://{upstream}{READY_PATH}");
    let direct = wait_for_http(&direct_client, &direct_url, None).await?;
    if !direct.status().is_success() {
        return Err(format!(
            "published Agent /health/ready not ready directly: {}",
            direct.status()
        )
        .into());
    }

    let directory = tempfile::tempdir()?;
    let gateway_id = Uuid::now_v7();
    let (traffic_address, management_address) = unused_loopback_addresses()?;
    let managed_state_file = directory.path().join("managed-snapshot.json");
    let certificate_directory = directory.path().join("certificates");
    std::fs::create_dir_all(&certificate_directory)?;
    let config_path = directory.path().join("gateway.acl");
    std::fs::write(
        &config_path,
        management_gateway_acl(gateway_id, management_address, &managed_state_file),
    )?;

    std::env::set_var(GATEWAY_TOKEN_ENV, GATEWAY_TOKEN);
    let mut gateway = GatewayProcess::start(Path::new(gateway_bin), &config_path)?;
    wait_for_gateway_management(&mut gateway.child, management_address).await?;

    let node_config = gateway_node_agent_config(
        directory.path(),
        management_address,
        certificate_directory.clone(),
    )?;
    let installer = DurableGatewaySnapshotInstaller::from_config(
        &node_config,
        gateway_id,
        Arc::new(ArcRejectSigner),
    )?;

    let target = ManagedTarget {
        target_id: Uuid::now_v7(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
    };
    let issued_at = Utc::now();
    let snapshot = GatewaySnapshot::new(
        gateway_id,
        1,
        None,
        issued_at,
        issued_at + ChronoDuration::minutes(15),
        agent_ready_gateway_acl(
            traffic_address.port(),
            management_address.port(),
            gateway_id,
            &managed_state_file,
            upstream,
            &target,
        ),
    )?;

    let outcome = GatewaySnapshotInstaller::install(&installer, &snapshot).await?;
    if !matches!(outcome, GatewaySnapshotInstallOutcome::Applied { .. }) {
        return Err(format!("DurableGatewaySnapshotInstaller did not apply: {outcome:?}").into());
    }

    let traffic_url = format!("http://127.0.0.1:{}{READY_PATH}", traffic_address.port());
    let traffic_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = wait_for_http(&traffic_client, &traffic_url, Some(&mut gateway.child)).await?;
    if !response.status().is_success() {
        return Err(format!(
            "Gateway public /health/ready failed: {}",
            response.status()
        )
        .into());
    }

    let artifact_digest = artifact
        .rsplit_once('@')
        .map(|(_, d)| d.to_owned())
        .unwrap_or_else(|| artifact.to_owned());
    println!(
        "{GATEWAY_PUBLIC_MARKER} gateway_public=traffic path={READY_PATH} \
         gateway_revision={gateway_revision} box_revision={box_revision} \
         artifact_digest={artifact_digest} upstream={upstream} \
         traffic=127.0.0.1:{} unit_id={} generation={}",
        traffic_address.port(),
        spec.unit_id,
        spec.generation
    );
    Ok(())
}

#[cfg(target_os = "linux")]
struct ManagedTarget {
    target_id: Uuid,
    unit_id: String,
    generation: u64,
}

#[cfg(target_os = "linux")]
impl ManagedTarget {
    fn acl_object(&self) -> String {
        format!(
            "{{ target_id = \"{}\", unit_id = \"{}\", generation = {} }}",
            self.target_id, self.unit_id, self.generation
        )
    }

    #[allow(dead_code)]
    fn metric_id(&self) -> String {
        let mut identity = Sha256::new();
        identity.update(b"a3s-gateway-managed-target-v1");
        identity.update([0]);
        identity.update(self.target_id.as_bytes());
        identity.update([0]);
        identity.update(self.unit_id.as_bytes());
        identity.update([0]);
        identity.update(self.generation.to_be_bytes());
        format!("b_{:x}", identity.finalize())
    }
}

#[cfg(target_os = "linux")]
struct GatewayProcess {
    child: Child,
}

#[cfg(target_os = "linux")]
impl GatewayProcess {
    fn start(binary: &Path, config_path: &Path) -> std::io::Result<Self> {
        let child = Command::new(binary)
            .arg("--config")
            .arg(config_path)
            .env(GATEWAY_TOKEN_ENV, GATEWAY_TOKEN)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child })
    }
}

#[cfg(target_os = "linux")]
impl Drop for GatewayProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Signing transport stub: HTTP-only GA-1 smoke never requests certificates.
#[cfg(target_os = "linux")]
struct ArcRejectSigner;

#[async_trait::async_trait]
#[cfg(target_os = "linux")]
impl a3s_cloud_node_agent::GatewayCertificateSigningTransport for ArcRejectSigner {
    async fn sign_gateway_certificate(
        &self,
        _request: &a3s_cloud_contracts::GatewayCertificateSigningRequest,
    ) -> Result<
        a3s_cloud_contracts::GatewayCertificateSigningResponse,
        a3s_cloud_node_agent::NodeControlClientError,
    > {
        Err(a3s_cloud_node_agent::NodeControlClientError::Invalid(
            "GA-1 Gateway LIVE HTTP smoke does not sign certificates".into(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn unused_loopback_addresses() -> std::io::Result<(SocketAddr, SocketAddr)> {
    let traffic = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let management = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let addresses = (traffic.local_addr()?, management.local_addr()?);
    drop((traffic, management));
    Ok(addresses)
}

#[cfg(target_os = "linux")]
fn management_gateway_acl(
    gateway_id: Uuid,
    management_address: SocketAddr,
    managed_state_file: &Path,
) -> String {
    format!(
        r#"mode {{ kind = "cloud-managed" }}

managed {{
  gateway_id = "{gateway_id}"
  state_file = "{}"
}}

management {{
  enabled = true
  address = "{management_address}"
  path_prefix = "/api/gateway"
  auth_token_env = "{GATEWAY_TOKEN_ENV}"
  allowed_ips = ["127.0.0.1"]
}}
"#,
        managed_state_file.display()
    )
}

#[cfg(target_os = "linux")]
fn agent_ready_gateway_acl(
    traffic_port: u16,
    management_port: u16,
    gateway_id: Uuid,
    managed_state_file: &Path,
    upstream: SocketAddr,
    target: &ManagedTarget,
) -> String {
    let target_acl = target.acl_object();
    format!(
        r#"entrypoints "web" {{ address = "127.0.0.1:{traffic_port}" }}

routers "ga1-agent-ready" {{
  rule = "PathPrefix(`/`)"
  service = "ga1-agent-ready"
  entrypoints = ["web"]
}}

# target revision={} unit={} generation={}
services "ga1-agent-ready" {{
  load_balancer {{
    strategy = "round-robin"
    request_timeout = "5s"
    servers = [{{ url = "http://{upstream}", target = {target_acl} }}]
  }}
}}

{}
"#,
        target.target_id,
        target.unit_id,
        target.generation,
        management_gateway_acl(
            gateway_id,
            SocketAddr::from((Ipv4Addr::LOCALHOST, management_port)),
            managed_state_file,
        )
    )
}

#[cfg(target_os = "linux")]
fn gateway_node_agent_config(
    root: &Path,
    management_address: SocketAddr,
    certificate_directory: PathBuf,
) -> TestResult<NodeAgentConfig> {
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
            name: "ga1-gateway-live".into(),
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
            management_url: url::Url::parse(&format!(
                "http://{management_address}/api/gateway"
            ))?,
            auth_token_env: GATEWAY_TOKEN_ENV.into(),
            certificate_directory,
            connect_timeout_ms: 2_000,
            apply_timeout_ms: 5_000,
            readiness_timeout_ms: 10_000,
        },
    })
}

#[cfg(target_os = "linux")]
async fn wait_for_gateway_management(
    child: &mut Child,
    management_address: SocketAddr,
) -> TestResult {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?;
    let url = format!("http://{management_address}/api/gateway/version");
    for _ in 0..200 {
        if child.try_wait()?.is_some() {
            return Err("A3S Gateway exited before management API was ready".into());
        }
        if client
            .get(&url)
            .bearer_auth(GATEWAY_TOKEN)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("A3S Gateway management API did not become ready".into())
}

#[cfg(target_os = "linux")]
async fn wait_for_http(
    client: &reqwest::Client,
    url: &str,
    mut child: Option<&mut Child>,
) -> TestResult<reqwest::Response> {
    let mut last = "no HTTP response".to_owned();
    for _ in 0..100 {
        if let Some(child) = child.as_mut() {
            if child.try_wait()?.is_some() {
                return Err("A3S Gateway exited before HTTP was ready".into());
            }
        }
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => last = format!("HTTP {}", response.status()),
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("HTTP endpoint not ready ({url}): {last}").into())
}

#[cfg(target_os = "linux")]
fn cloud_pin(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[cfg(target_os = "linux")]
fn refuse_placeholder(value: &str) -> TestResult {
    if value.contains("PLACEHOLDER") {
        return Err(format!("refuse PLACEHOLDER value: {value}").into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_sidecar_revision(gateway_bin: &str) -> TestResult<String> {
    let bin = Path::new(gateway_bin);
    let sidecar = bin
        .parent()
        .map(|p| p.join("GATEWAY-REVISION"))
        .ok_or("gateway bin has no parent")?;
    let revision = std::fs::read_to_string(&sidecar)
        .map_err(|e| format!("GATEWAY-REVISION missing next to {gateway_bin}: {e}"))?
        .trim()
        .to_owned();
    if revision.len() != 40 {
        return Err(format!("invalid GATEWAY-REVISION: {revision}").into());
    }
    Ok(revision)
}
