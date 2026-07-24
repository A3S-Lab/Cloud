use super::*;
use crate::{GatewayCertificateSigningTransport, NodeControlClientError};
use a3s_cloud_contracts::{
    GatewayCertificateRequest, GatewayCertificateSigningRequest, GatewayCertificateSigningResponse,
};
use chrono::{TimeZone, Utc};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateSigningRequestParams,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType, SerialNumber,
};
use sha2::{Digest, Sha256};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const GATEWAY_TOKEN: &str = "a3s-cloud-gateway-integration-token";
const TLS_HOSTNAME: &str = "managed-tls.a3s.test";
const UPSTREAM_BODY: &str = "a3s-cloud-managed-tls-ok";

struct FixtureGatewayCertificateSigner {
    node_id: uuid::Uuid,
    dns_names: Vec<String>,
    certificate: Certificate,
    certificate_pem: String,
    private_key: KeyPair,
    calls: AtomicUsize,
}

impl FixtureGatewayCertificateSigner {
    fn new(node_id: uuid::Uuid, dns_names: Vec<String>) -> Self {
        let private_key = KeyPair::generate().expect("fixture Gateway CA key");
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let certificate = params
            .self_signed(&private_key)
            .expect("fixture Gateway CA");
        let certificate_pem = certificate.pem();
        Self {
            node_id,
            dns_names,
            certificate,
            certificate_pem,
            private_key,
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl GatewayCertificateSigningTransport for FixtureGatewayCertificateSigner {
    async fn sign_gateway_certificate(
        &self,
        request: &GatewayCertificateSigningRequest,
    ) -> Result<GatewayCertificateSigningResponse, NodeControlClientError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request.node_id != self.node_id {
            return Err(NodeControlClientError::Invalid(
                "fixture Gateway signing node changed".into(),
            ));
        }
        let mut csr = CertificateSigningRequestParams::from_pem(&request.csr_pem)
            .map_err(|error| NodeControlClientError::Invalid(error.to_string()))?;
        let serial = SerialNumber::from_slice(request.certificate_id.as_bytes());
        csr.params.serial_number = Some(serial.clone());
        csr.params.is_ca = IsCa::NoCa;
        csr.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        csr.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        csr.params.subject_alt_names = self
            .dns_names
            .iter()
            .map(|dns_name| {
                dns_name
                    .as_str()
                    .try_into()
                    .map(SanType::DnsName)
                    .map_err(|error| NodeControlClientError::Invalid(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let issued_at = Utc
            .timestamp_opt(csr.params.not_before.unix_timestamp(), 0)
            .single()
            .ok_or_else(|| {
                NodeControlClientError::Invalid("fixture Gateway issue timestamp is invalid".into())
            })?;
        let expires_at = Utc
            .timestamp_opt(csr.params.not_after.unix_timestamp(), 0)
            .single()
            .ok_or_else(|| {
                NodeControlClientError::Invalid(
                    "fixture Gateway expiry timestamp is invalid".into(),
                )
            })?;
        let certificate = csr
            .signed_by(&self.certificate, &self.private_key)
            .map_err(|error| NodeControlClientError::Invalid(error.to_string()))?;
        Ok(GatewayCertificateSigningResponse {
            schema: GatewayCertificateSigningResponse::SCHEMA.into(),
            certificate_id: request.certificate_id,
            node_id: request.node_id,
            dns_names: self.dns_names.clone(),
            serial_number: serial.to_string(),
            fingerprint: format!("sha256:{:x}", Sha256::digest(certificate.der())),
            certificate_pem: certificate.pem(),
            ca_bundle_pem: self.certificate_pem.clone(),
            issued_at,
            expires_at,
        })
    }
}

struct GatewayProcess {
    child: Child,
}

impl GatewayProcess {
    fn start(binary: &str, config_path: &Path) -> std::io::Result<Self> {
        let child = Command::new(binary)
            .arg("--config")
            .arg(config_path)
            .env("A3S_GATEWAY_ADMIN_TOKEN", GATEWAY_TOKEN)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child })
    }
}

impl Drop for GatewayProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct LoopbackHttpUpstream {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl LoopbackHttpUpstream {
    async fn start() -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0_u8; 4096];
                if stream.read(&mut request).await.is_err() {
                    continue;
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    UPSTREAM_BODY.len(),
                    UPSTREAM_BODY
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        Ok(Self { address, task })
    }
}

impl Drop for LoopbackHttpUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
#[ignore = "requires a dedicated remote Gateway runner"]
async fn installed_a3s_gateway_validates_and_reloads_complete_snapshots() -> TestResult {
    let binary = required_gateway_binary()?;
    let directory = tempfile::tempdir()?;
    let (traffic_port, management_port) = unused_ports();
    let gateway_id = uuid::Uuid::now_v7();
    let managed_state_file = directory.path().join("managed-snapshot.json");
    let bootstrap = management_gateway_acl(management_port, gateway_id, &managed_state_file);
    let config_path = directory.path().join("gateway.acl");
    std::fs::write(&config_path, &bootstrap)?;
    let mut gateway = GatewayProcess::start(&binary, &config_path)?;

    let base_url = format!("http://127.0.0.1:{management_port}/api/gateway");
    wait_for_gateway(&base_url, &mut gateway.child).await?;
    let control = gateway_control(&base_url)?;
    let installer = DurableGatewaySnapshotInstaller::new(gateway_id, control.clone());
    let first_issued_at = Utc::now();
    let first = GatewaySnapshot::new(
        gateway_id,
        1,
        None,
        first_issued_at,
        first_issued_at + chrono::Duration::minutes(10),
        gateway_acl(
            traffic_port,
            management_port,
            gateway_id,
            &managed_state_file,
            1,
        ),
    )?;
    if installer.install(&first).await? != GatewaySnapshotInstallOutcome::Applied {
        return Err("real Gateway did not apply the first snapshot".into());
    }
    let second_issued_at = Utc::now();
    let second = GatewaySnapshot::new(
        gateway_id,
        2,
        Some(1),
        second_issued_at,
        second_issued_at + chrono::Duration::minutes(10),
        gateway_acl(
            traffic_port,
            management_port,
            gateway_id,
            &managed_state_file,
            2,
        ),
    )?;
    if installer.install(&second).await? != GatewaySnapshotInstallOutcome::Applied {
        return Err("real Gateway did not apply the second snapshot".into());
    }
    let invalid_issued_at = Utc::now();
    let invalid = GatewaySnapshot::new(
        gateway_id,
        3,
        Some(2),
        invalid_issued_at,
        invalid_issued_at + chrono::Duration::minutes(10),
        invalid_gateway_acl(management_port, gateway_id, &managed_state_file),
    )?;
    if !matches!(
        installer.install(&invalid).await?,
        GatewaySnapshotInstallOutcome::Rejected { .. }
    ) {
        return Err("real Gateway accepted invalid ACL".into());
    }
    let retained = control.readiness(&second).await?;
    if retained.state != ManagedSnapshotState::Applied || !retained.ready {
        return Err("rejected native Gateway apply changed the prior ready snapshot".into());
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires a dedicated remote Gateway runner"]
async fn installed_a3s_gateway_serves_managed_tls_after_exact_snapshot_reload() -> TestResult {
    let binary = required_gateway_binary()?;
    let directory = tempfile::tempdir()?;
    let (tls_port, management_port) = unused_ports();
    let node_id = uuid::Uuid::now_v7();
    let managed_state_file = directory.path().join("managed-snapshot.json");
    let config_path = directory.path().join("gateway.acl");
    std::fs::write(
        &config_path,
        management_gateway_acl(management_port, node_id, &managed_state_file),
    )?;
    let mut gateway = GatewayProcess::start(&binary, &config_path)?;

    let base_url = format!("http://127.0.0.1:{management_port}/api/gateway");
    wait_for_gateway(&base_url, &mut gateway.child).await?;
    if tokio::net::TcpStream::connect(("127.0.0.1", tls_port))
        .await
        .is_ok()
    {
        return Err("Gateway TLS port was available before snapshot reload".into());
    }

    let upstream = LoopbackHttpUpstream::start().await?;
    let certificate_id = uuid::Uuid::now_v7();
    let dns_names = vec![TLS_HOSTNAME.to_owned()];
    let certificate_root = directory.path().join("managed-certificates");
    let certificate_directory = certificate_root.join(certificate_id.to_string());
    let certificate_request = GatewayCertificateRequest::new(
        certificate_id,
        dns_names.clone(),
        certificate_directory
            .join("certificate.pem")
            .to_string_lossy(),
        certificate_directory
            .join("private-key.pem")
            .to_string_lossy(),
    )?;
    let signer = Arc::new(FixtureGatewayCertificateSigner::new(node_id, dns_names));
    let ca_bundle_pem = signer.certificate_pem.clone();
    let provisioner = Arc::new(NodeGatewayCertificateProvisioner::new(
        certificate_root,
        node_id,
        signer.clone(),
        Arc::new(SystemGatewayCertificateClock),
    )?);
    let control = gateway_control(&base_url)?;
    let installer = DurableGatewaySnapshotInstaller::new_with_certificates(
        node_id,
        control.clone(),
        provisioner,
    );
    let issued_at = Utc::now();
    let snapshot = GatewaySnapshot::new_with_certificate(
        node_id,
        1,
        None,
        issued_at,
        issued_at + chrono::Duration::minutes(10),
        tls_gateway_acl(
            tls_port,
            management_port,
            upstream.address,
            &certificate_request,
            node_id,
            &managed_state_file,
        ),
        Some(certificate_request),
    )?;
    if installer.install(&snapshot).await? != GatewaySnapshotInstallOutcome::Applied {
        return Err("real Gateway did not apply the managed TLS snapshot".into());
    }
    if signer.calls.load(Ordering::SeqCst) != 1 {
        return Err("managed TLS fixture did not perform exactly one signing request".into());
    }

    let root = reqwest::Certificate::from_pem(ca_bundle_pem.as_bytes())?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .tls_built_in_root_certs(false)
        .add_root_certificate(root)
        .resolve(TLS_HOSTNAME, SocketAddr::from(([127, 0, 0, 1], tls_port)))
        .timeout(Duration::from_secs(2))
        .build()?;
    let response = wait_for_https(
        &client,
        &format!("https://{TLS_HOSTNAME}:{tls_port}/fixture"),
        &mut gateway.child,
    )
    .await?;
    if response.text().await? != UPSTREAM_BODY {
        return Err("managed TLS route returned an unexpected upstream response".into());
    }
    let status = control.readiness(&snapshot).await?;
    if status.state != ManagedSnapshotState::Applied || !status.ready {
        return Err("managed TLS fixture did not preserve exact Gateway readiness".into());
    }
    if !tokio::fs::try_exists(&managed_state_file).await? {
        return Err("managed TLS fixture omitted the Gateway-native durable journal".into());
    }
    Ok(())
}

fn required_gateway_binary() -> TestResult<String> {
    std::env::var("A3S_CLOUD_TEST_GATEWAY_BIN")
        .map_err(|_| "A3S_CLOUD_TEST_GATEWAY_BIN is required for remote Gateway tests".into())
}

fn gateway_control(
    base_url: &str,
) -> Result<Arc<GatewayManagementClient>, GatewaySnapshotInstallError> {
    Ok(Arc::new(GatewayManagementClient::new(
        url::Url::parse(base_url)
            .map_err(|error| GatewaySnapshotInstallError::InvalidState(error.to_string()))?,
        GATEWAY_TOKEN.into(),
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_secs(5),
    )?))
}

fn unused_ports() -> (u16, u16) {
    let traffic = TcpListener::bind("127.0.0.1:0").expect("bind traffic port");
    let management = TcpListener::bind("127.0.0.1:0").expect("bind management port");
    let ports = (
        traffic.local_addr().expect("traffic address").port(),
        management.local_addr().expect("management address").port(),
    );
    drop((traffic, management));
    ports
}

fn gateway_acl(
    traffic_port: u16,
    management_port: u16,
    gateway_id: uuid::Uuid,
    managed_state_file: &Path,
    revision: u64,
) -> String {
    format!(
        r#"# revision {revision}
entrypoints "web" {{ address = "127.0.0.1:{traffic_port}" }}

{}
"#,
        management_gateway_acl(management_port, gateway_id, managed_state_file)
    )
}

fn tls_gateway_acl(
    tls_port: u16,
    management_port: u16,
    upstream: SocketAddr,
    certificate: &GatewayCertificateRequest,
    gateway_id: uuid::Uuid,
    managed_state_file: &Path,
) -> String {
    format!(
        r#"entrypoints "a3s-cloud-https" {{
  address = "127.0.0.1:{tls_port}"
  tls {{
    cert_file = "{}"
    key_file = "{}"
    min_version = "1.2"
  }}
}}

routers "managed-tls-fixture" {{
  rule = "Host(`{TLS_HOSTNAME}`) && PathPrefix(`/`)"
  service = "managed-tls-fixture"
  entrypoints = ["a3s-cloud-https"]
}}

services "managed-tls-fixture" {{
  load_balancer {{
    strategy = "round-robin"
    request_timeout = "2s"
    servers = [{{ url = "http://{upstream}" }}]
  }}
}}

{}
"#,
        certificate.certificate_file,
        certificate.private_key_file,
        management_gateway_acl(management_port, gateway_id, managed_state_file)
    )
}

fn management_gateway_acl(
    management_port: u16,
    gateway_id: uuid::Uuid,
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
  address = "127.0.0.1:{management_port}"
  path_prefix = "/api/gateway"
  auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  allowed_ips = ["127.0.0.1"]
}}"#,
        managed_state_file.display()
    )
}

fn invalid_gateway_acl(
    management_port: u16,
    gateway_id: uuid::Uuid,
    managed_state_file: &Path,
) -> String {
    format!(
        r#"entrypoints "web" {{ address = "invalid-address" }}

{}
"#,
        management_gateway_acl(management_port, gateway_id, managed_state_file)
    )
}

async fn wait_for_gateway(base_url: &str, child: &mut Child) -> TestResult {
    let client = reqwest::Client::builder().no_proxy().build()?;
    for _ in 0..100 {
        if child.try_wait()?.is_some() {
            return Err("A3S Gateway exited before its management API was ready".into());
        }
        if client
            .get(format!("{base_url}/version"))
            .bearer_auth(GATEWAY_TOKEN)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("A3S Gateway management API did not become ready".into())
}

async fn wait_for_https(
    client: &reqwest::Client,
    url: &str,
    child: &mut Child,
) -> TestResult<reqwest::Response> {
    let mut last_failure = "no HTTPS response".to_owned();
    for _ in 0..100 {
        if child.try_wait()?.is_some() {
            return Err("A3S Gateway exited before managed TLS was ready".into());
        }
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                return Ok(response);
            }
            Ok(response) => {
                last_failure = format!("HTTP {}", response.status());
            }
            Err(error) => {
                last_failure = error.to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("A3S Gateway managed TLS endpoint did not become ready: {last_failure}").into())
}
