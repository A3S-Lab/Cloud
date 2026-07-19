use crate::gateway_certificate::{
    GatewayCertificateProvisioningError, NodeGatewayCertificateProvisioner,
    SystemGatewayCertificateClock,
};
use crate::state_file::{self, SecureStateError};
use crate::{GatewayCertificateSigningTransport, NodeAgentConfig};
use a3s_cloud_contracts::GatewaySnapshot;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const MAX_MANAGEMENT_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewaySnapshotInstallOutcome {
    Applied,
    Rejected { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum GatewaySnapshotInstallError {
    #[error("invalid Gateway snapshot state: {0}")]
    InvalidState(String),
    #[error("Gateway snapshot storage failed: {0}")]
    Storage(String),
    #[error("Gateway management API is unavailable: {0}")]
    Unavailable(String),
    #[error("Gateway management protocol failed: {0}")]
    Protocol(String),
}

impl GatewaySnapshotInstallError {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Storage(_) | Self::Unavailable(_))
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidState(_) => "gateway_state_invalid",
            Self::Storage(_) => "gateway_state_storage",
            Self::Unavailable(_) => "gateway_unavailable",
            Self::Protocol(_) => "gateway_protocol",
        }
    }
}

#[async_trait]
pub trait GatewaySnapshotInstaller: Send + Sync {
    async fn install(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError>;
}

#[derive(Debug, thiserror::Error)]
enum GatewayControlError {
    #[error("{0}")]
    Rejected(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Protocol(String),
}

#[async_trait]
trait GatewayControl: Send + Sync {
    async fn validate(&self, acl: &str) -> Result<(), GatewayControlError>;
    async fn reload(&self, acl: &str) -> Result<(), GatewayControlError>;
}

pub struct DurableGatewaySnapshotInstaller {
    state_file: PathBuf,
    control: Arc<dyn GatewayControl>,
    certificates: Option<Arc<NodeGatewayCertificateProvisioner>>,
    installation: Mutex<()>,
}

impl DurableGatewaySnapshotInstaller {
    pub fn from_config(
        config: &NodeAgentConfig,
        node_id: uuid::Uuid,
        transport: Arc<dyn GatewayCertificateSigningTransport>,
    ) -> Result<Self, GatewaySnapshotInstallError> {
        let token = config
            .gateway_auth_token()
            .map_err(|error| GatewaySnapshotInstallError::InvalidState(error.to_string()))?;
        let control = Arc::new(GatewayManagementClient::new(
            config.gateway.management_url.clone(),
            token,
            Duration::from_millis(config.gateway.connect_timeout_ms),
            Duration::from_millis(config.gateway.validation_timeout_ms),
            Duration::from_millis(config.gateway.reload_timeout_ms),
        )?);
        let certificates = Arc::new(
            NodeGatewayCertificateProvisioner::new(
                config.gateway.certificate_directory.clone(),
                node_id,
                transport,
                Arc::new(SystemGatewayCertificateClock),
            )
            .map_err(map_certificate_error)?,
        );
        Ok(Self::new_with_certificates(
            config.gateway.state_file.clone(),
            control,
            certificates,
        ))
    }

    #[cfg(test)]
    fn new(state_file: PathBuf, control: Arc<dyn GatewayControl>) -> Self {
        Self {
            state_file,
            control,
            certificates: None,
            installation: Mutex::new(()),
        }
    }

    fn new_with_certificates(
        state_file: PathBuf,
        control: Arc<dyn GatewayControl>,
        certificates: Arc<NodeGatewayCertificateProvisioner>,
    ) -> Self {
        Self {
            state_file,
            control,
            certificates: Some(certificates),
            installation: Mutex::new(()),
        }
    }

    async fn read_installed(
        &self,
    ) -> Result<Option<InstalledGatewaySnapshot>, GatewaySnapshotInstallError> {
        let path = self.state_file.clone();
        tokio::task::spawn_blocking(move || {
            state_file::read_json(&path, "installed Gateway snapshot")
                .map_err(map_state_error)
                .and_then(|state: Option<InstalledGatewaySnapshot>| {
                    if let Some(state) = &state {
                        state.validate()?;
                    }
                    Ok(state)
                })
        })
        .await
        .map_err(|error| {
            GatewaySnapshotInstallError::Storage(format!(
                "Gateway state reader task failed: {error}"
            ))
        })?
    }

