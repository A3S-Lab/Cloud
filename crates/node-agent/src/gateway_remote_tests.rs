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

#[path = "gateway_remote_tests/replicated_gateway_tests.rs"]
mod replicated_gateway_tests;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const GATEWAY_TOKEN: &str = "a3s-cloud-gateway-integration-token";
const TLS_HOSTNAME: &str = "managed-tls.a3s.test";
const INITIAL_UPSTREAM_BODY: &str = "a3s-cloud-target-generation-1";
const REPLACEMENT_UPSTREAM_BODY: &str = "a3s-cloud-target-generation-2";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedTargetFixture {
    target_id: uuid::Uuid,
    unit_id: String,
    generation: u64,
}

impl ManagedTargetFixture {
    fn new(target_id: uuid::Uuid, unit_id: String, generation: u64) -> Self {
        Self {
            target_id,
            unit_id,
            generation,
        }
    }

    fn acl_object(&self) -> String {
        format!(
            "{{ target_id = \"{}\", unit_id = \"{}\", generation = {} }}",
            self.target_id, self.unit_id, self.generation
        )
    }

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
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl LoopbackHttpUpstream {
    async fn start(body: &'static str) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let requests = Arc::new(AtomicUsize::new(0));
        let observed_requests = requests.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0_u8; 4096];
                if stream.read(&mut request).await.is_err() {
                    continue;
                }
                observed_requests.fetch_add(1, Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        Ok(Self {
            address,
            requests,
            task,
        })
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
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
    let workload_id = uuid::Uuid::now_v7();
    let initial_revision_id = uuid::Uuid::now_v7();
    let replacement_revision_id = uuid::Uuid::now_v7();
    let initial_target = ManagedTargetFixture::new(
        initial_revision_id,
        format!("workload:{workload_id}:revision:{initial_revision_id}"),
        1,
    );
    let replacement_target = ManagedTargetFixture::new(
        replacement_revision_id,
        format!("workload:{workload_id}:revision:{replacement_revision_id}"),
        2,
    );
    let initial_upstream = LoopbackHttpUpstream::start(INITIAL_UPSTREAM_BODY).await?;
    let replacement_upstream = LoopbackHttpUpstream::start(REPLACEMENT_UPSTREAM_BODY).await?;
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
            initial_upstream.address,
            &initial_target,
        ),
    )?;
    if !matches!(
        installer.install(&first).await?,
        GatewaySnapshotInstallOutcome::Applied { .. }
    ) {
        return Err("real Gateway did not apply the first snapshot".into());
    }
    let traffic_url = format!("http://127.0.0.1:{traffic_port}/fixture");
    let traffic_client = reqwest::Client::builder().no_proxy().build()?;
    let response = wait_for_http(&traffic_client, &traffic_url, &mut gateway.child).await?;
    if response.text().await? != INITIAL_UPSTREAM_BODY {
        return Err("first managed snapshot returned an unexpected target".into());
    }
    let initial_request_count = initial_upstream.request_count();
    assert_gateway_target_metrics(&base_url, &initial_target, None).await?;
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
            replacement_upstream.address,
            &replacement_target,
        ),
    )?;
    if !matches!(
        installer.install(&second).await?,
        GatewaySnapshotInstallOutcome::Applied { .. }
    ) {
        return Err("real Gateway did not apply the second snapshot".into());
    }
    let response = wait_for_http(&traffic_client, &traffic_url, &mut gateway.child).await?;
    if response.text().await? != REPLACEMENT_UPSTREAM_BODY {
        return Err("second managed snapshot returned a stale target generation".into());
    }
    if initial_upstream.request_count() != initial_request_count {
        return Err("replacement snapshot reached the superseded target generation".into());
    }
    assert_gateway_target_metrics(&base_url, &replacement_target, Some(&initial_target)).await?;
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
    let response = wait_for_http(&traffic_client, &traffic_url, &mut gateway.child).await?;
    if response.text().await? != REPLACEMENT_UPSTREAM_BODY {
        return Err("rejected apply changed the active target generation".into());
    }
    assert_gateway_target_metrics(&base_url, &replacement_target, Some(&initial_target)).await?;

    let renewal_issued_at = Utc::now();
    let renewal = GatewaySnapshot::new(
        gateway_id,
        3,
        Some(2),
        renewal_issued_at,
        renewal_issued_at + chrono::Duration::minutes(20),
        second.acl.clone(),
    )?;
    if renewal.snapshot_digest != second.snapshot_digest {
        return Err("Gateway validity renewal changed the exact ACL digest".into());
    }
    if !matches!(
        installer.install(&renewal).await?,
        GatewaySnapshotInstallOutcome::Applied { .. }
    ) {
        return Err("real Gateway did not apply the validity renewal".into());
    }
    let renewed = control.readiness(&renewal).await?;
    if renewed.state != ManagedSnapshotState::Applied
        || !renewed.ready
        || renewed
            .applied
            .as_ref()
            .is_none_or(|identity| identity.expires_at != renewal.expires_at)
    {
        return Err("real Gateway did not expose exact renewed readiness".into());
    }
    let superseded = control.readiness(&second).await?;
    if superseded.state != ManagedSnapshotState::NotApplied || superseded.ready {
        return Err("real Gateway kept the superseded validity selector ready".into());
    }

    drop(gateway);
    let mut restarted = GatewayProcess::start(&binary, &config_path)?;
    wait_for_gateway(&base_url, &mut restarted.child).await?;
    let recovered = control.readiness(&renewal).await?;
    if recovered.state != ManagedSnapshotState::Applied || !recovered.ready {
        return Err("Gateway did not recover the renewed target snapshot exactly".into());
    }
    let response = wait_for_http(&traffic_client, &traffic_url, &mut restarted.child).await?;
    if response.text().await? != REPLACEMENT_UPSTREAM_BODY {
        return Err("Gateway restart recovered a superseded target generation".into());
    }
    assert_gateway_target_metrics(&base_url, &replacement_target, Some(&initial_target)).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a dedicated remote Gateway runner"]
