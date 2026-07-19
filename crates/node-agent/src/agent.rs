use crate::control_plane::{CertificateReloadError, ReloadableNodeControlClient};
use crate::state_file::{self, StateLock};
use crate::{
    CommandExecutionError, CommandExecutor, CommandJournalError, DurableGatewaySnapshotInstaller,
    EnrolledNodeIdentity, FileCommandJournal, FileNodeIdentityStore, GatewaySnapshotInstallError,
    GatewaySnapshotInstaller, IdentityStoreError, NodeAgentConfig, NodeControlClient,
    NodeControlClientError, NodeControlTransport, NodeIdentityState,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandAckReceipt, NodeCommandOutcome, NodeCommandResult, NodeGatewayAck,
    NodeGatewayAckReceipt, NodeHeartbeat, NodeObservationBatch, RuntimeObservationReport,
};
use a3s_runtime::contract::{RuntimeCapabilities, RuntimeInspection, RuntimeObservation};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use async_trait::async_trait;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

const PROCESS_LOCK_FILE: &str = "node-agent.lock";
const MAX_COMMANDS_PER_LEASE: u16 = 16;

pub async fn run_node_agent(
    config: NodeAgentConfig,
    runtime: NodeRuntimeProvider,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), NodeAgentError> {
    let _process_lock = acquire_process_lock(&config.node.state_dir).await?;
    let gateway: Arc<dyn GatewaySnapshotInstaller> =
        Arc::new(DurableGatewaySnapshotInstaller::from_config(&config)?);
    let capabilities = runtime.client.capabilities().await?;
    capabilities.validate().map_err(NodeAgentError::Invalid)?;

    let identity_store = FileNodeIdentityStore::new(config.node.state_dir.clone());
    let Some(identity) =
        ensure_enrolled(&config, &identity_store, &capabilities, &mut shutdown).await?
    else {
        return Ok(());
    };
    let client = NodeControlClient::new(&config.control_plane, &identity).await?;
    let transport = Arc::new(ReloadableNodeControlClient::new(client));
    let Some(identity) = ensure_current_certificate(
        &config,
        &identity_store,
        &transport,
        identity,
        &mut shutdown,
    )
    .await?
    else {
        return Ok(());
    };
    runtime.binding.bind_node(identity.response.node_id).await?;
    let session_transport: Arc<dyn NodeControlTransport> = transport.clone();
    let session = NodeAgentSession::new(
        session_transport,
        runtime.client,
        gateway,
        identity,
        capabilities,
        env!("CARGO_PKG_VERSION").into(),
        config.node.state_dir.clone(),
        Duration::from_millis(config.control_plane.retry_initial_ms),
        Duration::from_millis(config.control_plane.retry_max_ms),
    )?;
    let session_run = session.run(shutdown.clone());
    let rotation_run = certificate_rotation_loop(config, identity_store, transport, shutdown);
    tokio::pin!(session_run, rotation_run);
    tokio::select! {
        result = &mut session_run => result,
        result = &mut rotation_run => result,
    }
}

#[async_trait]
pub trait NodeRuntimeBinding: Send + Sync {
    async fn bind_node(&self, node_id: uuid::Uuid) -> RuntimeResult<()>;
}

pub struct NodeRuntimeProvider {
    client: Arc<dyn RuntimeClient>,
    binding: Arc<dyn NodeRuntimeBinding>,
}

impl NodeRuntimeProvider {
    pub fn new(client: Arc<dyn RuntimeClient>, binding: Arc<dyn NodeRuntimeBinding>) -> Self {
        Self { client, binding }
    }
}

pub struct NodeAgentSession {
    transport: Arc<dyn NodeControlTransport>,
    executor: CommandExecutor,
    identity: EnrolledNodeIdentity,
    capabilities: RuntimeCapabilities,
    agent_version: String,
    retry_initial: Duration,
    retry_maximum: Duration,
}