    async fn write_installed(
        &self,
        snapshot: GatewaySnapshot,
    ) -> Result<(), GatewaySnapshotInstallError> {
        let path = self.state_file.clone();
        tokio::task::spawn_blocking(move || {
            let parent = path.parent().ok_or_else(|| {
                GatewaySnapshotInstallError::InvalidState(
                    "Gateway state file has no parent directory".into(),
                )
            })?;
            state_file::ensure_directory(parent).map_err(map_state_error)?;
            state_file::atomic_write(
                &path,
                &InstalledGatewaySnapshot {
                    schema: InstalledGatewaySnapshot::SCHEMA.into(),
                    snapshot,
                },
            )
            .map_err(map_state_error)
        })
        .await
        .map_err(|error| {
            GatewaySnapshotInstallError::Storage(format!(
                "Gateway state writer task failed: {error}"
            ))
        })?
    }

    fn call_control(
        result: Result<(), GatewayControlError>,
    ) -> Result<Option<GatewaySnapshotInstallOutcome>, GatewaySnapshotInstallError> {
        match result {
            Ok(()) => Ok(None),
            Err(GatewayControlError::Rejected(message)) => {
                Ok(Some(GatewaySnapshotInstallOutcome::Rejected {
                    message: sanitize_message(&message, "Gateway rejected the snapshot"),
                }))
            }
            Err(GatewayControlError::Unavailable(message)) => {
                Err(GatewaySnapshotInstallError::Unavailable(sanitize_message(
                    &message,
                    "Gateway management API is unavailable",
                )))
            }
            Err(GatewayControlError::Protocol(message)) => {
                Err(GatewaySnapshotInstallError::Protocol(sanitize_message(
                    &message,
                    "Gateway management response is invalid",
                )))
            }
        }
    }