async fn installed_a3s_gateway_rotates_managed_tls_and_target_generation() -> TestResult {
    let binary = required_gateway_binary()?;
    let directory = tempfile::tempdir()?;
    let (tls_port, management_port) = unused_ports();
    let node_id = uuid::Uuid::now_v7();
    let workload_id = uuid::Uuid::now_v7();
    let initial_revision_id = uuid::Uuid::now_v7();
    let replacement_revision_id = uuid::Uuid::now_v7();
    let initial_target = ManagedTargetFixture::new(
        initial_revision_id,
        format!("workload:{workload_id}:revision:{initial_revision_id}"),
        1,
    );
    let replacement_target = ManagedTargetFixture::new(
        replacement_revision_id,
        format!("workload:{workload_id}:revision:{replacement_revision_id}"),
        2,
    );
    if initial_target.metric_id() == replacement_target.metric_id() {
        return Err("managed target telemetry identity did not bind the generation".into());
    }
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

    let initial_upstream = LoopbackHttpUpstream::start(INITIAL_UPSTREAM_BODY).await?;
    let initial_certificate_id = uuid::Uuid::now_v7();
    let dns_names = vec![TLS_HOSTNAME.to_owned()];
    let certificate_root = directory.path().join("managed-certificates");
    let initial_certificate_directory = certificate_root.join(initial_certificate_id.to_string());
    let initial_certificate_request = GatewayCertificateRequest::new(
        initial_certificate_id,
        dns_names.clone(),
        initial_certificate_directory
            .join("certificate.pem")
            .to_string_lossy(),
        initial_certificate_directory
            .join("private-key.pem")
            .to_string_lossy(),
    )?;
    let initial_signer = Arc::new(FixtureGatewayCertificateSigner::new(
        node_id,
        dns_names.clone(),
    ));
    let initial_ca_bundle_pem = initial_signer.certificate_pem.clone();
    let initial_provisioner = Arc::new(NodeGatewayCertificateProvisioner::new(
        certificate_root.clone(),
        node_id,
        initial_signer.clone(),
        Arc::new(SystemGatewayCertificateClock),
    )?);
    let control = gateway_control(&base_url)?;
    let initial_installer = DurableGatewaySnapshotInstaller::new_with_certificates(
        node_id,
        control.clone(),
        initial_provisioner,
    );
    let initial_issued_at = Utc::now();
    let initial_snapshot = GatewaySnapshot::new_with_certificate(
        node_id,
        1,
        None,
        initial_issued_at,
        initial_issued_at + chrono::Duration::minutes(10),
        tls_gateway_acl(
            tls_port,
            management_port,
            initial_upstream.address,
            &initial_certificate_request,
            node_id,
            &managed_state_file,
            &initial_target,
        ),
        Some(initial_certificate_request),
    )?;
    if !initial_snapshot
        .acl
        .contains(&format!("target = {}", initial_target.acl_object()))
    {
        return Err("initial snapshot omitted the typed Cloud target identity".into());
    }
    if !matches!(
        initial_installer.install(&initial_snapshot).await?,
        GatewaySnapshotInstallOutcome::Applied { .. }
    ) {
        return Err("real Gateway did not apply the managed TLS snapshot".into());
    }
    if initial_signer.calls.load(Ordering::SeqCst) != 1 {
        return Err("managed TLS fixture did not perform exactly one signing request".into());
    }

    let url = format!("https://{TLS_HOSTNAME}:{tls_port}/fixture");
    let initial_client = managed_tls_client(&initial_ca_bundle_pem, tls_port)?;
    let response = wait_for_https(&initial_client, &url, &mut gateway.child).await?;
    if response.text().await? != INITIAL_UPSTREAM_BODY {
        return Err("initial managed TLS route returned an unexpected target".into());
    }
    assert_gateway_target_metrics(&base_url, &initial_target, None).await?;
    let initial_request_count = initial_upstream.request_count();
    let initial_status = control.readiness(&initial_snapshot).await?;
    if initial_status.state != ManagedSnapshotState::Applied || !initial_status.ready {
        return Err("initial managed TLS snapshot was not exactly ready".into());
    }

    let replacement_upstream = LoopbackHttpUpstream::start(REPLACEMENT_UPSTREAM_BODY).await?;
    let replacement_certificate_id = uuid::Uuid::now_v7();
    let replacement_certificate_directory =
        certificate_root.join(replacement_certificate_id.to_string());
    let replacement_certificate_request = GatewayCertificateRequest::new(
        replacement_certificate_id,
        dns_names.clone(),
        replacement_certificate_directory
            .join("certificate.pem")
            .to_string_lossy(),
        replacement_certificate_directory
            .join("private-key.pem")
            .to_string_lossy(),
    )?;
    let replacement_signer = Arc::new(FixtureGatewayCertificateSigner::new(node_id, dns_names));
    let replacement_ca_bundle_pem = replacement_signer.certificate_pem.clone();
    let replacement_provisioner = Arc::new(NodeGatewayCertificateProvisioner::new(
        certificate_root,
        node_id,
        replacement_signer.clone(),
        Arc::new(SystemGatewayCertificateClock),
    )?);
    let replacement_installer = DurableGatewaySnapshotInstaller::new_with_certificates(
        node_id,
        control.clone(),
        replacement_provisioner,
    );
    let replacement_issued_at = Utc::now();
    let replacement_snapshot = GatewaySnapshot::new_with_certificate(
        node_id,
        2,
        Some(1),
        replacement_issued_at,
        replacement_issued_at + chrono::Duration::minutes(10),
        tls_gateway_acl(
            tls_port,
            management_port,
            replacement_upstream.address,
            &replacement_certificate_request,
            node_id,
            &managed_state_file,
            &replacement_target,
        ),
        Some(replacement_certificate_request),
    )?;
    if !replacement_snapshot
        .acl
        .contains(&format!("target = {}", replacement_target.acl_object()))
    {
        return Err("replacement snapshot omitted the typed Cloud target identity".into());
    }
    if !matches!(
        replacement_installer.install(&replacement_snapshot).await?,
        GatewaySnapshotInstallOutcome::Applied { .. }
    ) {
        return Err("real Gateway did not apply the replacement TLS snapshot".into());
    }
    if replacement_signer.calls.load(Ordering::SeqCst) != 1
        || initial_signer.calls.load(Ordering::SeqCst) != 1
    {
        return Err("certificate replacement did not issue each identity exactly once".into());
    }

    let replacement_client = managed_tls_client(&replacement_ca_bundle_pem, tls_port)?;
    let response = wait_for_https(&replacement_client, &url, &mut gateway.child).await?;
    if response.text().await? != REPLACEMENT_UPSTREAM_BODY {
        return Err("replacement snapshot exposed a stale target generation".into());
    }
    assert_gateway_target_metrics(&base_url, &replacement_target, Some(&initial_target)).await?;
    if initial_upstream.request_count() != initial_request_count {
        return Err("replacement traffic reached the superseded target generation".into());
    }
    if initial_client.get(&url).send().await.is_ok() {
        return Err("replacement snapshot continued serving the superseded certificate".into());
    }
    let replacement_status = control.readiness(&replacement_snapshot).await?;
    if replacement_status.state != ManagedSnapshotState::Applied || !replacement_status.ready {
        return Err("replacement managed TLS snapshot was not exactly ready".into());
    }
    let superseded_status = control.readiness(&initial_snapshot).await?;
    if superseded_status.state != ManagedSnapshotState::NotApplied || superseded_status.ready {
        return Err("superseded certificate and target selector remained ready".into());
    }
    if !tokio::fs::try_exists(&managed_state_file).await? {
        return Err("managed TLS fixture omitted the Gateway-native durable journal".into());
    }

    tokio::fs::remove_dir_all(initial_certificate_directory).await?;
    drop(initial_upstream);
    drop(gateway);

    let mut restarted = GatewayProcess::start(&binary, &config_path)?;
    wait_for_gateway(&base_url, &mut restarted.child).await?;
    let recovered = control.readiness(&replacement_snapshot).await?;
    if recovered.state != ManagedSnapshotState::Applied || !recovered.ready {
        return Err("Gateway did not recover the replacement snapshot exactly".into());
    }
    let response = wait_for_https(&replacement_client, &url, &mut restarted.child).await?;
    if response.text().await? != REPLACEMENT_UPSTREAM_BODY {
        return Err("Gateway restart recovered a superseded target generation".into());
    }
    assert_gateway_target_metrics(&base_url, &replacement_target, Some(&initial_target)).await?;
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

fn managed_tls_client(ca_bundle_pem: &str, tls_port: u16) -> TestResult<reqwest::Client> {
    let root = reqwest::Certificate::from_pem(ca_bundle_pem.as_bytes())?;
    Ok(reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .tls_built_in_root_certs(false)
        .add_root_certificate(root)
        .resolve(TLS_HOSTNAME, SocketAddr::from(([127, 0, 0, 1], tls_port)))
        .timeout(Duration::from_secs(2))
        .build()?)
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
    upstream: SocketAddr,
    target: &ManagedTargetFixture,
) -> String {
    let target_acl = target.acl_object();
    format!(
        r#"# revision {revision}
entrypoints "web" {{ address = "127.0.0.1:{traffic_port}" }}

routers "managed-http-fixture" {{
  rule = "PathPrefix(`/`)"
  service = "managed-http-fixture"
  entrypoints = ["web"]
}}

# target revision={} unit={} generation={}
services "managed-http-fixture" {{
  load_balancer {{
    strategy = "round-robin"
    request_timeout = "2s"
    servers = [{{ url = "http://{upstream}", target = {target_acl} }}]
  }}
}}

{}
"#,
        target.target_id,
        target.unit_id,
        target.generation,
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
    target: &ManagedTargetFixture,
) -> String {
    let target_acl = target.acl_object();
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

# target revision={} unit={} generation={}
services "managed-tls-fixture" {{
  load_balancer {{
    strategy = "round-robin"
    request_timeout = "2s"
    servers = [{{ url = "http://{upstream}", target = {target_acl} }}]
  }}
}}

{}
"#,
        certificate.certificate_file,
        certificate.private_key_file,
        target.target_id,
        target.unit_id,
        target.generation,
        management_gateway_acl(management_port, gateway_id, managed_state_file)
    )
}

async fn assert_gateway_target_metrics(
    base_url: &str,
    expected: &ManagedTargetFixture,
    superseded: Option<&ManagedTargetFixture>,
) -> TestResult {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .build()?;
    let output = client
        .get(format!("{base_url}/metrics"))
        .bearer_auth(GATEWAY_TOKEN)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected_metric_id = expected.metric_id();
    if !output.contains(&format!("backend_id=\"{expected_metric_id}\"")) {
        return Err("Gateway telemetry omitted the exact managed target identity".into());
    }
    if output.contains(&expected.target_id.to_string()) || output.contains(&expected.unit_id) {
        return Err("Gateway telemetry exposed raw managed target identity".into());
    }
    if let Some(superseded) = superseded {
        let superseded_metric_id = superseded.metric_id();
        if output.contains(&format!("backend_id=\"{superseded_metric_id}\"")) {
            return Err("Gateway telemetry retained the superseded target generation".into());
        }
    }
    Ok(())
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
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .build()?;
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

async fn wait_for_http(
    client: &reqwest::Client,
    url: &str,
    child: &mut Child,
) -> TestResult<reqwest::Response> {
    let mut last_failure = "no HTTP response".to_owned();
    for _ in 0..100 {
        if child.try_wait()?.is_some() {
            return Err("A3S Gateway exited before managed HTTP was ready".into());
        }
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => last_failure = format!("HTTP {}", response.status()),
            Err(error) => last_failure = error.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("A3S Gateway managed HTTP endpoint did not become ready: {last_failure}").into())
}
