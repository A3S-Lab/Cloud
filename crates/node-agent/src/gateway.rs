use crate::gateway_certificate::{
    GatewayCertificateProvisioningError, NodeGatewayCertificateProvisioner,
    SystemGatewayCertificateClock,
};
use crate::{GatewayCertificateSigningTransport, NodeAgentConfig};
use a3s_cloud_contracts::GatewaySnapshot;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

const MAX_MANAGEMENT_RESPONSE_BYTES: usize = 64 * 1024;
const MANAGED_SNAPSHOT_SCHEMA: &str = "a3s.gateway.managed-snapshot.v1";
const MANAGED_SNAPSHOT_STATUS_SCHEMA: &str = "a3s.gateway.managed-snapshot-status.v1";

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
    Unavailable(String),
    #[error("{0}")]
    Protocol(String),
}

#[async_trait]
trait GatewayControl: Send + Sync {
    async fn apply(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<ManagedSnapshotStatus, GatewayControlError>;

    async fn readiness(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<ManagedSnapshotStatus, GatewayControlError>;
}

pub struct DurableGatewaySnapshotInstaller {
    gateway_id: Uuid,
    control: Arc<dyn GatewayControl>,
    certificates: Option<Arc<NodeGatewayCertificateProvisioner>>,
    installation: Mutex<()>,
}

impl DurableGatewaySnapshotInstaller {
    pub fn from_config(
        config: &NodeAgentConfig,
        node_id: Uuid,
        transport: Arc<dyn GatewayCertificateSigningTransport>,
    ) -> Result<Self, GatewaySnapshotInstallError> {
        let token = config
            .gateway_auth_token()
            .map_err(|error| GatewaySnapshotInstallError::InvalidState(error.to_string()))?;
        let control = Arc::new(GatewayManagementClient::new(
            config.gateway.management_url.clone(),
            token,
            Duration::from_millis(config.gateway.connect_timeout_ms),
            Duration::from_millis(config.gateway.apply_timeout_ms),
            Duration::from_millis(config.gateway.readiness_timeout_ms),
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
        Ok(Self::new_with_certificates(node_id, control, certificates))
    }

    #[cfg(test)]
    fn new(gateway_id: Uuid, control: Arc<dyn GatewayControl>) -> Self {
        Self {
            gateway_id,
            control,
            certificates: None,
            installation: Mutex::new(()),
        }
    }

    fn new_with_certificates(
        gateway_id: Uuid,
        control: Arc<dyn GatewayControl>,
        certificates: Arc<NodeGatewayCertificateProvisioner>,
    ) -> Self {
        Self {
            gateway_id,
            control,
            certificates: Some(certificates),
            installation: Mutex::new(()),
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

    fn confirm_status(
        &self,
        snapshot: &GatewaySnapshot,
        status: ManagedSnapshotStatus,
    ) -> Result<GatewaySnapshotInstallOutcome, GatewaySnapshotInstallError> {
        status.validate_shape()?;
        let expected = ManagedSnapshotIdentity::from(snapshot);
        if status.gateway_id != Some(self.gateway_id)
            || status.requested.as_ref() != Some(&expected)
        {
            return Err(GatewaySnapshotInstallError::Protocol(
                "Gateway status does not identify the requested Gateway snapshot".into(),
            ));
        }

        match status.state {
            ManagedSnapshotState::Applied => {
                let Some(applied) = status.applied.as_ref() else {
                    return Err(GatewaySnapshotInstallError::Protocol(
                        "Gateway applied status omitted applied snapshot metadata".into(),
                    ));
                };
                if !applied.matches(snapshot) {
                    return Err(GatewaySnapshotInstallError::Protocol(
                        "Gateway applied status does not match the requested snapshot".into(),
                    ));
                }
                if !status.ready {
                    return Err(GatewaySnapshotInstallError::Unavailable(sanitize_message(
                        status.reason.as_deref().unwrap_or_default(),
                        "Gateway has not confirmed exact snapshot readiness",
                    )));
                }
                Ok(GatewaySnapshotInstallOutcome::Applied)
            }
            ManagedSnapshotState::Rejected => Ok(GatewaySnapshotInstallOutcome::Rejected {
                message: sanitize_message(
                    status.reason.as_deref().unwrap_or_default(),
                    "Gateway rejected the snapshot",
                ),
            }),
            ManagedSnapshotState::Applying => {
                Err(GatewaySnapshotInstallError::Unavailable(sanitize_message(
                    status.reason.as_deref().unwrap_or_default(),
                    "Gateway snapshot application is not yet ready",
                )))
            }
            ManagedSnapshotState::Expired | ManagedSnapshotState::NotApplied => {
                Ok(GatewaySnapshotInstallOutcome::Rejected {
                    message: sanitize_message(
                        status.reason.as_deref().unwrap_or_default(),
                        "Gateway did not apply the requested snapshot",
                    ),
                })
            }
            ManagedSnapshotState::Disabled | ManagedSnapshotState::Uninitialized => {
                Err(GatewaySnapshotInstallError::Protocol(sanitize_message(
                    status.reason.as_deref().unwrap_or_default(),
                    "Gateway native managed snapshots are not initialized",
                )))
            }
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
        if snapshot.gateway_id != self.gateway_id {
            return Ok(GatewaySnapshotInstallOutcome::Rejected {
                message: format!(
                    "Gateway snapshot targets {}, but this node manages Gateway {}",
                    snapshot.gateway_id, self.gateway_id
                ),
            });
        }

        let _installation = self.installation.lock().await;
        if let Some(outcome) = self.provision_certificate(snapshot).await? {
            return Ok(outcome);
        }

        let apply = self
            .control
            .apply(snapshot)
            .await
            .map_err(map_control_error)?;
        let apply_outcome = self.confirm_status(snapshot, apply)?;
        if matches!(
            apply_outcome,
            GatewaySnapshotInstallOutcome::Rejected { .. }
        ) {
            return Ok(apply_outcome);
        }

        let readiness = self
            .control
            .readiness(snapshot)
            .await
            .map_err(map_control_error)?;
        self.confirm_status(snapshot, readiness)
    }
}

#[derive(Debug, Serialize)]
struct ManagedSnapshotRequest<'a> {
    schema: &'static str,
    gateway_id: Uuid,
    revision: u64,
    expected_revision: Option<u64>,
    snapshot_digest: &'a str,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    acl: &'a str,
}

impl<'a> From<&'a GatewaySnapshot> for ManagedSnapshotRequest<'a> {
    fn from(snapshot: &'a GatewaySnapshot) -> Self {
        Self {
            schema: MANAGED_SNAPSHOT_SCHEMA,
            gateway_id: snapshot.gateway_id,
            revision: snapshot.revision,
            expected_revision: snapshot.expected_revision,
            snapshot_digest: &snapshot.snapshot_digest,
            issued_at: snapshot.issued_at,
            expires_at: snapshot.expires_at,
            acl: &snapshot.acl,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedSnapshotIdentity {
    gateway_id: Uuid,
    revision: u64,
    snapshot_digest: String,
}

impl From<&GatewaySnapshot> for ManagedSnapshotIdentity {
    fn from(snapshot: &GatewaySnapshot) -> Self {
        Self {
            gateway_id: snapshot.gateway_id,
            revision: snapshot.revision,
            snapshot_digest: snapshot.snapshot_digest.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ManagedSnapshotState {
    Disabled,
    Uninitialized,
    Applying,
    Applied,
    Rejected,
    Expired,
    NotApplied,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppliedManagedSnapshot {
    gateway_id: Uuid,
    revision: u64,
    expected_revision: Option<u64>,
    snapshot_digest: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    applied_at: DateTime<Utc>,
}

impl AppliedManagedSnapshot {
    fn matches(&self, snapshot: &GatewaySnapshot) -> bool {
        self.gateway_id == snapshot.gateway_id
            && self.revision == snapshot.revision
            && self.expected_revision == snapshot.expected_revision
            && self.snapshot_digest == snapshot.snapshot_digest
            && self.issued_at == snapshot.issued_at
            && self.expires_at == snapshot.expires_at
            && self.applied_at >= snapshot.issued_at
            && self.applied_at < snapshot.expires_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectedManagedSnapshot {
    gateway_id: Uuid,
    revision: u64,
    snapshot_digest: String,
    rejected_at: DateTime<Utc>,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedSnapshotStatus {
    schema: String,
    gateway_id: Option<Uuid>,
    #[serde(default)]
    requested: Option<ManagedSnapshotIdentity>,
    state: ManagedSnapshotState,
    ready: bool,
    replayed: bool,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    applied: Option<AppliedManagedSnapshot>,
    #[serde(default)]
    last_rejection: Option<RejectedManagedSnapshot>,
}

impl ManagedSnapshotStatus {
    fn validate_shape(&self) -> Result<(), GatewaySnapshotInstallError> {
        if self.schema != MANAGED_SNAPSHOT_STATUS_SCHEMA {
            return Err(GatewaySnapshotInstallError::Protocol(format!(
                "unsupported Gateway managed snapshot status schema {:?}",
                self.schema
            )));
        }
        if self.ready && self.state != ManagedSnapshotState::Applied {
            return Err(GatewaySnapshotInstallError::Protocol(
                "Gateway reported readiness for a non-applied snapshot".into(),
            ));
        }
        Ok(())
    }
}

struct GatewayManagementClient {
    client: reqwest::Client,
    base_url: url::Url,
    token: String,
    apply_timeout: Duration,
    readiness_timeout: Duration,
}

impl GatewayManagementClient {
    fn new(
        mut management_url: url::Url,
        token: String,
        connect_timeout: Duration,
        apply_timeout: Duration,
        readiness_timeout: Duration,
    ) -> Result<Self, GatewaySnapshotInstallError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(connect_timeout)
            .build()
            .map_err(|error| GatewaySnapshotInstallError::Protocol(error.to_string()))?;
        if !management_url.path().ends_with('/') {
            let path = format!("{}/", management_url.path());
            management_url.set_path(&path);
        }
        Ok(Self {
            client,
            base_url: management_url,
            token,
            apply_timeout,
            readiness_timeout,
        })
    }

    async fn decode(
        response: reqwest::Response,
    ) -> Result<ManagedSnapshotStatus, GatewayControlError> {
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
        if matches!(
            status,
            StatusCode::OK
                | StatusCode::CONFLICT
                | StatusCode::UNPROCESSABLE_ENTITY
                | StatusCode::SERVICE_UNAVAILABLE
        ) {
            return serde_json::from_slice(&body)
                .map_err(|error| GatewayControlError::Protocol(error.to_string()));
        }

        let message = management_error_message(&body, status.as_u16());
        if status.is_server_error()
            || status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::TOO_MANY_REQUESTS
        {
            Err(GatewayControlError::Unavailable(message))
        } else {
            Err(GatewayControlError::Protocol(message))
        }
    }
}

#[async_trait]
impl GatewayControl for GatewayManagementClient {
    async fn apply(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<ManagedSnapshotStatus, GatewayControlError> {
        let url = self
            .base_url
            .join("snapshots/apply")
            .map_err(|error| GatewayControlError::Protocol(error.to_string()))?;
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.token)
            .json(&ManagedSnapshotRequest::from(snapshot))
            .timeout(self.apply_timeout)
            .send()
            .await
            .map_err(|error| GatewayControlError::Unavailable(error.to_string()))?;
        Self::decode(response).await
    }

    async fn readiness(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<ManagedSnapshotStatus, GatewayControlError> {
        let mut url = self
            .base_url
            .join("snapshots/status")
            .map_err(|error| GatewayControlError::Protocol(error.to_string()))?;
        url.query_pairs_mut()
            .append_pair("gateway_id", &snapshot.gateway_id.to_string())
            .append_pair("revision", &snapshot.revision.to_string())
            .append_pair("snapshot_digest", &snapshot.snapshot_digest);
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .timeout(self.readiness_timeout)
            .send()
            .await
            .map_err(|error| GatewayControlError::Unavailable(error.to_string()))?;
        Self::decode(response).await
    }
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

fn map_control_error(error: GatewayControlError) -> GatewaySnapshotInstallError {
    match error {
        GatewayControlError::Unavailable(message) => GatewaySnapshotInstallError::Unavailable(
            sanitize_message(&message, "Gateway management API is unavailable"),
        ),
        GatewayControlError::Protocol(message) => GatewaySnapshotInstallError::Protocol(
            sanitize_message(&message, "Gateway management response is invalid"),
        ),
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
    use chrono::Duration as ChronoDuration;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct FakeGatewayControl {
        calls: Mutex<Vec<&'static str>>,
        reject_apply: AtomicBool,
        fail_readiness: AtomicBool,
        wrong_digest: AtomicBool,
    }

    fn status(
        snapshot: &GatewaySnapshot,
        state: ManagedSnapshotState,
        ready: bool,
        reason: Option<&str>,
    ) -> ManagedSnapshotStatus {
        ManagedSnapshotStatus {
            schema: MANAGED_SNAPSHOT_STATUS_SCHEMA.into(),
            gateway_id: Some(snapshot.gateway_id),
            requested: Some(ManagedSnapshotIdentity::from(snapshot)),
            state,
            ready,
            replayed: false,
            reason: reason.map(str::to_owned),
            applied: (state == ManagedSnapshotState::Applied).then(|| AppliedManagedSnapshot {
                gateway_id: snapshot.gateway_id,
                revision: snapshot.revision,
                expected_revision: snapshot.expected_revision,
                snapshot_digest: snapshot.snapshot_digest.clone(),
                issued_at: snapshot.issued_at,
                expires_at: snapshot.expires_at,
                applied_at: snapshot.issued_at + ChronoDuration::milliseconds(1),
            }),
            last_rejection: None,
        }
    }

    #[async_trait]
    impl GatewayControl for FakeGatewayControl {
        async fn apply(
            &self,
            snapshot: &GatewaySnapshot,
        ) -> Result<ManagedSnapshotStatus, GatewayControlError> {
            self.calls.lock().await.push("apply");
            if self.reject_apply.load(Ordering::SeqCst) {
                return Ok(status(
                    snapshot,
                    ManagedSnapshotState::Rejected,
                    false,
                    Some("invalid ACL"),
                ));
            }
            let mut status = status(snapshot, ManagedSnapshotState::Applied, true, None);
            if self.wrong_digest.load(Ordering::SeqCst) {
                status
                    .applied
                    .as_mut()
                    .expect("applied metadata")
                    .snapshot_digest = format!("sha256:{}", "f".repeat(64));
            }
            Ok(status)
        }

        async fn readiness(
            &self,
            snapshot: &GatewaySnapshot,
        ) -> Result<ManagedSnapshotStatus, GatewayControlError> {
            self.calls.lock().await.push("readiness");
            if self.fail_readiness.load(Ordering::SeqCst) {
                Err(GatewayControlError::Unavailable(
                    "Gateway is restarting".into(),
                ))
            } else {
                Ok(status(snapshot, ManagedSnapshotState::Applied, true, None))
            }
        }
    }

    fn snapshot(
        gateway_id: Uuid,
        revision: u64,
        expected_revision: Option<u64>,
    ) -> GatewaySnapshot {
        let issued_at = Utc::now();
        GatewaySnapshot::new(
            gateway_id,
            revision,
            expected_revision,
            issued_at,
            issued_at + ChronoDuration::minutes(10),
            format!("management {{ enabled = true }}\n# revision {revision}\n"),
        )
        .expect("Gateway snapshot")
    }

    #[tokio::test]
    async fn install_uses_native_apply_and_exact_readiness_for_replay() {
        let gateway_id = Uuid::now_v7();
        let control = Arc::new(FakeGatewayControl::default());
        let installer = DurableGatewaySnapshotInstaller::new(gateway_id, control.clone());
        let snapshot = snapshot(gateway_id, 1, None);

        assert_eq!(
            installer.install(&snapshot).await.expect("install"),
            GatewaySnapshotInstallOutcome::Applied
        );
        assert_eq!(
            installer.install(&snapshot).await.expect("exact replay"),
            GatewaySnapshotInstallOutcome::Applied
        );
        assert_eq!(
            &*control.calls.lock().await,
            &["apply", "readiness", "apply", "readiness"]
        );
    }

    #[tokio::test]
    async fn rejection_and_readiness_failure_never_report_applied() {
        let gateway_id = Uuid::now_v7();
        let control = Arc::new(FakeGatewayControl::default());
        let installer = DurableGatewaySnapshotInstaller::new(gateway_id, control.clone());
        let snapshot = snapshot(gateway_id, 1, None);

        control.reject_apply.store(true, Ordering::SeqCst);
        assert!(matches!(
            installer.install(&snapshot).await.expect("rejection"),
            GatewaySnapshotInstallOutcome::Rejected { .. }
        ));
        control.reject_apply.store(false, Ordering::SeqCst);
        control.fail_readiness.store(true, Ordering::SeqCst);
        assert!(matches!(
            installer.install(&snapshot).await,
            Err(GatewaySnapshotInstallError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn mismatched_gateway_status_is_a_protocol_failure() {
        let gateway_id = Uuid::now_v7();
        let control = Arc::new(FakeGatewayControl::default());
        control.wrong_digest.store(true, Ordering::SeqCst);
        let installer = DurableGatewaySnapshotInstaller::new(gateway_id, control);

        assert!(matches!(
            installer.install(&snapshot(gateway_id, 1, None)).await,
            Err(GatewaySnapshotInstallError::Protocol(_))
        ));
    }
}

#[cfg(test)]
#[path = "gateway_remote_tests.rs"]
mod remote_tests;

#[cfg(test)]
#[path = "gateway_reload_crash_tests.rs"]
mod reload_crash_tests;