    async fn provision_certificate(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<Option<GatewaySnapshotInstallOutcome>, GatewaySnapshotInstallError> {
        let Some(request) = snapshot.certificate_request.as_ref() else {
            return Ok(None);
        };
        let Some(certificates) = self.certificates.as_ref() else {
            return Ok(Some(GatewaySnapshotInstallOutcome::Rejected {
                message: "Gateway certificate provisioner is not configured".into(),
            }));
        };
        match certificates.provision(request).await {
            Ok(()) => Ok(None),
            Err(GatewayCertificateProvisioningError::Invalid(message)) => {
                Ok(Some(GatewaySnapshotInstallOutcome::Rejected {
                    message: sanitize_message(
                        &message,
                        "Gateway certificate provisioning was rejected",
                    ),
                }))
            }
            Err(error) => Err(map_certificate_error(error)),
        }
    }
}

#[async_trait]
impl GatewaySnapshotInstaller for DurableGatewaySnapshotInstaller {
    async fn install(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError> {
        if let Err(error) = snapshot.validate() {
            return Ok(GatewaySnapshotInstallOutcome::Rejected {
                message: sanitize_message(&error, "Gateway snapshot is invalid"),
            });
        }
        let _installation = self.installation.lock().await;
        let installed = self.read_installed().await?;
        if installed.as_ref().is_some_and(|installed| {
            installed.snapshot.revision == snapshot.revision
                && installed.snapshot.snapshot_digest == snapshot.snapshot_digest
                && installed.snapshot.acl == snapshot.acl
        }) {
            if let Some(outcome) = self.provision_certificate(snapshot).await? {
                return Ok(outcome);
            }
            return Ok(GatewaySnapshotInstallOutcome::Applied);
        }
        let installed_revision = installed.as_ref().map(|state| state.snapshot.revision);
        if installed_revision != snapshot.expected_revision {
            return Ok(GatewaySnapshotInstallOutcome::Rejected {
                message: format!(
                    "Gateway snapshot expected revision {}, but node has revision {}",
                    display_revision(snapshot.expected_revision),
                    display_revision(installed_revision)
                ),
            });
        }
        if installed_revision.is_some_and(|revision| revision >= snapshot.revision) {
            return Ok(GatewaySnapshotInstallOutcome::Rejected {
                message: "Gateway snapshot revision does not advance the installed revision".into(),
            });
        }
        if let Some(outcome) = self.provision_certificate(snapshot).await? {
            return Ok(outcome);
        }
        if let Some(outcome) = Self::call_control(self.control.validate(&snapshot.acl).await)? {
            return Ok(outcome);
        }
        if let Some(outcome) = Self::call_control(self.control.reload(&snapshot.acl).await)? {
            return Ok(outcome);
        }
        self.write_installed(snapshot.clone()).await?;
        Ok(GatewaySnapshotInstallOutcome::Applied)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledGatewaySnapshot {
    schema: String,
    snapshot: GatewaySnapshot,
}

impl InstalledGatewaySnapshot {
    const SCHEMA: &'static str = "a3s.cloud.installed-gateway-snapshot.v1";

    fn validate(&self) -> Result<(), GatewaySnapshotInstallError> {
        if self.schema != Self::SCHEMA {
            return Err(GatewaySnapshotInstallError::InvalidState(format!(
                "unsupported installed Gateway snapshot schema {:?}",
                self.schema
            )));
        }
        self.snapshot
            .validate()
            .map_err(GatewaySnapshotInstallError::InvalidState)
    }
}

struct GatewayManagementClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
    validation_timeout: Duration,
    reload_timeout: Duration,
}

impl GatewayManagementClient {
    fn new(
        management_url: url::Url,
        token: String,
        connect_timeout: Duration,
        validation_timeout: Duration,
        reload_timeout: Duration,
    ) -> Result<Self, GatewaySnapshotInstallError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(connect_timeout)
            .build()
            .map_err(|error| GatewaySnapshotInstallError::Protocol(error.to_string()))?;
        Ok(Self {
            client,
            base_url: management_url.as_str().trim_end_matches('/').into(),
            token,
            validation_timeout,
            reload_timeout,
        })
    }

    async fn mutate(&self, action: &'static str, acl: &str) -> Result<(), GatewayControlError> {
        let response = self
            .client
            .post(format!("{}/config/{action}", self.base_url))
            .bearer_auth(&self.token)
            .header(reqwest::header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(acl.to_owned())
            .timeout(if action == "reload" {
                self.reload_timeout
            } else {
                self.validation_timeout
            })
            .send()
            .await
            .map_err(|error| GatewayControlError::Unavailable(error.to_string()))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MANAGEMENT_RESPONSE_BYTES as u64)
        {
            return Err(GatewayControlError::Protocol(
                "Gateway management response exceeds 64 KiB".into(),
            ));
        }
        let body = response
            .bytes()
            .await
            .map_err(|error| GatewayControlError::Unavailable(error.to_string()))?;
        if body.len() > MAX_MANAGEMENT_RESPONSE_BYTES {
            return Err(GatewayControlError::Protocol(
                "Gateway management response exceeds 64 KiB".into(),
            ));
        }
        if !status.is_success() {
            let message = management_error_message(&body, status.as_u16());
            return Err(
                if status.is_server_error()
                    || status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                {
                    GatewayControlError::Unavailable(message)
                } else if status == reqwest::StatusCode::BAD_REQUEST
                    || status == reqwest::StatusCode::UNPROCESSABLE_ENTITY
                {
                    GatewayControlError::Rejected(message)
                } else {
                    GatewayControlError::Protocol(message)
                },
            );
        }
        let mutation: GatewayMutationResponse = serde_json::from_slice(&body)
            .map_err(|error| GatewayControlError::Protocol(error.to_string()))?;
        let expected_reload = action == "reload";
        if !mutation.valid || mutation.reloaded != expected_reload {
            return Err(GatewayControlError::Protocol(format!(
                "Gateway {action} response did not confirm the requested mutation"
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl GatewayControl for GatewayManagementClient {
    async fn validate(&self, acl: &str) -> Result<(), GatewayControlError> {
        self.mutate("validate", acl).await
    }

    async fn reload(&self, acl: &str) -> Result<(), GatewayControlError> {
        self.mutate("reload", acl).await
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GatewayMutationResponse {
    valid: bool,
    reloaded: bool,
    #[allow(dead_code)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct GatewayErrorResponse {
    error: String,
}

fn management_error_message(body: &[u8], status: u16) -> String {
    serde_json::from_slice::<GatewayErrorResponse>(body)
        .ok()
        .map(|response| sanitize_message(&response.error, "Gateway rejected the request"))
        .unwrap_or_else(|| format!("Gateway management request failed with HTTP {status}"))
}

fn map_state_error(error: SecureStateError) -> GatewaySnapshotInstallError {
    match error {
        SecureStateError::Invalid(message) => GatewaySnapshotInstallError::InvalidState(message),
        SecureStateError::Storage(message) => GatewaySnapshotInstallError::Storage(message),
    }
}

fn map_certificate_error(
    error: GatewayCertificateProvisioningError,
) -> GatewaySnapshotInstallError {
    match error {
        GatewayCertificateProvisioningError::Invalid(message) => {
            GatewaySnapshotInstallError::InvalidState(message)
        }
        GatewayCertificateProvisioningError::Storage(message) => {
            GatewaySnapshotInstallError::Storage(message)
        }
        GatewayCertificateProvisioningError::ControlPlane(error) if error.retryable() => {
            GatewaySnapshotInstallError::Unavailable(
                "Gateway certificate signing request could not complete".into(),
            )
        }
        GatewayCertificateProvisioningError::ControlPlane(_) => {
            GatewaySnapshotInstallError::Protocol(
                "Gateway certificate signing request was rejected".into(),
            )
        }
    }
}

fn display_revision(revision: Option<u64>) -> String {
    revision.map_or_else(|| "none".into(), |revision| revision.to_string())
}

fn sanitize_message(message: &str, fallback: &str) -> String {
    let message = message.replace(['\0', '\r', '\n'], " ");
    let message = message.trim();
    if message.is_empty() {
        fallback.into()
    } else {
        let mut sanitized = String::with_capacity(message.len().min(16 * 1024));
        for character in message.chars() {
            if sanitized.len() + character.len_utf8() > 16 * 1024 {
                break;
            }
            sanitized.push(character);
        }
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct FakeGatewayControl {
        calls: Mutex<Vec<&'static str>>,
        reject_validation: AtomicBool,
        fail_reload: AtomicBool,
    }

    #[async_trait]
    impl GatewayControl for FakeGatewayControl {
        async fn validate(&self, _acl: &str) -> Result<(), GatewayControlError> {
            self.calls.lock().await.push("validate");
            if self.reject_validation.load(Ordering::SeqCst) {
                Err(GatewayControlError::Rejected("invalid ACL".into()))
            } else {
                Ok(())
            }
        }

        async fn reload(&self, _acl: &str) -> Result<(), GatewayControlError> {
            self.calls.lock().await.push("reload");
            if self.fail_reload.load(Ordering::SeqCst) {
                Err(GatewayControlError::Unavailable(
                    "Gateway is offline".into(),
                ))
            } else {
                Ok(())
            }
        }
    }

    fn snapshot(revision: u64, expected_revision: Option<u64>) -> GatewaySnapshot {
        GatewaySnapshot::new(
            revision,
            expected_revision,
            format!("management {{ enabled = true }}\n# revision {revision}\n"),
        )
        .expect("Gateway snapshot")
    }

    #[tokio::test]
    async fn install_is_atomic_compare_and_swap_and_exact_replay_is_local() {
        let directory = tempfile::tempdir().expect("Gateway state directory");
        let control = Arc::new(FakeGatewayControl::default());
        let installer = DurableGatewaySnapshotInstaller::new(
            directory.path().join("gateway.json"),
            control.clone(),
        );
        assert_eq!(
            installer
                .install(&snapshot(1, None))
                .await
                .expect("install"),
            GatewaySnapshotInstallOutcome::Applied
        );
        assert_eq!(
            installer
                .install(&snapshot(1, None))
                .await
                .expect("exact replay"),
            GatewaySnapshotInstallOutcome::Applied
        );
        assert_eq!(&*control.calls.lock().await, &["validate", "reload"]);

        let rejected = installer
            .install(&snapshot(3, Some(2)))
            .await
            .expect("CAS rejection");
        assert!(matches!(
            rejected,
            GatewaySnapshotInstallOutcome::Rejected { .. }
        ));
        assert_eq!(
            installer
                .read_installed()
                .await
                .expect("installed state")
                .expect("installed snapshot")
                .snapshot
                .revision,
            1
        );
    }

    #[tokio::test]
    async fn validation_and_reload_failures_preserve_the_prior_snapshot() {
        let directory = tempfile::tempdir().expect("Gateway state directory");
        let control = Arc::new(FakeGatewayControl::default());
        let installer = DurableGatewaySnapshotInstaller::new(
            directory.path().join("gateway.json"),
            control.clone(),
        );
        installer
            .install(&snapshot(1, None))
            .await
            .expect("initial install");

        control.reject_validation.store(true, Ordering::SeqCst);
        assert!(matches!(
            installer
                .install(&snapshot(2, Some(1)))
                .await
                .expect("validation rejection"),
            GatewaySnapshotInstallOutcome::Rejected { .. }
        ));
        control.reject_validation.store(false, Ordering::SeqCst);
        control.fail_reload.store(true, Ordering::SeqCst);
        assert!(matches!(
            installer.install(&snapshot(2, Some(1))).await,
            Err(GatewaySnapshotInstallError::Unavailable(_))
        ));
        assert_eq!(
            installer
                .read_installed()
                .await
                .expect("installed state")
                .expect("installed snapshot")
                .snapshot
                .revision,
            1
        );
    }

    #[tokio::test]
    async fn installed_a3s_gateway_validates_and_reloads_complete_snapshots() {
        let Ok(binary) = std::env::var("A3S_CLOUD_TEST_GATEWAY_BIN") else {
            return;
        };
        let directory = tempfile::tempdir().expect("real Gateway test directory");
        let (traffic_port, management_port) = unused_ports();
        let token = "a3s-cloud-gateway-integration-token";
        let bootstrap = gateway_acl(traffic_port, management_port, 0);
        let config_path = directory.path().join("gateway.acl");
        std::fs::write(&config_path, &bootstrap).expect("write Gateway bootstrap config");
        let mut gateway = Command::new(binary)
            .arg("--config")
            .arg(&config_path)
            .env("A3S_GATEWAY_ADMIN_TOKEN", token)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start A3S Gateway");

        let result = async {
            let base_url = format!("http://127.0.0.1:{management_port}/api/gateway");
            wait_for_gateway(&base_url, token, &mut gateway).await?;
            let control = Arc::new(GatewayManagementClient::new(
                url::Url::parse(&base_url)?,
                token.into(),
                Duration::from_secs(2),
                Duration::from_secs(2),
                Duration::from_secs(5),
            )?);
            let installer = DurableGatewaySnapshotInstaller::new(
                directory.path().join("installed.json"),
                control,
            );
            let first =
                GatewaySnapshot::new(1, None, gateway_acl(traffic_port, management_port, 1))?;
            if installer.install(&first).await? != GatewaySnapshotInstallOutcome::Applied {
                return Err("real Gateway did not apply the first snapshot".into());
            }
            let second =
                GatewaySnapshot::new(2, Some(1), gateway_acl(traffic_port, management_port, 2))?;
            if installer.install(&second).await? != GatewaySnapshotInstallOutcome::Applied {
                return Err("real Gateway did not apply the second snapshot".into());
            }
            let invalid = GatewaySnapshot::new(3, Some(2), invalid_gateway_acl(management_port))?;
            if !matches!(
                installer.install(&invalid).await?,
                GatewaySnapshotInstallOutcome::Rejected { .. }
            ) {
                return Err("real Gateway accepted invalid ACL".into());
            }
            let installed = installer
                .read_installed()
                .await?
                .ok_or("real Gateway test has no durable snapshot")?;
            if installed.snapshot.revision != 2 {
                return Err("rejected real Gateway reload changed durable state".into());
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        }
        .await;
        let _ = gateway.kill();
        let _ = gateway.wait();
        result.expect("real A3S Gateway snapshot integration");
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

    fn gateway_acl(traffic_port: u16, management_port: u16, revision: u64) -> String {
        format!(
            r#"# revision {revision}
entrypoints "web" {{ address = "127.0.0.1:{traffic_port}" }}

management {{
  enabled = true
  address = "127.0.0.1:{management_port}"
  path_prefix = "/api/gateway"
  auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  allowed_ips = ["127.0.0.1"]
}}
"#
        )
    }

    fn invalid_gateway_acl(management_port: u16) -> String {
        format!(
            r#"entrypoints "web" {{ address = "invalid-address" }}

management {{
  enabled = true
  address = "127.0.0.1:{management_port}"
  path_prefix = "/api/gateway"
  auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  allowed_ips = ["127.0.0.1"]
}}
"#
        )
    }

    async fn wait_for_gateway(
        base_url: &str,
        token: &str,
        child: &mut std::process::Child,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        for _ in 0..100 {
            if child.try_wait()?.is_some() {
                return Err("A3S Gateway exited before its management API was ready".into());
            }
            if client
                .get(format!("{base_url}/version"))
                .bearer_auth(token)
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
}