impl NodeAgentSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transport: Arc<dyn NodeControlTransport>,
        runtime: Arc<dyn RuntimeClient>,
        gateway: Arc<dyn GatewaySnapshotInstaller>,
        identity: EnrolledNodeIdentity,
        capabilities: RuntimeCapabilities,
        agent_version: String,
        state_dir: PathBuf,
        retry_initial: Duration,
        retry_maximum: Duration,
    ) -> Result<Self, NodeAgentError> {
        identity
            .response
            .validate()
            .map_err(NodeAgentError::Invalid)?;
        capabilities.validate().map_err(NodeAgentError::Invalid)?;
        if agent_version.trim().is_empty()
            || agent_version.len() > 255
            || agent_version.contains(['\0', '\r', '\n'])
            || retry_initial.is_zero()
            || retry_maximum < retry_initial
        {
            return Err(NodeAgentError::Invalid(
                "node-agent session identity or retry policy is invalid".into(),
            ));
        }
        let journal = FileCommandJournal::new(state_dir, identity.response.node_id)?;
        Ok(Self {
            transport,
            executor: CommandExecutor::new(journal, runtime, gateway),
            identity,
            capabilities,
            agent_version,
            retry_initial,
            retry_maximum,
        })
    }

    pub async fn run(&self, shutdown: watch::Receiver<bool>) -> Result<(), NodeAgentError> {
        let command_loop = self.command_loop();
        let heartbeat_loop = self.heartbeat_loop();
        let shutdown = wait_for_shutdown(shutdown);
        tokio::pin!(command_loop, heartbeat_loop, shutdown);
        tokio::select! {
            result = &mut command_loop => result,
            result = &mut heartbeat_loop => result,
            () = &mut shutdown => Ok(()),
        }
    }

    pub async fn synchronize_once(&self) -> Result<(), NodeAgentError> {
        let mut must_redeliver = false;
        for acknowledgement in self.executor.journal().pending_acknowledgements().await? {
            match self.deliver_completion(&acknowledgement).await? {
                Delivery::Acknowledged => {}
                Delivery::RequiresRedelivery => {
                    must_redeliver = true;
                    break;
                }
            }
        }

        let after_sequence = self.executor.journal().after_sequence().await?;
        let response = self
            .transport
            .lease(
                after_sequence,
                MAX_COMMANDS_PER_LEASE,
                self.identity.response.command_long_poll_ms,
            )
            .await?;
        response
            .validate(Utc::now())
            .map_err(NodeAgentError::Invalid)?;
        if response.node_id != self.identity.response.node_id
            || response.agent_instance_id != self.identity.agent_instance_id
        {
            return Err(NodeAgentError::Invalid(
                "leased command batch belongs to a different node-agent identity".into(),
            ));
        }

        for command in response.commands {
            let acknowledgement = self.executor.execute(command).await?;
            if self.deliver_completion(&acknowledgement).await? == Delivery::RequiresRedelivery {
                return Ok(());
            }
        }
        if must_redeliver && self.executor.journal().after_sequence().await? == after_sequence {
            tracing::debug!(
                after_sequence,
                "waiting for the control plane to redeliver an expired command lease"
            );
        }
        Ok(())
    }

    pub async fn heartbeat_once(&self) -> Result<(), NodeAgentError> {
        let batch = self.observation_batch(Vec::new());
        batch.validate().map_err(NodeAgentError::Invalid)?;
        let receipt = self.transport.record_observations(&batch).await?;
        receipt.validate().map_err(NodeAgentError::Invalid)?;
        if receipt.node_id != batch.node_id
            || receipt.accepted_reports != 0
            || receipt.replayed_reports != 0
        {
            return Err(NodeAgentError::Invalid(
                "heartbeat receipt changed the node identity or report count".into(),
            ));
        }
        Ok(())
    }

    async fn command_loop(&self) -> Result<(), NodeAgentError> {
        let mut backoff = ExponentialBackoff::new(self.retry_initial, self.retry_maximum);
        loop {
            match self.synchronize_once().await {
                Ok(()) => backoff.reset(),
                Err(error) if error.retryable() => {
                    let delay = backoff.next_delay();
                    tracing::warn!(error = %error, ?delay, "node command synchronization will retry");
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn heartbeat_loop(&self) -> Result<(), NodeAgentError> {
        let interval = Duration::from_millis(self.identity.response.heartbeat_interval_ms);
        let mut backoff = ExponentialBackoff::new(self.retry_initial, self.retry_maximum);
        loop {
            match self.heartbeat_once().await {
                Ok(()) => {
                    backoff.reset();
                    tokio::time::sleep(interval).await;
                }
                Err(error) if error.retryable() => {
                    let delay = backoff.next_delay();
                    tracing::warn!(error = %error, ?delay, "node heartbeat will retry");
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn deliver_completion(
        &self,
        acknowledgement: &NodeCommandAck,
    ) -> Result<Delivery, NodeAgentError> {
        if let Some(report) = completion_observation(acknowledgement) {
            let batch = self.observation_batch(vec![report]);
            batch.validate().map_err(NodeAgentError::Invalid)?;
            let receipt = self.transport.record_observations(&batch).await?;
            receipt.validate().map_err(NodeAgentError::Invalid)?;
            if receipt.node_id != batch.node_id
                || usize::from(receipt.accepted_reports) + usize::from(receipt.replayed_reports)
                    != 1
            {
                return Err(NodeAgentError::Invalid(
                    "command observation receipt changed the node identity or report count".into(),
                ));
            }
        }

        if let Some(gateway_acknowledgement) = completion_gateway_ack(acknowledgement) {
            let receipt = self
                .transport
                .record_gateway_acknowledgement(gateway_acknowledgement)
                .await?;
            validate_gateway_acknowledgement_receipt(gateway_acknowledgement, &receipt)?;
        }

        let receipt = match self.transport.acknowledge(acknowledgement).await {
            Ok(receipt) => receipt,
            Err(error) if error.requires_command_redelivery() => {
                return Ok(Delivery::RequiresRedelivery)
            }
            Err(error) => return Err(error.into()),
        };
        validate_acknowledgement_receipt(acknowledgement, &receipt)?;
        self.executor.journal().mark_acknowledged(receipt).await?;
        Ok(Delivery::Acknowledged)
    }

    fn observation_batch(
        &self,
        observations: Vec<RuntimeObservationReport>,
    ) -> NodeObservationBatch {
        let newest_report = observations.iter().map(|report| report.observed_at).max();
        let now = Utc::now();
        let sent_at = newest_report.map_or(now, |observed_at| now.max(observed_at));
        NodeObservationBatch {
            schema: NodeObservationBatch::SCHEMA.into(),
            node_id: self.identity.response.node_id,
            agent_instance_id: self.identity.agent_instance_id,
            sent_at,
            heartbeat: NodeHeartbeat {
                schema: NodeHeartbeat::SCHEMA.into(),
                node_id: self.identity.response.node_id,
                agent_instance_id: self.identity.agent_instance_id,
                observed_at: sent_at,
                agent_version: self.agent_version.clone(),
                runtime_capabilities: self.capabilities.clone(),
            },
            observations,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Acknowledged,
    RequiresRedelivery,
}

fn completion_observation(acknowledgement: &NodeCommandAck) -> Option<RuntimeObservationReport> {
    let observation: &RuntimeObservation = match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::RuntimeApplied { observation } => observation,
            NodeCommandResult::RuntimeInspected {
                inspection: RuntimeInspection::Found { observation, .. },
            }
            | NodeCommandResult::RuntimeStopped {
                inspection: RuntimeInspection::Found { observation, .. },
            } => observation,
            NodeCommandResult::RuntimeInspected { .. }
            | NodeCommandResult::RuntimeStopped { .. }
            | NodeCommandResult::RuntimeRemoved { .. }
            | NodeCommandResult::GatewaySnapshotInstalled { .. } => return None,
        },
        NodeCommandOutcome::Rejected { .. } | NodeCommandOutcome::Failed { .. } => return None,
    };
    Some(RuntimeObservationReport {
        report_id: acknowledgement.command_id,
        command_id: Some(acknowledgement.command_id),
        observed_at: acknowledgement.completed_at,
        observation: observation.clone(),
    })
}

fn completion_gateway_ack(acknowledgement: &NodeCommandAck) -> Option<&NodeGatewayAck> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } => {
                Some(acknowledgement)
            }
            NodeCommandResult::RuntimeApplied { .. }
            | NodeCommandResult::RuntimeInspected { .. }
            | NodeCommandResult::RuntimeStopped { .. }
            | NodeCommandResult::RuntimeRemoved { .. } => None,
        },
        NodeCommandOutcome::Rejected { .. } | NodeCommandOutcome::Failed { .. } => None,
    }
}

fn validate_gateway_acknowledgement_receipt(
    acknowledgement: &NodeGatewayAck,
    receipt: &NodeGatewayAckReceipt,
) -> Result<(), NodeAgentError> {
    receipt.validate().map_err(NodeAgentError::Invalid)?;
    if receipt.acknowledgement_id != acknowledgement.acknowledgement_id
        || receipt.command_id != acknowledgement.command_id
        || receipt.node_id != acknowledgement.node_id
    {
        return Err(NodeAgentError::Invalid(
            "Gateway acknowledgement receipt changed the publication identity".into(),
        ));
    }
    Ok(())
}

fn validate_acknowledgement_receipt(
    acknowledgement: &NodeCommandAck,
    receipt: &NodeCommandAckReceipt,
) -> Result<(), NodeAgentError> {
    receipt.validate().map_err(NodeAgentError::Invalid)?;
    if receipt.command_id != acknowledgement.command_id
        || receipt.node_id != acknowledgement.node_id
    {
        return Err(NodeAgentError::Invalid(
            "command acknowledgement receipt changed the command identity".into(),
        ));
    }
    Ok(())
}

async fn ensure_enrolled(
    config: &NodeAgentConfig,
    store: &FileNodeIdentityStore,
    capabilities: &RuntimeCapabilities,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<Option<EnrolledNodeIdentity>, NodeAgentError> {
    let state = store
        .prepare(
            config.node.name.clone(),
            env!("CARGO_PKG_VERSION").into(),
            capabilities.clone(),
        )
        .await?;
    let pending = match state {
        NodeIdentityState::Enrolled(identity) => return Ok(Some(identity)),
        NodeIdentityState::Pending(identity) => identity,
    };
    let enrollment_token = config.enrollment_token()?;
    let mut backoff = ExponentialBackoff::new(
        Duration::from_millis(config.control_plane.retry_initial_ms),
        Duration::from_millis(config.control_plane.retry_max_ms),
    );
    loop {
        let enrollment =
            NodeControlClient::enroll(&config.control_plane, &pending, enrollment_token.clone());
        tokio::select! {
            result = enrollment => match result {
                Ok(response) => return store.complete(response).await.map(Some).map_err(Into::into),
                Err(error) if error.retryable() => {
                    let delay = backoff.next_delay();
                    tracing::warn!(error = %error, ?delay, "node enrollment will retry");
                    if sleep_or_shutdown(delay, shutdown).await {
                        return Ok(None);
                    }
                }
                Err(error) => return Err(error.into()),
            },
            _ = wait_for_shutdown(shutdown.clone()) => return Ok(None),
        }
    }
}

async fn ensure_current_certificate(
    config: &NodeAgentConfig,
    store: &FileNodeIdentityStore,
    transport: &ReloadableNodeControlClient,
    mut identity: EnrolledNodeIdentity,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<Option<EnrolledNodeIdentity>, NodeAgentError> {
    if certificate_rotation_delay(&identity, Utc::now())? > Duration::ZERO {
        return Ok(Some(identity));
    }
    let mut backoff = ExponentialBackoff::new(
        Duration::from_millis(config.control_plane.retry_initial_ms),
        Duration::from_millis(config.control_plane.retry_max_ms),
    );
    loop {
        let prepared = store.prepare_rotation().await?;
        match transport
            .rotate(&config.control_plane, store, &prepared)
            .await
        {
            Ok(rotated) => {
                identity = rotated;
                return Ok(Some(identity));
            }
            Err(error) if error.retryable() => {
                let delay = backoff.next_delay();
                tracing::warn!(error = %error, ?delay, "node certificate rotation will retry before startup");
                if sleep_or_shutdown(delay, shutdown).await {
                    return Ok(None);
                }
            }
            Err(error) => return Err(reload_error(error)),
        }
    }
}

async fn certificate_rotation_loop(
    config: NodeAgentConfig,
    store: FileNodeIdentityStore,
    transport: Arc<ReloadableNodeControlClient>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), NodeAgentError> {
    let mut backoff = ExponentialBackoff::new(
        Duration::from_millis(config.control_plane.retry_initial_ms),
        Duration::from_millis(config.control_plane.retry_max_ms),
    );
    loop {
        let identity = match store.load().await? {
            Some(NodeIdentityState::Enrolled(identity)) => identity,
            Some(NodeIdentityState::Pending(_)) | None => {
                return Err(NodeAgentError::Invalid(
                    "node certificate maintenance requires an enrolled identity".into(),
                ))
            }
        };
        let delay = certificate_rotation_delay(&identity, Utc::now())?;
        if !delay.is_zero() && sleep_or_shutdown(delay, &mut shutdown).await {
            return Ok(());
        }
        let prepared = store.prepare_rotation().await?;
        match transport
            .rotate(&config.control_plane, &store, &prepared)
            .await
        {
            Ok(_) => backoff.reset(),
            Err(error) if error.retryable() => {
                let delay = backoff.next_delay();
                tracing::warn!(error = %error, ?delay, "node certificate rotation will retry");
                if sleep_or_shutdown(delay, &mut shutdown).await {
                    return Ok(());
                }
            }
            Err(error) => return Err(reload_error(error)),
        }
    }
}

fn certificate_rotation_delay(
    identity: &EnrolledNodeIdentity,
    now: chrono::DateTime<Utc>,
) -> Result<Duration, NodeAgentError> {
    if identity.has_pending_rotation() {
        return Ok(Duration::ZERO);
    }
    if now >= identity.response.certificate.expires_at {
        return Err(NodeAgentError::Invalid(
            "node certificate expired before it could be rotated".into(),
        ));
    }
    let window_ms = i64::try_from(identity.response.certificate_rotation_window_ms)
        .map_err(|_| NodeAgentError::Invalid("certificate rotation window overflowed".into()))?;
    let rotation_at = identity
        .response
        .certificate
        .expires_at
        .checked_sub_signed(chrono::Duration::milliseconds(window_ms))
        .ok_or_else(|| NodeAgentError::Invalid("certificate rotation time overflowed".into()))?;
    if rotation_at <= now {
        return Ok(Duration::ZERO);
    }
    (rotation_at - now).to_std().map_err(|error| {
        NodeAgentError::Invalid(format!("certificate rotation delay is invalid: {error}"))
    })
}

fn reload_error(error: CertificateReloadError) -> NodeAgentError {
    match error {
        CertificateReloadError::ControlPlane(error) => NodeAgentError::ControlPlane(error),
        CertificateReloadError::Identity(error) => NodeAgentError::Identity(error),
    }
}

async fn acquire_process_lock(root: &Path) -> Result<StateLock, NodeAgentError> {
    let root = root.to_owned();
    tokio::task::spawn_blocking(move || {
        state_file::ensure_directory(&root)
            .and_then(|()| StateLock::try_exclusive(&root.join(PROCESS_LOCK_FILE)))
            .map_err(|error| NodeAgentError::State(error.to_string()))
    })
    .await
    .map_err(|error| NodeAgentError::State(format!("process-lock task failed: {error}")))?
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}

async fn sleep_or_shutdown(delay: Duration, shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        () = tokio::time::sleep(delay) => false,
        () = wait_for_shutdown(shutdown.clone()) => true,
    }
}

struct ExponentialBackoff {
    initial: Duration,
    maximum: Duration,
    next: Duration,
}

impl ExponentialBackoff {
    fn new(initial: Duration, maximum: Duration) -> Self {
        Self {
            initial,
            maximum,
            next: initial,
        }
    }

    fn reset(&mut self) {
        self.next = self.initial;
    }

    fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(self.maximum);
        delay
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NodeAgentError {
    #[error("invalid node-agent state: {0}")]
    Invalid(String),
    #[error("node-agent secure state failed: {0}")]
    State(String),
    #[error(transparent)]
    Config(#[from] crate::ConfigError),
    #[error(transparent)]
    Identity(#[from] IdentityStoreError),
    #[error(transparent)]
    Journal(#[from] CommandJournalError),
    #[error(transparent)]
    Execution(#[from] CommandExecutionError),
    #[error(transparent)]
    ControlPlane(#[from] NodeControlClientError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Gateway(#[from] GatewaySnapshotInstallError),
}

impl NodeAgentError {
    pub fn retryable(&self) -> bool {
        match self {
            Self::ControlPlane(error) => error.retryable(),
            Self::Runtime(RuntimeError::ProviderUnavailable(_) | RuntimeError::Transport(_)) => {
                true
            }
            Self::Gateway(error) => error.retryable(),
            Self::Invalid(_)
            | Self::State(_)
            | Self::Config(_)
            | Self::Identity(_)
            | Self::Journal(_)
            | Self::Execution(_)
            | Self::Runtime(_) => false,
        }
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
